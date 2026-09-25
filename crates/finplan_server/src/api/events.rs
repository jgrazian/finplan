//! Event CRUD. Triggers and effects travel as nested JSON and are exploded into
//! the self-referential tables by `api::specs`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::ReorderRequest;
use super::specs::{
    AmountSpec, Comparison, EffectParent, EffectSpec, Interval, OffsetUnit, TriggerParent,
    TriggerSpec,
};
use crate::auth::activity::{ActivityFields, Submitted};
use crate::auth::session::CurrentUser;
use crate::compile::rows::ScenarioGraph;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::observability::{EventFields, Operation, Resource};
use crate::state::AppState;
use ts_rs::TS;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios/{scenario_id}/events", get(list).post(create))
        .route("/scenarios/{scenario_id}/events/reorder", post(reorder))
        .route(
            "/scenarios/{scenario_id}/events/{id}",
            get(fetch).put(replace).delete(destroy),
        )
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Event {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub fires_once: bool,
    pub enabled: bool,
    pub sort_order: i64,
    pub trigger: TriggerSpec,
    pub effects: Vec<EffectSpec>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct EventBody {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fires_once: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Omitted appends a new event to the end of the list and leaves a replaced
    /// one where it already sat — the PUT that saves an edited trigger must not
    /// silently drag the row back to the top.
    #[serde(default)]
    pub sort_order: Option<i64>,
    pub trigger: TriggerSpec,
    #[serde(default)]
    pub effects: Vec<EffectSpec>,
}

fn yes() -> bool {
    true
}

// ── reading: rows back into nested specs ────────────────────────────────────

fn read_trigger(graph: &ScenarioGraph, trigger_id: i64, depth: usize) -> ApiResult<TriggerSpec> {
    if depth > 64 {
        return Err(ApiError::internal("trigger graph is cyclic"));
    }
    let row = graph
        .triggers
        .get(&trigger_id)
        .ok_or(ApiError::NotFound("trigger"))?;

    let comparison = || match row.comparison.as_deref() {
        Some("LessThanOrEqual") => Comparison::LessThanOrEqual,
        _ => Comparison::GreaterThanOrEqual,
    };

    Ok(match row.kind.as_str() {
        "Date" if row.parameter_id.is_some() => TriggerSpec::DateParameter {
            parameter_id: row.parameter_id.unwrap(),
        },
        "Age" if row.parameter_id.is_some() => TriggerSpec::AgeParameter {
            parameter_id: row.parameter_id.unwrap(),
        },
        "Date" => TriggerSpec::Date {
            on_date: row.on_date.clone().unwrap_or_default(),
        },
        "Age" => TriggerSpec::Age {
            years: row.age_years.unwrap_or_default() as u8,
            months: row.age_months.map(|m| m as u8),
        },
        "RelativeToEvent" => TriggerSpec::RelativeToEvent {
            event_id: row.ref_event_id.unwrap_or_default(),
            unit: match row.offset_unit.as_deref() {
                Some("Days") => OffsetUnit::Days,
                Some("Years") => OffsetUnit::Years,
                _ => OffsetUnit::Months,
            },
            value: row.offset_value.unwrap_or_default() as i32,
        },
        "AccountBalance" => TriggerSpec::AccountBalance {
            account_id: row.account_id.unwrap_or_default(),
            comparison: comparison(),
            threshold: row.threshold.unwrap_or_default(),
        },
        "AssetBalance" => TriggerSpec::AssetBalance {
            account_id: row.account_id.unwrap_or_default(),
            asset_id: row.asset_id.unwrap_or_default(),
            comparison: comparison(),
            threshold: row.threshold.unwrap_or_default(),
        },
        "NetWorth" => TriggerSpec::NetWorth {
            comparison: comparison(),
            threshold: row.threshold.unwrap_or_default(),
        },
        "And" | "Or" => {
            let mut children = Vec::new();
            for child in graph
                .trigger_children
                .get(&trigger_id)
                .into_iter()
                .flatten()
            {
                children.push(read_trigger(graph, *child, depth + 1)?);
            }
            if row.kind == "And" {
                TriggerSpec::And { children }
            } else {
                TriggerSpec::Or { children }
            }
        }
        "Repeating" => TriggerSpec::Repeating {
            interval: match row.interval.as_deref() {
                Some("Never") => Interval::Never,
                Some("Weekly") => Interval::Weekly,
                Some("BiWeekly") => Interval::BiWeekly,
                Some("Quarterly") => Interval::Quarterly,
                Some("Yearly") => Interval::Yearly,
                _ => Interval::Monthly,
            },
            start_condition: row
                .start_trigger_id
                .map(|id| read_trigger(graph, id, depth + 1).map(Box::new))
                .transpose()?,
            end_condition: row
                .end_trigger_id
                .map(|id| read_trigger(graph, id, depth + 1).map(Box::new))
                .transpose()?,
            max_occurrences: row.max_occurrences.map(|m| m as u32),
        },
        _ => TriggerSpec::Manual,
    })
}

fn read_amount(graph: &ScenarioGraph, amount_id: i64, depth: usize) -> ApiResult<AmountSpec> {
    if depth > 64 {
        return Err(ApiError::internal("transfer amount graph is cyclic"));
    }
    let row = graph
        .amounts
        .get(&amount_id)
        .ok_or(ApiError::NotFound("transfer amount"))?;

    if let Some(source) = &row.expression_source {
        return Ok(AmountSpec::Expression {
            source: source.clone(),
        });
    }

    let left = |depth: usize| -> ApiResult<Box<AmountSpec>> {
        let id = row
            .left_id
            .ok_or_else(|| ApiError::internal("amount is missing its operand"))?;
        Ok(Box::new(read_amount(graph, id, depth + 1)?))
    };
    let right = |depth: usize| -> ApiResult<Box<AmountSpec>> {
        let id = row
            .right_id
            .ok_or_else(|| ApiError::internal("amount is missing its right operand"))?;
        Ok(Box::new(read_amount(graph, id, depth + 1)?))
    };

    Ok(match row.kind.as_str() {
        "Fixed" => AmountSpec::Fixed {
            value: row.value.unwrap_or_default(),
        },
        "TargetToBalance" => AmountSpec::TargetToBalance {
            value: row.value.unwrap_or_default(),
        },
        "SourceBalance" => AmountSpec::SourceBalance,
        "ZeroTargetBalance" => AmountSpec::ZeroTargetBalance,
        "InflationAdjusted" => AmountSpec::InflationAdjusted {
            inner: left(depth)?,
        },
        "Scale" => AmountSpec::Scale {
            factor: row.value.unwrap_or(1.0),
            inner: left(depth)?,
        },
        "AssetBalance" => AmountSpec::AssetBalance {
            account_id: row.account_id.unwrap_or_default(),
            asset_id: row.asset_id.unwrap_or_default(),
        },
        "AccountTotalBalance" => AmountSpec::AccountTotalBalance {
            account_id: row.account_id.unwrap_or_default(),
        },
        "AccountCashBalance" => AmountSpec::AccountCashBalance {
            account_id: row.account_id.unwrap_or_default(),
        },
        "Min" => AmountSpec::Min {
            left: left(depth)?,
            right: right(depth)?,
        },
        "Max" => AmountSpec::Max {
            left: left(depth)?,
            right: right(depth)?,
        },
        "Sub" => AmountSpec::Sub {
            left: left(depth)?,
            right: right(depth)?,
        },
        "Add" => AmountSpec::Add {
            left: left(depth)?,
            right: right(depth)?,
        },
        _ => AmountSpec::Mul {
            left: left(depth)?,
            right: right(depth)?,
        },
    })
}

fn read_effect(graph: &ScenarioGraph, effect_id: i64, depth: usize) -> ApiResult<EffectSpec> {
    use super::specs::{AmountMode, IncomeType, LotMethod};

    if depth > 64 {
        return Err(ApiError::internal("effect graph is cyclic"));
    }
    let row = graph
        .effects
        .get(&effect_id)
        .ok_or(ApiError::NotFound("effect"))?;

    let amount = || -> ApiResult<AmountSpec> {
        let id = row
            .amount_id
            .ok_or_else(|| ApiError::internal("effect is missing its amount"))?;
        read_amount(graph, id, depth)
    };
    let amount_mode = match row.amount_mode.as_deref() {
        Some("Gross") => AmountMode::Gross,
        _ => AmountMode::Net,
    };
    let income_type = match row.income_type.as_deref() {
        Some("TaxFree") => IncomeType::TaxFree,
        _ => IncomeType::Taxable,
    };
    let lot_method = match row.lot_method.as_deref() {
        Some("Lifo") => LotMethod::Lifo,
        Some("HighestCost") => LotMethod::HighestCost,
        Some("LowestCost") => LotMethod::LowestCost,
        Some("AverageCost") => LotMethod::AverageCost,
        _ => LotMethod::Fifo,
    };
    let to = row.to_account_id.unwrap_or_default();
    let from = row.from_account_id.unwrap_or_default();
    let target = row.target_event_id.unwrap_or_default();

    Ok(match row.kind.as_str() {
        "Income" => EffectSpec::Income {
            to_account_id: to,
            amount: amount()?,
            amount_mode,
            income_type,
        },
        "Expense" => EffectSpec::Expense {
            from_account_id: from,
            amount: amount()?,
        },
        "AssetPurchase" => EffectSpec::AssetPurchase {
            from_account_id: from,
            to_account_id: to,
            asset_id: row.asset_id.unwrap_or_default(),
            amount: amount()?,
        },
        "AssetSale" => EffectSpec::AssetSale {
            from_account_id: from,
            asset_id: row.asset_id,
            amount: amount()?,
            amount_mode,
            lot_method,
        },
        "Sweep" => EffectSpec::Sweep {
            to_account_id: to,
            amount: amount()?,
            sources: read_withdrawal_sources(graph, effect_id),
            amount_mode,
            lot_method,
            income_type,
        },
        "AdjustBalance" => EffectSpec::AdjustBalance {
            account_id: to,
            amount: amount()?,
        },
        "CashTransfer" => EffectSpec::CashTransfer {
            from_account_id: from,
            to_account_id: to,
            amount: amount()?,
        },
        "TriggerEvent" => EffectSpec::TriggerEvent {
            target_event_id: target,
        },
        "PauseEvent" => EffectSpec::PauseEvent {
            target_event_id: target,
        },
        "ResumeEvent" => EffectSpec::ResumeEvent {
            target_event_id: target,
        },
        "TerminateEvent" => EffectSpec::TerminateEvent {
            target_event_id: target,
        },
        "DeleteAccount" => EffectSpec::DeleteAccount { account_id: to },
        "ApplyRmd" => EffectSpec::ApplyRmd {
            to_account_id: to,
            lot_method,
        },
        "RsuVesting" => EffectSpec::RsuVesting {
            to_account_id: to,
            asset_id: row.asset_id.unwrap_or_default(),
            units: row.units.unwrap_or_default(),
            sell_to_cover: row.sell_to_cover.unwrap_or(0) != 0,
            lot_method,
        },
        _ => {
            let on_true_id = graph
                .effect_children
                .get(&(effect_id, "on_true".to_string()))
                .ok_or_else(|| ApiError::internal("Random effect has no on_true branch"))?;
            EffectSpec::Random {
                probability: row.probability.unwrap_or_default(),
                on_true: Box::new(read_effect(graph, *on_true_id, depth + 1)?),
                on_false: graph
                    .effect_children
                    .get(&(effect_id, "on_false".to_string()))
                    .map(|id| read_effect(graph, *id, depth + 1).map(Box::new))
                    .transpose()?,
            }
        }
    })
}

fn read_withdrawal_sources(
    graph: &ScenarioGraph,
    effect_id: i64,
) -> Option<super::specs::WithdrawalSourcesSpec> {
    use super::specs::{AssetRef, WithdrawalSourcesSpec, WithdrawalStrategy};

    let row = graph.withdrawal_sources.get(&effect_id)?;
    let items = graph.withdrawal_items.get(&effect_id);

    Some(match row.mode.as_str() {
        "SingleAsset" => WithdrawalSourcesSpec::SingleAsset {
            account_id: row.account_id.unwrap_or_default(),
            asset_id: row.asset_id.unwrap_or_default(),
        },
        "SingleAccount" => WithdrawalSourcesSpec::SingleAccount {
            account_id: row.account_id.unwrap_or_default(),
        },
        "Custom" => WithdrawalSourcesSpec::Custom {
            entries: items
                .into_iter()
                .flatten()
                .filter(|i| i.role == "custom")
                .map(|i| AssetRef {
                    account_id: i.account_id,
                    asset_id: i.asset_id.unwrap_or_default(),
                })
                .collect(),
        },
        _ => WithdrawalSourcesSpec::Strategy {
            strategy: match row.strategy.as_deref() {
                Some("TaxDeferredFirst") => WithdrawalStrategy::TaxDeferredFirst,
                Some("TaxFreeFirst") => WithdrawalStrategy::TaxFreeFirst,
                Some("ProRata") => WithdrawalStrategy::ProRata,
                Some("PenaltyAware") => WithdrawalStrategy::PenaltyAware,
                _ => WithdrawalStrategy::TaxEfficientEarly,
            },
            exclude_accounts: items
                .into_iter()
                .flatten()
                .filter(|i| i.role == "exclude")
                .map(|i| i.account_id)
                .collect(),
        },
    })
}

fn read_event(graph: &ScenarioGraph, event_id: i64) -> ApiResult<Event> {
    let row = graph
        .events
        .iter()
        .find(|e| e.id == event_id)
        .ok_or(ApiError::NotFound("event"))?;

    let trigger_id = graph
        .event_trigger
        .get(&event_id)
        .ok_or_else(|| ApiError::internal("event has no trigger"))?;

    let mut effects = Vec::new();
    for id in graph.event_effects.get(&event_id).into_iter().flatten() {
        effects.push(read_effect(graph, *id, 0)?);
    }

    Ok(Event {
        id: row.id,
        name: row.name.clone(),
        description: row.description.clone(),
        fires_once: row.fires_once != 0,
        enabled: row.enabled != 0,
        sort_order: row.sort_order,
        trigger: read_trigger(graph, *trigger_id, 0)?,
        effects,
    })
}

// ── handlers ────────────────────────────────────────────────────────────────

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Vec<Event>>> {
    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    let mut out = Vec::with_capacity(graph.events.len());
    for event in &graph.events {
        out.push(read_event(&graph, event.id)?);
    }
    Ok(Json(out))
}

/// Put the scenario's events in the order the body names.
async fn reorder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ReorderRequest>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    let current: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM events WHERE scenario_id = ?1 ORDER BY sort_order, id")
            .bind(scenario_id)
            .fetch_all(&state.db)
            .await?;

    let affected = super::apply_order(&state.db, "events", &current, &body.ids).await?;

    if affected > 0 {
        state.telemetry.mutation(
            Resource::Event,
            Operation::Reordered,
            &EventFields {
                user_id: Some(&user.id),
                scenario_id: Some(scenario_id),
                fields: &["ids"],
                count: Some(affected),
                ..Default::default()
            },
        );
    }
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<Json<Event>> {
    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    Ok(Json(read_event(&graph, id)?))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(Submitted { body, fields }): Json<Submitted<EventBody>>,
) -> ApiResult<(StatusCode, Json<Event>)> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let current = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    super::expressions::validate_tree(&current, &body.effects)?;

    let mut tx = state.db.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO events (scenario_id, name, description, fires_once, enabled, sort_order)
         VALUES (?1,?2,?3,?4,?5,
                 COALESCE(?6, (SELECT COALESCE(MAX(sort_order), -1) + 1
                                 FROM events WHERE scenario_id = ?1)))
         RETURNING id",
    )
    .bind(scenario_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(i64::from(body.fires_once))
    .bind(i64::from(body.enabled))
    .bind(body.sort_order)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "an event with that name already exists"))?;

    write_tree(&mut tx, scenario_id, id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Event,
        Operation::Created,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;

    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    Ok((StatusCode::CREATED, Json(read_event(&graph, id)?)))
}

/// Replace an event wholesale.
///
/// PUT rather than PATCH: a trigger or effect list is a tree, and merging a
/// partial tree into an existing one has no sensible semantics. The old tree is
/// deleted and rewritten in one transaction.
async fn replace(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
    Json(Submitted { body, fields }): Json<Submitted<EventBody>>,
) -> ApiResult<Json<Event>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let current = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    super::expressions::validate_tree(&current, &body.effects)?;

    let exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM events WHERE id = ?1 AND scenario_id = ?2")
            .bind(id)
            .bind(scenario_id)
            .fetch_optional(&state.db)
            .await?;
    exists.ok_or(ApiError::NotFound("event"))?;

    let mut tx = state.db.begin().await?;

    let affected = sqlx::query(
        "UPDATE events SET name = ?3, description = ?4, fires_once = ?5, enabled = ?6,
                           sort_order = COALESCE(?7, sort_order),
                           updated_at = datetime('now')
          WHERE id = ?1 AND scenario_id = ?2",
    )
    .bind(id)
    .bind(scenario_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(i64::from(body.fires_once))
    .bind(i64::from(body.enabled))
    .bind(body.sort_order)
    .execute(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "an event with that name already exists"))?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("event"));
    }
    // Child triggers cascade from the root; effects cascade from the event and
    // take their `Random` branches and withdrawal-source rows with them.
    sqlx::query("DELETE FROM triggers WHERE event_id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM effects WHERE event_id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    write_tree(&mut tx, scenario_id, id, &body).await?;
    collect_orphans(&mut tx, scenario_id).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Event,
        Operation::Updated,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;

    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    Ok(Json(read_event(&graph, id)?))
}

/// Delete tree nodes that nothing points at any more.
///
/// Rewriting or deleting an event drops its `effects` and root `triggers` rows,
/// but two kinds of node survive that cascade:
///
///   * `transfer_amounts`, which are referenced *by* effects rather than owned
///     by them, and
///   * the `Repeating` start/end sub-conditions, which hang off the parent's
///     own columns and so carry neither an `event_id` nor a `parent_id`.
///
/// Both are reachable only from the tree that was just removed, so collect them
/// here. Freeing a parent can orphan its children, so each sweep repeats until
/// it stops making progress.
async fn collect_orphans(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
) -> ApiResult<()> {
    loop {
        let removed = sqlx::query(
            "DELETE FROM transfer_amounts
              WHERE scenario_id = ?1
                AND NOT EXISTS (SELECT 1 FROM effects e WHERE e.amount_id = transfer_amounts.id)
                AND NOT EXISTS (SELECT 1 FROM transfer_amounts p
                                 WHERE p.left_id = transfer_amounts.id
                                    OR p.right_id = transfer_amounts.id)",
        )
        .bind(scenario_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();

        if removed == 0 {
            break;
        }
    }

    loop {
        let removed = sqlx::query(
            "DELETE FROM triggers
              WHERE scenario_id = ?1
                AND event_id IS NULL
                AND parent_id IS NULL
                AND NOT EXISTS (SELECT 1 FROM triggers p
                                 WHERE p.start_trigger_id = triggers.id
                                    OR p.end_trigger_id = triggers.id)",
        )
        .bind(scenario_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();

        if removed == 0 {
            break;
        }
    }

    Ok(())
}

async fn write_tree(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    event_id: i64,
    body: &EventBody,
) -> ApiResult<()> {
    body.trigger
        .insert(tx, scenario_id, TriggerParent::Event(event_id), 0)
        .await?;

    for (position, effect) in body.effects.iter().enumerate() {
        effect
            .insert(
                tx,
                scenario_id,
                EffectParent::Event {
                    event_id,
                    position: position as i64,
                },
                0,
            )
            .await?;
    }
    Ok(())
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    // Another event may point here via RelativeToEvent or a *Event effect;
    // those rows cascade-delete, which would silently drop a trigger condition.
    // Refuse instead and let the caller decide.
    let referrers: Vec<String> = sqlx::query_scalar(
        "SELECT e.name FROM triggers t JOIN events e ON e.id = t.event_id
          WHERE t.ref_event_id = ?1 AND t.event_id <> ?1
         UNION
         SELECT e.name FROM effects f JOIN events e ON e.id = f.event_id
          WHERE f.target_event_id = ?1 AND f.event_id <> ?1",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    if !referrers.is_empty() {
        return Err(ApiError::Conflict(format!(
            "event is referenced by: {}",
            referrers.join(", ")
        )));
    }

    let mut tx = state.db.begin().await?;
    let affected = sqlx::query("DELETE FROM events WHERE id = ?1 AND scenario_id = ?2")
        .bind(id)
        .bind(scenario_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("event"));
    }
    collect_orphans(&mut tx, scenario_id).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Event,
        Operation::Deleted,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(scenario_id),
            resource_id: Some(id),
            ..Default::default()
        },
    );
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

impl ActivityFields for EventBody {
    const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "fires_once",
        "enabled",
        "sort_order",
        "trigger",
        "effects",
    ];
}
