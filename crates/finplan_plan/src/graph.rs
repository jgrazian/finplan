//! The rows backing one scenario, indexed in memory.
//!
//! The row structs mirror the database schema, and [`ScenarioGraph`] holds
//! every row of one scenario. The recursive structures (triggers, transfer
//! amounts, effects) are stored flat with parent links, so recursing over rows
//! in memory is far cheaper than recursing over `await` points.
//!
//! Nothing here touches a database: with the `sqlx` feature the rows derive
//! `FromRow` so a server can read them, and `finplan_server::db::graph` is the
//! loader that does.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::specs::CatchUpSpec;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
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
    /// The plan-level funding policy's strategy (`WithdrawalStrategy` name);
    /// null = off. Skipped when unset so plans without it keep their input hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "sqlx", sqlx(default))]
    pub funding_strategy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "sqlx", sqlx(default))]
    pub funding_bracket_ceiling: Option<f64>,
    /// The tax a tax-deferred balance is assumed to owe, for valuing the plan
    /// after tax (spec 21). Left out of the JSON at its default, so plans that
    /// never set it keep their input hash and older snapshots read as 24%.
    #[serde(
        default = "default_deferred_tax_rate",
        skip_serializing_if = "is_default_deferred_tax_rate"
    )]
    pub deferred_tax_rate: f64,
    pub created_at: String,
    pub updated_at: String,
}

fn default_deferred_tax_rate() -> f64 {
    finplan_core::config::DEFAULT_DEFERRED_TAX_RATE
}

fn is_default_deferred_tax_rate(rate: &f64) -> bool {
    *rate == finplan_core::config::DEFAULT_DEFERRED_TAX_RATE
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
#[ts(export)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct ReturnProfileRow {
    #[serde(default)]
    pub asset_class: Option<String>,
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub distribution_id: i64,
    /// Where the profile sits in the caller's library (`ORDER BY sort_order,
    /// name`). Not part of what a run depends on, so a zero is left out of the
    /// JSON and [`crate::snapshot::snapshot`] zeroes it: the input hash of a
    /// plan does not move when someone reorders their library.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[cfg_attr(feature = "sqlx", sqlx(default))]
    pub sort_order: i64,
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct AssetRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub initial_price: f64,
    /// Null where the asset is unmapped; the compiler gives it flat zero growth.
    pub return_profile_id: Option<i64>,
    pub tracking_error: Option<f64>,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct AccountRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub flavor: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct BankRow {
    pub account_id: i64,
    pub cash_value: f64,
    pub return_profile_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct InvestmentRow {
    pub account_id: i64,
    pub tax_status: String,
    pub cash_value: f64,
    pub cash_return_profile_id: i64,
    pub contribution_limit: Option<f64>,
    pub contribution_period: Option<String>,
    /// Defaulted so a plan archived before plan types existed still imports.
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "sqlx", sqlx(json))]
    pub catch_up: Vec<CatchUpSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct PropertyRow {
    pub account_id: i64,
    pub asset_id: i64,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct LiabilityRow {
    pub account_id: i64,
    pub principal: f64,
    pub interest_rate: f64,
    #[serde(default)]
    pub repay_from_account_id: Option<i64>,
    #[serde(default)]
    pub term_months: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct PositionRow {
    pub id: i64,
    pub account_id: i64,
    pub asset_id: i64,
    pub purchase_date: String,
    pub units: f64,
    pub cost_basis: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct TransferAmountRow {
    pub id: i64,
    pub kind: String,
    pub value: Option<f64>,
    pub account_id: Option<i64>,
    pub asset_id: Option<i64>,
    pub left_id: Option<i64>,
    pub right_id: Option<i64>,
    pub expression_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct ParameterRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub number_value: Option<f64>,
    pub date_value: Option<String>,
    pub age_years: Option<i64>,
    pub age_months: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct EventRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub fires_once: i64,
    pub enabled: i64,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
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
    pub parameter_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
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
    #[serde(default)]
    pub loan_account_id: Option<i64>,
    #[serde(default)]
    pub down_payment_amount_id: Option<i64>,
    #[serde(default)]
    pub term_months: Option<i64>,
    #[serde(default)]
    pub selling_cost_rate: Option<f64>,
    #[serde(default)]
    pub gain_exclusion: Option<f64>,
    #[serde(default)]
    pub shock_drop: Option<f64>,
    /// RothConversion: the account paying the tax; NULL withholds it.
    #[serde(default)]
    pub pay_tax_from_account_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct WithdrawalSourceRow {
    pub effect_id: i64,
    pub mode: String,
    pub account_id: Option<i64>,
    pub asset_id: Option<i64>,
    pub strategy: Option<String>,
    /// Skipped when unset so plans without it keep their input hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bracket_ceiling: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct WithdrawalItemRow {
    pub effect_id: i64,
    pub role: String,
    pub position: i64,
    pub account_id: i64,
    pub asset_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct TaxConfigRow {
    pub id: i64,
    pub name: String,
    pub state_rate: f64,
    pub capital_gains_rate: f64,
    pub early_withdrawal_penalty_rate: f64,
    /// Absent from archives written before the column existed.
    #[serde(default)]
    pub standard_deduction: f64,
    #[serde(default)]
    pub age_65_extra_deduction: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct TaxBracketRow {
    pub threshold: f64,
    pub rate: f64,
}

/// A tax config of the caller's library with its bracket table.
#[derive(Debug, Clone)]
pub struct TaxConfigEntry {
    pub config: TaxConfigRow,
    /// The library's description of it, which a run does not read (so it is
    /// not on [`TaxConfigRow`], whose JSON is part of the input snapshot).
    pub description: Option<String>,
    pub brackets: Vec<TaxBracketRow>,
}

/// An inflation profile of the caller's library: its name and the
/// distribution (in [`ScenarioGraph::distributions`]) that drives it.
#[derive(Debug, Clone)]
pub struct InflationEntry {
    pub name: String,
    pub distribution_id: i64,
    pub description: Option<String>,
    /// Its place in the caller's library (`ORDER BY sort_order, name`).
    pub sort_order: i64,
}

/// Every row backing one scenario, indexed for in-memory tree assembly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioGraph {
    pub scenario: ScenarioRow,
    /// Investment accounts the funding policy never sells.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub funding_excludes: Vec<i64>,

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
    #[serde(default)]
    pub inflation_profile_name: Option<String>,
    pub inflation_distribution_id: Option<i64>,

    pub tax_config: Option<TaxConfigRow>,
    pub tax_brackets: Vec<TaxBracketRow>,
    /// The caller's whole tax config and inflation profile libraries, so an
    /// in-memory edit can switch the scenario onto another (`edit`).
    /// Filled by a live load only: they are not part of a run's input
    /// snapshot, which keeps just the ones the run used, so whoever edits a
    /// snapshot loads the ones its changes name first.
    #[serde(skip)]
    pub tax_configs: HashMap<i64, TaxConfigEntry>,
    #[serde(skip)]
    pub inflation_profiles: HashMap<i64, InflationEntry>,

    pub events: Vec<EventRow>,
    #[serde(default)]
    pub parameters: Vec<ParameterRow>,
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
    #[serde(with = "effect_child_pairs")]
    pub effect_children: HashMap<(i64, String), i64>,
    pub withdrawal_sources: HashMap<i64, WithdrawalSourceRow>,
    pub withdrawal_items: HashMap<i64, Vec<WithdrawalItemRow>>,
}

/// A table of the graph whose rows are keyed by an `i64` id, for
/// [`ScenarioGraph::next_id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Accounts,
    Assets,
    Events,
    Parameters,
    Positions,
    Triggers,
    Amounts,
    Effects,
    /// The caller's return profile library.
    ReturnProfiles,
    /// The caller's distribution library.
    Distributions,
    /// The caller's tax config library.
    TaxConfigs,
}

/// Ids for library rows (profiles, distributions, tax configs) that exist only
/// in memory start above every real id a run snapshot, which keeps just the
/// library rows a run used, could be missing.
const IN_MEMORY_LIBRARY_ID_FLOOR: i64 = 1_000_000_000;

impl ScenarioGraph {
    /// The funding policy as the API shows it; None when the policy is off.
    pub fn funding(&self) -> Option<crate::specs::scenarios::FundingPolicySpec> {
        let strategy =
            crate::specs::WithdrawalStrategy::parse(self.scenario.funding_strategy.as_deref()?)?;
        let mut exclude_accounts = self.funding_excludes.clone();
        exclude_accounts.sort_unstable();
        Some(crate::specs::scenarios::FundingPolicySpec {
            strategy,
            bracket_ceiling: self.scenario.funding_bracket_ceiling,
            exclude_accounts,
        })
    }

    /// The id for a new row of `table`: one above the largest id it holds
    /// (above [`IN_MEMORY_LIBRARY_ID_FLOOR`] for the library tables, whose
    /// rows a snapshot may leave out). Ids are only ever the graph's own, so
    /// the server's and an offline plan's in-memory rows are numbered alike.
    pub fn next_id(&self, table: Table) -> i64 {
        let max = |ids: &mut dyn Iterator<Item = i64>| ids.max().unwrap_or(0);
        let library = |max: i64| max.max(IN_MEMORY_LIBRARY_ID_FLOOR) + 1;
        match table {
            Table::Accounts => max(&mut self.accounts.iter().map(|r| r.id)) + 1,
            Table::Assets => max(&mut self.assets.iter().map(|r| r.id)) + 1,
            Table::Events => max(&mut self.events.iter().map(|r| r.id)) + 1,
            Table::Parameters => max(&mut self.parameters.iter().map(|r| r.id)) + 1,
            Table::Positions => max(&mut self.positions.values().flatten().map(|r| r.id)) + 1,
            Table::Triggers => max(&mut self.triggers.keys().copied()) + 1,
            Table::Amounts => max(&mut self.amounts.keys().copied()) + 1,
            Table::Effects => max(&mut self.effects.keys().copied()) + 1,
            Table::ReturnProfiles => library(max(&mut self.return_profiles.keys().copied())),
            Table::Distributions => library(max(&mut self.distributions.keys().copied())),
            Table::TaxConfigs => library(max(&mut self.tax_configs.keys().copied())),
        }
    }

    /// Rebuild the lookup indexes from the trigger and effect rows.
    ///
    /// The indexes are derived data: after rows are added or removed in memory,
    /// this puts them back exactly as a fresh load would —
    /// children and top-level effects in `(position, id)` order, withdrawal
    /// items by position.
    pub fn reindex(&mut self) {
        let mut triggers: Vec<&TriggerRow> = self.triggers.values().collect();
        triggers.sort_by_key(|r| (r.parent_id, r.position, r.id));
        self.trigger_children.clear();
        self.event_trigger.clear();
        for row in triggers {
            if let Some(parent) = row.parent_id {
                self.trigger_children
                    .entry(parent)
                    .or_default()
                    .push(row.id);
            }
            if let Some(event_id) = row.event_id {
                self.event_trigger.insert(event_id, row.id);
            }
        }

        let mut effects: Vec<&EffectRow> = self.effects.values().collect();
        effects.sort_by_key(|r| (r.event_id, r.position, r.id));
        self.event_effects.clear();
        self.effect_children.clear();
        for row in effects {
            if let Some(event_id) = row.event_id {
                self.event_effects.entry(event_id).or_default().push(row.id);
            }
            if let (Some(parent), Some(slot)) = (row.parent_id, row.parent_slot.clone()) {
                self.effect_children.insert((parent, slot), row.id);
            }
        }

        self.withdrawal_items.retain(|_, items| !items.is_empty());
        for items in self.withdrawal_items.values_mut() {
            items.sort_by_key(|r| r.position);
        }
    }
}

mod effect_child_pairs {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &HashMap<(i64, String), i64>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut pairs: Vec<_> = value.iter().collect();
        pairs.sort_by_key(|(key, _)| *key);
        pairs.serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<HashMap<(i64, String), i64>, D::Error> {
        Ok(Vec::<((i64, String), i64)>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}
