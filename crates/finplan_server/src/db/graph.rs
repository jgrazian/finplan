//! Load every row of one scenario into a [`ScenarioGraph`].
//!
//! Everything for one scenario is fetched in a fixed number of queries and
//! assembled in memory; the graph itself, and everything that works on it, is
//! in `finplan_plan`.

use std::collections::HashMap;

use finplan_plan::graph::{
    AccountRow, AssetRow, BankRow, DistributionRow, EffectRow, EventRow, InflationEntry,
    InvestmentRow, LiabilityRow, ParameterRow, PositionRow, PropertyRow, ReturnProfileRow,
    ScenarioGraph, ScenarioRow, TaxBracketRow, TaxConfigEntry, TaxConfigRow, TransferAmountRow,
    TriggerRow, WithdrawalItemRow, WithdrawalSourceRow,
};

use crate::db::Db;
use crate::error::{ApiError, ApiResult};

/// Load every row belonging to `scenario_id`, verifying it belongs to `user_id`.
pub async fn load(db: &Db, scenario_id: i64, user_id: &str) -> ApiResult<ScenarioGraph> {
    let mut tx = db.begin().await?;
    let graph = load_connection(&mut tx, scenario_id, user_id).await?;
    tx.commit().await?;
    Ok(graph)
}

pub async fn load_connection(
    db: &mut sqlx::SqliteConnection,
    scenario_id: i64,
    user_id: &str,
) -> ApiResult<ScenarioGraph> {
    let scenario: ScenarioRow = sqlx::query_as(
        "SELECT id, user_id, name, description, start_date, birth_date, duration_years,
                inflation_profile_id, tax_config_id, collect_ledger, created_at, updated_at
           FROM scenarios WHERE id = ?1 AND user_id = ?2",
    )
    .bind(scenario_id)
    .bind(user_id)
    .fetch_optional(&mut *db)
    .await?
    .ok_or(ApiError::NotFound("scenario"))?;

    let assets: Vec<AssetRow> = sqlx::query_as(
        "SELECT id, name, description, initial_price, return_profile_id, tracking_error, sort_order
           FROM assets WHERE scenario_id = ?1 ORDER BY sort_order, id",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let accounts: Vec<AccountRow> = sqlx::query_as(
        "SELECT id, name, description, flavor, sort_order
           FROM accounts WHERE scenario_id = ?1 ORDER BY sort_order, id",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let bank: Vec<BankRow> = sqlx::query_as(
        "SELECT b.account_id, b.cash_value, b.return_profile_id
           FROM account_bank b JOIN accounts a ON a.id = b.account_id
          WHERE a.scenario_id = ?1",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let investment: Vec<InvestmentRow> = sqlx::query_as(
        "SELECT i.account_id, i.tax_status, i.cash_value, i.cash_return_profile_id,
                i.contribution_limit, i.contribution_period, i.plan_type, i.catch_up
           FROM account_investment i JOIN accounts a ON a.id = i.account_id
          WHERE a.scenario_id = ?1",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let property: Vec<PropertyRow> = sqlx::query_as(
        "SELECT p.account_id, p.asset_id, p.value
           FROM account_property p JOIN accounts a ON a.id = p.account_id
          WHERE a.scenario_id = ?1",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let liability: Vec<LiabilityRow> = sqlx::query_as(
        "SELECT l.account_id, l.principal, l.interest_rate, l.repay_from_account_id,
                l.term_months
           FROM account_liability l JOIN accounts a ON a.id = l.account_id
          WHERE a.scenario_id = ?1",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let position_rows: Vec<PositionRow> = sqlx::query_as(
        "SELECT p.id, p.account_id, p.asset_id, p.purchase_date, p.units, p.cost_basis
           FROM positions p JOIN accounts a ON a.id = p.account_id
          WHERE a.scenario_id = ?1 ORDER BY p.sort_order, p.purchase_date, p.id",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    // Return profiles live in the user's library; pull the whole library so
    // any row the scenario references is present.
    let profile_rows: Vec<ReturnProfileRow> = sqlx::query_as(
        "SELECT id, name, description, distribution_id, asset_class
           FROM return_profiles WHERE user_id = ?1",
    )
    .bind(user_id)
    .fetch_all(&mut *db)
    .await?;

    let distribution_rows: Vec<DistributionRow> = sqlx::query_as(
        "SELECT id, kind, rate, mean, std_dev, scale, df, bull_id, bear_id,
                bull_to_bear_prob, bear_to_bull_prob, history_preset, block_size
           FROM distributions WHERE user_id = ?1",
    )
    .bind(user_id)
    .fetch_all(&mut *db)
    .await?;

    let inflation_distribution_id: Option<i64> = match scenario.inflation_profile_id {
        Some(id) => {
            sqlx::query_scalar(
                "SELECT distribution_id FROM inflation_profiles WHERE id = ?1 AND user_id = ?2",
            )
            .bind(id)
            .bind(user_id)
            .fetch_optional(&mut *db)
            .await?
        }
        None => None,
    };

    let inflation_profile_name: Option<String> = match scenario.inflation_profile_id {
        Some(id) => {
            sqlx::query_scalar("SELECT name FROM inflation_profiles WHERE id = ?1 AND user_id = ?2")
                .bind(id)
                .bind(user_id)
                .fetch_optional(&mut *db)
                .await?
        }
        None => None,
    };

    let (tax_config, tax_brackets) = match scenario.tax_config_id {
        Some(id) => {
            let cfg: Option<TaxConfigRow> = sqlx::query_as(
                "SELECT id, name, state_rate, capital_gains_rate, early_withdrawal_penalty_rate,
               standard_deduction, age_65_extra_deduction
                   FROM tax_configs WHERE id = ?1 AND user_id = ?2",
            )
            .bind(id)
            .bind(user_id)
            .fetch_optional(&mut *db)
            .await?;

            let brackets: Vec<TaxBracketRow> = sqlx::query_as(
                "SELECT threshold, rate FROM tax_brackets
                  WHERE tax_config_id = ?1 ORDER BY threshold ASC",
            )
            .bind(id)
            .fetch_all(&mut *db)
            .await?;

            (cfg, brackets)
        }
        None => (None, Vec::new()),
    };

    let mut tax_configs = HashMap::new();
    let config_rows: Vec<TaxConfigRow> = sqlx::query_as(
        "SELECT id, name, state_rate, capital_gains_rate, early_withdrawal_penalty_rate,
               standard_deduction, age_65_extra_deduction
           FROM tax_configs WHERE user_id = ?1",
    )
    .bind(user_id)
    .fetch_all(&mut *db)
    .await?;
    for config in config_rows {
        let brackets: Vec<TaxBracketRow> = sqlx::query_as(
            "SELECT threshold, rate FROM tax_brackets
              WHERE tax_config_id = ?1 ORDER BY threshold ASC",
        )
        .bind(config.id)
        .fetch_all(&mut *db)
        .await?;
        tax_configs.insert(config.id, TaxConfigEntry { config, brackets });
    }
    let inflation_profiles: HashMap<i64, InflationEntry> = sqlx::query_as::<_, (i64, String, i64)>(
        "SELECT id, name, distribution_id FROM inflation_profiles WHERE user_id = ?1",
    )
    .bind(user_id)
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(|(id, name, distribution_id)| {
        (
            id,
            InflationEntry {
                name,
                distribution_id,
            },
        )
    })
    .collect();

    let events: Vec<EventRow> = sqlx::query_as(
        "SELECT id, name, description, fires_once, enabled, sort_order
           FROM events WHERE scenario_id = ?1 ORDER BY sort_order, id",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let parameters: Vec<ParameterRow> = sqlx::query_as(
        "SELECT id, name, kind, number_value, date_value, age_years, age_months
           FROM named_parameters WHERE scenario_id = ?1 ORDER BY id",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let trigger_rows: Vec<TriggerRow> = sqlx::query_as(
        "SELECT id, event_id, kind, on_date, age_years, age_months, ref_event_id,
                offset_unit, offset_value, account_id, asset_id, comparison, threshold,
                interval, start_trigger_id, end_trigger_id, max_occurrences, parent_id, position, parameter_id
           FROM triggers WHERE scenario_id = ?1 ORDER BY parent_id, position, id",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let amount_rows: Vec<TransferAmountRow> = sqlx::query_as(
        "SELECT id, kind, value, account_id, asset_id, left_id, right_id, expression_source
           FROM transfer_amounts WHERE scenario_id = ?1",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let effect_rows: Vec<EffectRow> = sqlx::query_as(
        "SELECT id, event_id, parent_id, parent_slot, position, kind, from_account_id,
                to_account_id, asset_id, amount_id, target_event_id, amount_mode,
                income_type, lot_method, probability, units, sell_to_cover,
                loan_account_id, down_payment_amount_id, term_months, selling_cost_rate,
                gain_exclusion, shock_drop
           FROM effects WHERE scenario_id = ?1 ORDER BY event_id, position, id",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let ws_rows: Vec<WithdrawalSourceRow> = sqlx::query_as(
        "SELECT w.effect_id, w.mode, w.account_id, w.asset_id, w.strategy, w.bracket_ceiling
           FROM effect_withdrawal_sources w JOIN effects e ON e.id = w.effect_id
          WHERE e.scenario_id = ?1",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    let wi_rows: Vec<WithdrawalItemRow> = sqlx::query_as(
        "SELECT i.effect_id, i.role, i.position, i.account_id, i.asset_id
           FROM effect_withdrawal_source_items i JOIN effects e ON e.id = i.effect_id
          WHERE e.scenario_id = ?1 ORDER BY i.position, i.id",
    )
    .bind(scenario_id)
    .fetch_all(&mut *db)
    .await?;

    // ── index everything ────────────────────────────────────────────────
    let mut positions: HashMap<i64, Vec<PositionRow>> = HashMap::new();
    for row in position_rows {
        positions.entry(row.account_id).or_default().push(row);
    }

    let mut withdrawal_items: HashMap<i64, Vec<WithdrawalItemRow>> = HashMap::new();
    for row in wi_rows {
        withdrawal_items.entry(row.effect_id).or_default().push(row);
    }

    let mut graph = ScenarioGraph {
        scenario,
        assets,
        accounts,
        bank: bank.into_iter().map(|r| (r.account_id, r)).collect(),
        investment: investment.into_iter().map(|r| (r.account_id, r)).collect(),
        property: property.into_iter().map(|r| (r.account_id, r)).collect(),
        liability: liability.into_iter().map(|r| (r.account_id, r)).collect(),
        positions,
        return_profiles: profile_rows.into_iter().map(|r| (r.id, r)).collect(),
        distributions: distribution_rows.into_iter().map(|r| (r.id, r)).collect(),
        inflation_distribution_id,
        inflation_profile_name,
        tax_config,
        tax_brackets,
        tax_configs,
        inflation_profiles,
        events,
        parameters,
        triggers: trigger_rows.into_iter().map(|r| (r.id, r)).collect(),
        trigger_children: HashMap::new(),
        event_trigger: HashMap::new(),
        amounts: amount_rows.into_iter().map(|r| (r.id, r)).collect(),
        effects: effect_rows.into_iter().map(|r| (r.id, r)).collect(),
        event_effects: HashMap::new(),
        effect_children: HashMap::new(),
        withdrawal_sources: ws_rows.into_iter().map(|r| (r.effect_id, r)).collect(),
        withdrawal_items,
    };
    graph.reindex();
    Ok(graph)
}
