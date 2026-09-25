//! Cross-table operations that do not belong to a single route module.

use std::collections::HashMap;
use std::pin::Pin;

use sqlx::{Sqlite, Transaction};

use crate::compile::rows::ScenarioGraph;
use crate::db::Db;
use crate::error::{ApiError, ApiResult, on_unique_violation};

type Remap = HashMap<i64, i64>;
type Fut<'a, T> = Pin<Box<dyn Future<Output = ApiResult<T>> + Send + 'a>>;

/// Deep-copy `graph` into a new scenario named `name`, owned by the same user.
///
/// Every table is copied with fresh primary keys and each foreign key is
/// rewritten through the appropriate remap, so the clone is fully independent
/// of the original.
pub async fn clone_scenario(db: &Db, graph: &ScenarioGraph, name: &str) -> ApiResult<i64> {
    let mut tx = db.begin().await?;
    let id = clone_into(&mut tx, graph, name).await?;
    tx.commit().await?;
    Ok(id)
}

/// Copy graph records inside an existing import/setup transaction.
pub(crate) async fn clone_into(
    tx: &mut Transaction<'_, Sqlite>,
    graph: &ScenarioGraph,
    name: &str,
) -> ApiResult<i64> {
    if name.is_empty() {
        return Err(ApiError::bad_request("scenario name cannot be empty"));
    }

    let src = &graph.scenario;

    let new_id: i64 = sqlx::query_scalar(
        "INSERT INTO scenarios
            (user_id, name, description, start_date, birth_date, duration_years,
             inflation_profile_id, tax_config_id, collect_ledger)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) RETURNING id",
    )
    .bind(&src.user_id)
    .bind(name)
    .bind(&src.description)
    .bind(&src.start_date)
    .bind(&src.birth_date)
    .bind(src.duration_years)
    .bind(src.inflation_profile_id)
    .bind(src.tax_config_id)
    .bind(src.collect_ledger)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, "a scenario with that name already exists"))?;

    // ── assets ──────────────────────────────────────────────────────────────
    let mut assets: Remap = HashMap::new();
    for asset in &graph.assets {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO assets
                (scenario_id, name, description, initial_price, return_profile_id,
                 tracking_error, sort_order)
             VALUES (?1,?2,?3,?4,?5,?6,?7) RETURNING id",
        )
        .bind(new_id)
        .bind(&asset.name)
        .bind(&asset.description)
        .bind(asset.initial_price)
        .bind(asset.return_profile_id) // profiles are user-scoped, shared as-is
        .bind(asset.tracking_error)
        .bind(asset.sort_order)
        .fetch_one(&mut **tx)
        .await?;
        assets.insert(asset.id, id);
    }

    // ── accounts and their per-flavor detail rows ───────────────────────────
    let mut accounts: Remap = HashMap::new();
    for account in &graph.accounts {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO accounts (scenario_id, name, description, flavor, sort_order)
             VALUES (?1,?2,?3,?4,?5) RETURNING id",
        )
        .bind(new_id)
        .bind(&account.name)
        .bind(&account.description)
        .bind(&account.flavor)
        .bind(account.sort_order)
        .fetch_one(&mut **tx)
        .await?;
        accounts.insert(account.id, id);

        if let Some(bank) = graph.bank.get(&account.id) {
            sqlx::query(
                "INSERT INTO account_bank (account_id, cash_value, return_profile_id)
                 VALUES (?1,?2,?3)",
            )
            .bind(id)
            .bind(bank.cash_value)
            .bind(bank.return_profile_id)
            .execute(&mut **tx)
            .await?;
        }
        if let Some(inv) = graph.investment.get(&account.id) {
            sqlx::query(
                "INSERT INTO account_investment
                    (account_id, tax_status, cash_value, cash_return_profile_id,
                     contribution_limit, contribution_period)
                 VALUES (?1,?2,?3,?4,?5,?6)",
            )
            .bind(id)
            .bind(&inv.tax_status)
            .bind(inv.cash_value)
            .bind(inv.cash_return_profile_id)
            .bind(inv.contribution_limit)
            .bind(&inv.contribution_period)
            .execute(&mut **tx)
            .await?;
        }
        if let Some(prop) = graph.property.get(&account.id) {
            let asset_id = remap(&assets, prop.asset_id, "asset")?;
            sqlx::query(
                "INSERT INTO account_property (account_id, asset_id, value) VALUES (?1,?2,?3)",
            )
            .bind(id)
            .bind(asset_id)
            .bind(prop.value)
            .execute(&mut **tx)
            .await?;
        }
        if let Some(loan) = graph.liability.get(&account.id) {
            // The payer may not be copied yet; it is linked up once every
            // account has its new id, below.
            sqlx::query(
                "INSERT INTO account_liability (account_id, principal, interest_rate, term_months)
                 VALUES (?1,?2,?3,?4)",
            )
            .bind(id)
            .bind(loan.principal)
            .bind(loan.interest_rate)
            .bind(loan.term_months)
            .execute(&mut **tx)
            .await?;
        }

        // Enumerated rather than copying a stored rank: the graph loads lots
        // in `sort_order`, so the position in that list *is* the order, and
        // the copy is renumbered densely from it.
        for (rank, lot) in graph
            .positions
            .get(&account.id)
            .into_iter()
            .flatten()
            .enumerate()
        {
            sqlx::query(
                "INSERT INTO positions
                    (account_id, asset_id, purchase_date, units, cost_basis, sort_order)
                 VALUES (?1,?2,?3,?4,?5,?6)",
            )
            .bind(id)
            .bind(remap(&assets, lot.asset_id, "asset")?)
            .bind(&lot.purchase_date)
            .bind(lot.units)
            .bind(lot.cost_basis)
            .bind(rank as i64)
            .execute(&mut **tx)
            .await?;
        }
    }

    for loan in graph.liability.values() {
        if let Some(from) = loan.repay_from_account_id {
            sqlx::query(
                "UPDATE account_liability SET repay_from_account_id = ?2 WHERE account_id = ?1",
            )
            .bind(remap(&accounts, loan.account_id, "account")?)
            .bind(remap(&accounts, from, "account")?)
            .execute(&mut **tx)
            .await?;
        }
    }

    let mut parameters: Remap = HashMap::new();
    for parameter in &graph.parameters {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO named_parameters (scenario_id,name,kind,number_value,date_value,age_years,age_months)
             VALUES (?1,?2,?3,?4,?5,?6,?7) RETURNING id")
            .bind(new_id).bind(&parameter.name).bind(&parameter.kind).bind(parameter.number_value)
            .bind(&parameter.date_value).bind(parameter.age_years).bind(parameter.age_months)
            .fetch_one(&mut **tx).await?;
        parameters.insert(parameter.id, id);
    }

    // ── events, before triggers and effects that reference them ─────────────
    let mut events: Remap = HashMap::new();
    for event in &graph.events {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO events (scenario_id, name, description, fires_once, enabled, sort_order)
             VALUES (?1,?2,?3,?4,?5,?6) RETURNING id",
        )
        .bind(new_id)
        .bind(&event.name)
        .bind(&event.description)
        .bind(event.fires_once)
        .bind(event.enabled)
        .bind(event.sort_order)
        .fetch_one(&mut **tx)
        .await?;
        events.insert(event.id, id);
    }

    let mut amounts: Remap = HashMap::new();
    let mut triggers: Remap = HashMap::new();

    for event in &graph.events {
        if let Some(root) = graph.event_trigger.get(&event.id) {
            copy_trigger(
                tx,
                graph,
                new_id,
                *root,
                &accounts,
                &assets,
                &events,
                &parameters,
                &mut triggers,
            )
            .await?;
        }
        for effect_id in graph.event_effects.get(&event.id).into_iter().flatten() {
            copy_effect(
                tx,
                graph,
                new_id,
                *effect_id,
                &accounts,
                &assets,
                &events,
                &mut amounts,
            )
            .await?;
        }
    }

    Ok(new_id)
}

fn remap(map: &Remap, old: i64, what: &str) -> ApiResult<i64> {
    map.get(&old)
        .copied()
        .ok_or_else(|| ApiError::internal(format!("dangling {what} reference {old} while cloning")))
}

#[allow(clippy::too_many_arguments)]
fn copy_trigger<'a>(
    tx: &'a mut Transaction<'_, Sqlite>,
    graph: &'a ScenarioGraph,
    scenario_id: i64,
    trigger_id: i64,
    accounts: &'a Remap,
    assets: &'a Remap,
    events: &'a Remap,
    parameters: &'a Remap,
    done: &'a mut Remap,
) -> Fut<'a, i64> {
    Box::pin(async move {
        if let Some(existing) = done.get(&trigger_id) {
            return Ok(*existing);
        }

        let row = graph
            .triggers
            .get(&trigger_id)
            .ok_or_else(|| ApiError::internal(format!("missing trigger {trigger_id}")))?;

        // Sub-conditions are referenced by column, so they must exist first.
        let start_id = match row.start_trigger_id {
            Some(id) => Some(
                copy_trigger(
                    tx,
                    graph,
                    scenario_id,
                    id,
                    accounts,
                    assets,
                    events,
                    parameters,
                    done,
                )
                .await?,
            ),
            None => None,
        };
        let end_id = match row.end_trigger_id {
            Some(id) => Some(
                copy_trigger(
                    tx,
                    graph,
                    scenario_id,
                    id,
                    accounts,
                    assets,
                    events,
                    parameters,
                    done,
                )
                .await?,
            ),
            None => None,
        };
        let parent_id = match row.parent_id {
            Some(id) => Some(
                copy_trigger(
                    tx,
                    graph,
                    scenario_id,
                    id,
                    accounts,
                    assets,
                    events,
                    parameters,
                    done,
                )
                .await?,
            ),
            None => None,
        };

        let new_event_id = row
            .event_id
            .map(|id| remap(events, id, "event"))
            .transpose()?;
        let new_ref_event = row
            .ref_event_id
            .map(|id| remap(events, id, "event"))
            .transpose()?;
        let new_account = row
            .account_id
            .map(|id| remap(accounts, id, "account"))
            .transpose()?;
        let new_asset = row
            .asset_id
            .map(|id| remap(assets, id, "asset"))
            .transpose()?;

        let id: i64 = sqlx::query_scalar(
            "INSERT INTO triggers
                (scenario_id, event_id, kind, on_date, age_years, age_months, ref_event_id,
                 offset_unit, offset_value, account_id, asset_id, comparison, threshold,
                 interval, start_trigger_id, end_trigger_id, max_occurrences, parent_id, position, parameter_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)
             RETURNING id",
        )
        .bind(scenario_id)
        .bind(new_event_id)
        .bind(&row.kind)
        .bind(&row.on_date)
        .bind(row.age_years)
        .bind(row.age_months)
        .bind(new_ref_event)
        .bind(&row.offset_unit)
        .bind(row.offset_value)
        .bind(new_account)
        .bind(new_asset)
        .bind(&row.comparison)
        .bind(row.threshold)
        .bind(&row.interval)
        .bind(start_id)
        .bind(end_id)
        .bind(row.max_occurrences)
        .bind(parent_id)
        .bind(row.position)
        .bind(row.parameter_id.map(|id|remap(parameters,id,"parameter")).transpose()?)
        .fetch_one(&mut **tx)
        .await?;

        done.insert(trigger_id, id);

        for child in graph
            .trigger_children
            .get(&trigger_id)
            .into_iter()
            .flatten()
        {
            copy_trigger(
                tx,
                graph,
                scenario_id,
                *child,
                accounts,
                assets,
                events,
                parameters,
                done,
            )
            .await?;
        }

        Ok(id)
    })
}

fn copy_amount<'a>(
    tx: &'a mut Transaction<'_, Sqlite>,
    graph: &'a ScenarioGraph,
    scenario_id: i64,
    amount_id: i64,
    accounts: &'a Remap,
    assets: &'a Remap,
    done: &'a mut Remap,
) -> Fut<'a, i64> {
    Box::pin(async move {
        if let Some(existing) = done.get(&amount_id) {
            return Ok(*existing);
        }

        let row = graph
            .amounts
            .get(&amount_id)
            .ok_or_else(|| ApiError::internal(format!("missing transfer amount {amount_id}")))?;

        let left = match row.left_id {
            Some(id) => {
                Some(copy_amount(tx, graph, scenario_id, id, accounts, assets, done).await?)
            }
            None => None,
        };
        let right = match row.right_id {
            Some(id) => {
                Some(copy_amount(tx, graph, scenario_id, id, accounts, assets, done).await?)
            }
            None => None,
        };

        let id: i64 = sqlx::query_scalar(
            "INSERT INTO transfer_amounts
                (scenario_id, kind, value, account_id, asset_id, left_id, right_id, expression_source)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8) RETURNING id",
        )
        .bind(scenario_id)
        .bind(&row.kind)
        .bind(row.value)
        .bind(
            row.account_id
                .map(|id| remap(accounts, id, "account"))
                .transpose()?,
        )
        .bind(
            row.asset_id
                .map(|id| remap(assets, id, "asset"))
                .transpose()?,
        )
        .bind(left)
        .bind(right)
        .bind(&row.expression_source)
        .fetch_one(&mut **tx)
        .await?;

        done.insert(amount_id, id);
        Ok(id)
    })
}

#[allow(clippy::too_many_arguments)]
fn copy_effect<'a>(
    tx: &'a mut Transaction<'_, Sqlite>,
    graph: &'a ScenarioGraph,
    scenario_id: i64,
    effect_id: i64,
    accounts: &'a Remap,
    assets: &'a Remap,
    events: &'a Remap,
    amounts: &'a mut Remap,
) -> Fut<'a, i64> {
    Box::pin(async move {
        let row = graph
            .effects
            .get(&effect_id)
            .ok_or_else(|| ApiError::internal(format!("missing effect {effect_id}")))?;

        let amount_id = match row.amount_id {
            Some(id) => {
                Some(copy_amount(tx, graph, scenario_id, id, accounts, assets, amounts).await?)
            }
            None => None,
        };
        let down_payment_amount_id = match row.down_payment_amount_id {
            Some(id) => {
                Some(copy_amount(tx, graph, scenario_id, id, accounts, assets, amounts).await?)
            }
            None => None,
        };

        let parent_id = match row.parent_id {
            Some(id) => Some(*amounts.get(&-id).ok_or_else(|| {
                ApiError::internal("effect parent was not copied before its child")
            })?),
            None => None,
        };

        let id: i64 = sqlx::query_scalar(
            "INSERT INTO effects
                (scenario_id, event_id, parent_id, parent_slot, position, kind,
                 from_account_id, to_account_id, asset_id, amount_id, target_event_id,
                 amount_mode, income_type, lot_method, probability, units, sell_to_cover,
                 loan_account_id, down_payment_amount_id, term_months, selling_cost_rate,
                 gain_exclusion)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,
                     ?18,?19,?20,?21,?22)
             RETURNING id",
        )
        .bind(scenario_id)
        .bind(
            row.event_id
                .map(|id| remap(events, id, "event"))
                .transpose()?,
        )
        .bind(parent_id)
        .bind(&row.parent_slot)
        .bind(row.position)
        .bind(&row.kind)
        .bind(
            row.from_account_id
                .map(|id| remap(accounts, id, "account"))
                .transpose()?,
        )
        .bind(
            row.to_account_id
                .map(|id| remap(accounts, id, "account"))
                .transpose()?,
        )
        .bind(
            row.asset_id
                .map(|id| remap(assets, id, "asset"))
                .transpose()?,
        )
        .bind(amount_id)
        .bind(
            row.target_event_id
                .map(|id| remap(events, id, "event"))
                .transpose()?,
        )
        .bind(&row.amount_mode)
        .bind(&row.income_type)
        .bind(&row.lot_method)
        .bind(row.probability)
        .bind(row.units)
        .bind(row.sell_to_cover)
        .bind(
            row.loan_account_id
                .map(|id| remap(accounts, id, "account"))
                .transpose()?,
        )
        .bind(down_payment_amount_id)
        .bind(row.term_months)
        .bind(row.selling_cost_rate)
        .bind(row.gain_exclusion)
        .fetch_one(&mut **tx)
        .await?;

        // Effect ids are tracked under a negated key so they cannot collide with
        // the transfer-amount ids sharing this map.
        amounts.insert(-effect_id, id);

        if let Some(ws) = graph.withdrawal_sources.get(&effect_id) {
            sqlx::query(
                "INSERT INTO effect_withdrawal_sources
                    (effect_id, mode, account_id, asset_id, strategy)
                 VALUES (?1,?2,?3,?4,?5)",
            )
            .bind(id)
            .bind(&ws.mode)
            .bind(
                ws.account_id
                    .map(|a| remap(accounts, a, "account"))
                    .transpose()?,
            )
            .bind(ws.asset_id.map(|a| remap(assets, a, "asset")).transpose()?)
            .bind(&ws.strategy)
            .execute(&mut **tx)
            .await?;
        }

        for item in graph.withdrawal_items.get(&effect_id).into_iter().flatten() {
            sqlx::query(
                "INSERT INTO effect_withdrawal_source_items
                    (effect_id, role, position, account_id, asset_id)
                 VALUES (?1,?2,?3,?4,?5)",
            )
            .bind(id)
            .bind(&item.role)
            .bind(item.position)
            .bind(remap(accounts, item.account_id, "account")?)
            .bind(
                item.asset_id
                    .map(|a| remap(assets, a, "asset"))
                    .transpose()?,
            )
            .execute(&mut **tx)
            .await?;
        }

        for slot in ["on_true", "on_false"] {
            if let Some(child) = graph.effect_children.get(&(effect_id, slot.to_string())) {
                copy_effect(
                    tx,
                    graph,
                    scenario_id,
                    *child,
                    accounts,
                    assets,
                    events,
                    amounts,
                )
                .await?;
            }
        }

        Ok(id)
    })
}
