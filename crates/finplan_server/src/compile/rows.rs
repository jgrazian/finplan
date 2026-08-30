//! Row structs mirroring the schema, plus a bulk loader.
//!
//! Everything for one scenario is fetched in a fixed number of queries and
//! assembled in memory. The recursive structures (triggers, transfer amounts,
//! effects) are stored flat with parent links, so recursing over rows in memory
//! is far cheaper than recursing over `await` points.

use std::collections::HashMap;

use sqlx::FromRow;

use crate::db::Db;
use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, FromRow)]
pub struct ScenarioRow {
    pub id: i64,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub start_date: String,
    pub birth_date: Option<String>,
    pub duration_years: i64,
    pub inflation_profile_id: Option<i64>,
    pub tax_config_id: Option<i64>,
    pub collect_ledger: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct DistributionRow {
    pub id: i64,
    pub kind: String,
    pub rate: Option<f64>,
    pub mean: Option<f64>,
    pub std_dev: Option<f64>,
    pub scale: Option<f64>,
    pub df: Option<f64>,
    pub bull_id: Option<i64>,
    pub bear_id: Option<i64>,
    pub bull_to_bear_prob: Option<f64>,
    pub bear_to_bull_prob: Option<f64>,
    pub history_preset: Option<String>,
    pub block_size: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ReturnProfileRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub distribution_id: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct AssetRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub initial_price: f64,
    pub return_profile_id: i64,
    pub tracking_error: Option<f64>,
    pub sort_order: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct AccountRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub flavor: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct BankRow {
    pub account_id: i64,
    pub cash_value: f64,
    pub return_profile_id: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct InvestmentRow {
    pub account_id: i64,
    pub tax_status: String,
    pub cash_value: f64,
    pub cash_return_profile_id: i64,
    pub contribution_limit: Option<f64>,
    pub contribution_period: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct PropertyRow {
    pub account_id: i64,
    pub asset_id: i64,
    pub value: f64,
}

#[derive(Debug, Clone, FromRow)]
pub struct LiabilityRow {
    pub account_id: i64,
    pub principal: f64,
    pub interest_rate: f64,
}

#[derive(Debug, Clone, FromRow)]
pub struct PositionRow {
    pub id: i64,
    pub account_id: i64,
    pub asset_id: i64,
    pub purchase_date: String,
    pub units: f64,
    pub cost_basis: f64,
}

#[derive(Debug, Clone, FromRow)]
pub struct TransferAmountRow {
    pub id: i64,
    pub kind: String,
    pub value: Option<f64>,
    pub account_id: Option<i64>,
    pub asset_id: Option<i64>,
    pub left_id: Option<i64>,
    pub right_id: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub struct EventRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub fires_once: i64,
    pub enabled: i64,
    pub sort_order: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct TriggerRow {
    pub id: i64,
    pub event_id: Option<i64>,
    pub kind: String,
    pub on_date: Option<String>,
    pub age_years: Option<i64>,
    pub age_months: Option<i64>,
    pub ref_event_id: Option<i64>,
    pub offset_unit: Option<String>,
    pub offset_value: Option<i64>,
    pub account_id: Option<i64>,
    pub asset_id: Option<i64>,
    pub comparison: Option<String>,
    pub threshold: Option<f64>,
    pub interval: Option<String>,
    pub start_trigger_id: Option<i64>,
    pub end_trigger_id: Option<i64>,
    pub max_occurrences: Option<i64>,
    pub parent_id: Option<i64>,
    pub position: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct EffectRow {
    pub id: i64,
    pub event_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub parent_slot: Option<String>,
    pub position: i64,
    pub kind: String,
    pub from_account_id: Option<i64>,
    pub to_account_id: Option<i64>,
    pub asset_id: Option<i64>,
    pub amount_id: Option<i64>,
    pub target_event_id: Option<i64>,
    pub amount_mode: Option<String>,
    pub income_type: Option<String>,
    pub lot_method: Option<String>,
    pub probability: Option<f64>,
    pub units: Option<f64>,
    pub sell_to_cover: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub struct WithdrawalSourceRow {
    pub effect_id: i64,
    pub mode: String,
    pub account_id: Option<i64>,
    pub asset_id: Option<i64>,
    pub strategy: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct WithdrawalItemRow {
    pub effect_id: i64,
    pub role: String,
    pub position: i64,
    pub account_id: i64,
    pub asset_id: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub struct TaxConfigRow {
    pub id: i64,
    pub name: String,
    pub state_rate: f64,
    pub capital_gains_rate: f64,
    pub early_withdrawal_penalty_rate: f64,
}

#[derive(Debug, Clone, FromRow)]
pub struct TaxBracketRow {
    pub threshold: f64,
    pub rate: f64,
}

/// Every row backing one scenario, indexed for in-memory tree assembly.
#[derive(Debug, Clone)]
pub struct ScenarioGraph {
    pub scenario: ScenarioRow,

    pub assets: Vec<AssetRow>,
    pub accounts: Vec<AccountRow>,
    pub bank: HashMap<i64, BankRow>,
    pub investment: HashMap<i64, InvestmentRow>,
    pub property: HashMap<i64, PropertyRow>,
    pub liability: HashMap<i64, LiabilityRow>,
    /// account id -> its lots
    pub positions: HashMap<i64, Vec<PositionRow>>,

    pub return_profiles: HashMap<i64, ReturnProfileRow>,
    pub distributions: HashMap<i64, DistributionRow>,
    pub inflation_distribution_id: Option<i64>,

    pub tax_config: Option<TaxConfigRow>,
    pub tax_brackets: Vec<TaxBracketRow>,

    pub events: Vec<EventRow>,
    pub triggers: HashMap<i64, TriggerRow>,
    /// parent trigger id -> ordered child ids (And/Or members)
    pub trigger_children: HashMap<i64, Vec<i64>>,
    /// event id -> its root trigger id
    pub event_trigger: HashMap<i64, i64>,

    pub amounts: HashMap<i64, TransferAmountRow>,

    pub effects: HashMap<i64, EffectRow>,
    /// event id -> ordered top-level effect ids
    pub event_effects: HashMap<i64, Vec<i64>>,
    /// (parent effect id, slot) -> child effect id
    pub effect_children: HashMap<(i64, String), i64>,
    pub withdrawal_sources: HashMap<i64, WithdrawalSourceRow>,
    pub withdrawal_items: HashMap<i64, Vec<WithdrawalItemRow>>,
}

impl ScenarioGraph {
    /// Load every row belonging to `scenario_id`, verifying it belongs to `user_id`.
    pub async fn load(db: &Db, scenario_id: i64, user_id: &str) -> ApiResult<Self> {
        let scenario: ScenarioRow = sqlx::query_as(
            "SELECT id, user_id, name, description, start_date, birth_date, duration_years,
                    inflation_profile_id, tax_config_id, collect_ledger, created_at, updated_at
               FROM scenarios WHERE id = ?1 AND user_id = ?2",
        )
        .bind(scenario_id)
        .bind(user_id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::NotFound("scenario"))?;

        let assets: Vec<AssetRow> = sqlx::query_as(
            "SELECT id, name, description, initial_price, return_profile_id, tracking_error, sort_order
               FROM assets WHERE scenario_id = ?1 ORDER BY sort_order, id",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let accounts: Vec<AccountRow> = sqlx::query_as(
            "SELECT id, name, description, flavor, sort_order
               FROM accounts WHERE scenario_id = ?1 ORDER BY sort_order, id",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let bank: Vec<BankRow> = sqlx::query_as(
            "SELECT b.account_id, b.cash_value, b.return_profile_id
               FROM account_bank b JOIN accounts a ON a.id = b.account_id
              WHERE a.scenario_id = ?1",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let investment: Vec<InvestmentRow> = sqlx::query_as(
            "SELECT i.account_id, i.tax_status, i.cash_value, i.cash_return_profile_id,
                    i.contribution_limit, i.contribution_period
               FROM account_investment i JOIN accounts a ON a.id = i.account_id
              WHERE a.scenario_id = ?1",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let property: Vec<PropertyRow> = sqlx::query_as(
            "SELECT p.account_id, p.asset_id, p.value
               FROM account_property p JOIN accounts a ON a.id = p.account_id
              WHERE a.scenario_id = ?1",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let liability: Vec<LiabilityRow> = sqlx::query_as(
            "SELECT l.account_id, l.principal, l.interest_rate
               FROM account_liability l JOIN accounts a ON a.id = l.account_id
              WHERE a.scenario_id = ?1",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let position_rows: Vec<PositionRow> = sqlx::query_as(
            "SELECT p.id, p.account_id, p.asset_id, p.purchase_date, p.units, p.cost_basis
               FROM positions p JOIN accounts a ON a.id = p.account_id
              WHERE a.scenario_id = ?1 ORDER BY p.purchase_date, p.id",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        // Return profiles live in the user's library; pull the whole library so
        // any row the scenario references is present.
        let profile_rows: Vec<ReturnProfileRow> = sqlx::query_as(
            "SELECT id, name, description, distribution_id
               FROM return_profiles WHERE user_id = ?1",
        )
        .bind(user_id)
        .fetch_all(db)
        .await?;

        let distribution_rows: Vec<DistributionRow> = sqlx::query_as(
            "SELECT id, kind, rate, mean, std_dev, scale, df, bull_id, bear_id,
                    bull_to_bear_prob, bear_to_bull_prob, history_preset, block_size
               FROM distributions WHERE user_id = ?1",
        )
        .bind(user_id)
        .fetch_all(db)
        .await?;

        let inflation_distribution_id: Option<i64> =
            match scenario.inflation_profile_id {
                Some(id) => sqlx::query_scalar(
                    "SELECT distribution_id FROM inflation_profiles WHERE id = ?1 AND user_id = ?2",
                )
                .bind(id)
                .bind(user_id)
                .fetch_optional(db)
                .await?,
                None => None,
            };

        let (tax_config, tax_brackets) = match scenario.tax_config_id {
            Some(id) => {
                let cfg: Option<TaxConfigRow> = sqlx::query_as(
                    "SELECT id, name, state_rate, capital_gains_rate, early_withdrawal_penalty_rate
                       FROM tax_configs WHERE id = ?1 AND user_id = ?2",
                )
                .bind(id)
                .bind(user_id)
                .fetch_optional(db)
                .await?;

                let brackets: Vec<TaxBracketRow> = sqlx::query_as(
                    "SELECT threshold, rate FROM tax_brackets
                      WHERE tax_config_id = ?1 ORDER BY threshold ASC",
                )
                .bind(id)
                .fetch_all(db)
                .await?;

                (cfg, brackets)
            }
            None => (None, Vec::new()),
        };

        let events: Vec<EventRow> = sqlx::query_as(
            "SELECT id, name, description, fires_once, enabled, sort_order
               FROM events WHERE scenario_id = ?1 ORDER BY sort_order, id",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let trigger_rows: Vec<TriggerRow> = sqlx::query_as(
            "SELECT id, event_id, kind, on_date, age_years, age_months, ref_event_id,
                    offset_unit, offset_value, account_id, asset_id, comparison, threshold,
                    interval, start_trigger_id, end_trigger_id, max_occurrences, parent_id, position
               FROM triggers WHERE scenario_id = ?1 ORDER BY parent_id, position, id",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let amount_rows: Vec<TransferAmountRow> = sqlx::query_as(
            "SELECT id, kind, value, account_id, asset_id, left_id, right_id
               FROM transfer_amounts WHERE scenario_id = ?1",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let effect_rows: Vec<EffectRow> = sqlx::query_as(
            "SELECT id, event_id, parent_id, parent_slot, position, kind, from_account_id,
                    to_account_id, asset_id, amount_id, target_event_id, amount_mode,
                    income_type, lot_method, probability, units, sell_to_cover
               FROM effects WHERE scenario_id = ?1 ORDER BY event_id, position, id",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let ws_rows: Vec<WithdrawalSourceRow> = sqlx::query_as(
            "SELECT w.effect_id, w.mode, w.account_id, w.asset_id, w.strategy
               FROM effect_withdrawal_sources w JOIN effects e ON e.id = w.effect_id
              WHERE e.scenario_id = ?1",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        let wi_rows: Vec<WithdrawalItemRow> = sqlx::query_as(
            "SELECT i.effect_id, i.role, i.position, i.account_id, i.asset_id
               FROM effect_withdrawal_source_items i JOIN effects e ON e.id = i.effect_id
              WHERE e.scenario_id = ?1 ORDER BY i.position, i.id",
        )
        .bind(scenario_id)
        .fetch_all(db)
        .await?;

        // ── index everything ────────────────────────────────────────────────
        let mut positions: HashMap<i64, Vec<PositionRow>> = HashMap::new();
        for row in position_rows {
            positions.entry(row.account_id).or_default().push(row);
        }

        let mut triggers = HashMap::new();
        let mut trigger_children: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut event_trigger = HashMap::new();
        for row in trigger_rows {
            if let Some(parent) = row.parent_id {
                trigger_children.entry(parent).or_default().push(row.id);
            }
            if let Some(event_id) = row.event_id {
                event_trigger.insert(event_id, row.id);
            }
            triggers.insert(row.id, row);
        }

        let mut effects = HashMap::new();
        let mut event_effects: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut effect_children = HashMap::new();
        for row in effect_rows {
            if let Some(event_id) = row.event_id {
                event_effects.entry(event_id).or_default().push(row.id);
            }
            if let (Some(parent), Some(slot)) = (row.parent_id, row.parent_slot.clone()) {
                effect_children.insert((parent, slot), row.id);
            }
            effects.insert(row.id, row);
        }

        let mut withdrawal_items: HashMap<i64, Vec<WithdrawalItemRow>> = HashMap::new();
        for row in wi_rows {
            withdrawal_items.entry(row.effect_id).or_default().push(row);
        }

        Ok(ScenarioGraph {
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
            tax_config,
            tax_brackets,
            events,
            triggers,
            trigger_children,
            event_trigger,
            amounts: amount_rows.into_iter().map(|r| (r.id, r)).collect(),
            effects,
            event_effects,
            effect_children,
            withdrawal_sources: ws_rows.into_iter().map(|r| (r.effect_id, r)).collect(),
            withdrawal_items,
        })
    }
}
