//! The shapes a run's results are served in: the `GET /runs/{id}/results` and
//! `/ledger` bodies.
//!
//! They live here, rather than with the HTTP handlers, so the server's SQL read
//! path and [`super::RunResults`] build the very same values.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::funding::FundingDiagnostics;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Stats {
    pub num_iterations: i64,
    /// Fraction of paths with positive terminal net worth, not funding success.
    pub success_rate: f64,
    /// No settled cash shortfalls or event warnings. Null for historical runs
    /// that did not measure this; rerun instead of inferring it from snapshots.
    pub funding_success_rate: Option<f64>,
    pub mean_final_net_worth: f64,
    pub std_dev_final_net_worth: f64,
    pub min_final_net_worth: f64,
    pub max_final_net_worth: f64,
    pub lifetime_taxes: f64,
    pub converged: Option<bool>,
    pub convergence_metric: Option<String>,
    pub convergence_value: Option<f64>,
    pub percentile_values: Vec<PercentileValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PercentileValue {
    pub percentile: f64,
    pub final_net_worth: f64,
}

/// A representative path ranked by terminal NOMINAL net worth, not a
/// pointwise quantile. Null percentile is the synthetic nominal mean, which
/// has no coherent ledger and must not be deflated using mean inflation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Band {
    /// Run-local identity, also accepted by results/ledger `series` queries.
    pub path_id: String,
    pub percentile: Option<f64>,
    /// The seed that replays this path, as decimal text (a `u64`). Null for the
    /// mean and for a run stored before seeds were kept.
    #[serde(default)]
    pub seed: Option<String>,
    pub dates: Vec<String>,
    pub net_worth: Vec<f64>,
    /// Cumulative inflation at each of `dates`, on this path's own realised
    /// inflation: `real = net_worth[i] / inflation[i]`. All ones for a run
    /// stored before inflation was recorded.
    pub inflation: Vec<f64>,
}

/// Cumulative inflation for one plan year. Factor 1.0 is the plan's base
/// year; the engine uses annual factors without within-year interpolation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InflationPoint {
    pub year: i64,
    pub factor: f64,
}

/// What one year of the ledger holds, without the entries themselves — enough
/// for the cash-flow table to say how much is behind each row before anyone
/// expands it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LedgerYear {
    pub year: i64,
    /// Entry counts per filter bucket, and in total.
    pub cash: i64,
    pub asset: i64,
    pub tax: i64,
    pub event: i64,
    pub total: i64,
    /// The names of the events that started this year — retiring, a pension
    /// starting — in the order they fired; empty for a year that only did the
    /// ordinary things.
    pub tags: Vec<String>,
}

/// One flattened ledger entry.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
#[ts(export)]
pub struct LedgerEntry {
    pub position: i64,
    pub date: String,
    pub year: i64,
    /// `cash` | `asset` | `tax` | `event`.
    pub category: String,
    pub kind: String,
    /// Prose naming the accounts and events involved. Deliberately free of
    /// dollar figures, so the client can restate `amount` and `basis` in real
    /// dollars without rewriting it.
    pub detail: String,
    /// Signed against the plan: money in is positive, money out negative.
    pub amount: Option<f64>,
    pub basis: Option<f64>,
    pub basis_label: Option<String>,
    pub account_id: Option<i64>,
    pub event_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AccountSeries {
    pub account_id: i64,
    pub label: String,
    pub values: Vec<f64>,
    /// An investment account's uninvested cash at each point, part of
    /// `values`. Absent for other accounts, and for runs stored before it
    /// was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cash: Option<Vec<f64>>,
}

/// A series' cash, when every point has one.
pub fn cash_series(points: Vec<Option<f64>>) -> Option<Vec<f64>> {
    if points.is_empty() {
        return None;
    }
    points.into_iter().collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CashFlow {
    pub year: i64,
    pub income: f64,
    pub expenses: f64,
    pub contributions: f64,
    pub withdrawals: f64,
    pub appreciation: f64,
    pub net_cash_flow: f64,
    /// Income tax and early-withdrawal penalties.
    pub taxes: f64,
    /// The ordinary income the year's income tax was figured on: taxable
    /// income, tax-deferred withdrawals and Roth conversions, gross.
    #[serde(default)]
    pub ordinary_income: f64,
    /// Early-withdrawal penalties paid this year, part of `taxes`.
    #[serde(default)]
    pub early_withdrawal_penalties: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
#[ts(export)]
pub struct Warning {
    pub kind: String,
    pub date: Option<String>,
    pub event_id: Option<i64>,
    /// The account the warning is about, when the engine knew it.
    pub account_id: Option<i64>,
    pub message: String,
}

/// Real-dollar pointwise quantiles over ALL iterations, not selected paths.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
#[ts(export)]
pub struct RealQuantilePoint {
    pub date: String,
    pub p5: f64,
    /// P10, P25, P75 and P90: the Results fan's two bands. Null on runs
    /// stored before they were measured, which only have P5–P95.
    pub p10: Option<f64>,
    pub p25: Option<f64>,
    pub p50: f64,
    pub p75: Option<f64>,
    pub p90: Option<f64>,
    pub p95: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
#[ts(export)]
pub struct RealTerminalStats {
    pub base_date: String,
    pub num_iterations: i64,
    pub mean: f64,
    /// Population standard deviation.
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RealNetWorthSummary {
    /// Each iteration is deflated before aggregation. Annual factors relative
    /// to base_date; no within-year interpolation. Includes warning paths;
    /// hard simulation errors or invalid numbers fail the entire run.
    pub terminal: RealTerminalStats,
    /// Plan start, Dec 31 checkpoints, terminal date; duplicate dates collapsed.
    /// Exact type-7 quantiles: linear interpolation at (N - 1) * p.
    /// This envelope has no path ID, account decomposition or ledger.
    pub points: Vec<RealQuantilePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Results {
    pub run_id: i64,
    pub scenario_id: i64,
    pub stats: Stats,
    pub bands: Vec<Band>,
    /// Null for historical runs; never inferred from stored representative paths.
    pub real_net_worth: Option<RealNetWorthSummary>,
    /// False once a newer run in the scenario has succeeded. Only the newest
    /// successful run keeps its paths, account series, cash flows, taxes,
    /// warnings, real-dollar bands and ledger; an older run serves `stats`
    /// and `real_net_worth.terminal`, and every per-path field is empty.
    pub path_details: bool,
    /// Actual run-local path ID shared by accounts, cash flows and ledger.
    /// Empty when `path_details` is false.
    pub series_id: String,
    /// Per-account decomposition of the path named by `series_percentile`.
    pub account_series: Vec<AccountSeries>,
    pub series_percentile: Option<f64>,
    pub cash_flows: Vec<CashFlow>,
    pub warnings: Vec<Warning>,
    /// Cumulative inflation on the same path as `cash_flows`, one point per
    /// plan year. Empty for a run stored before inflation was recorded.
    pub inflation: Vec<InflationPoint>,
    /// Per-year ledger index, for the years the ledger covers.
    pub ledger_years: Vec<LedgerYear>,
    /// When, where and how the iterations that failed the funding check
    /// failed, over the whole run rather than the shown path. Kept for
    /// superseded runs too. Null for runs stored before it was measured.
    pub funding_diagnostics: Option<FundingDiagnostics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LedgerPage {
    pub run_id: i64,
    pub series_id: String,
    pub entries: Vec<LedgerEntry>,
    /// Entries matching the filter, of which `entries` is one page.
    pub total: i64,
}

/// The query of `GET /runs/{id}/ledger`.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct LedgerQuery {
    /// Which path to read, matching `ResultsQuery::series`.
    #[serde(default)]
    pub series: Option<String>,
    /// Restrict to one calendar year — how the cash-flow table reads the
    /// entries behind a row it has expanded.
    #[serde(default)]
    pub year: Option<i64>,
    /// One of `cash`, `asset`, `tax`, `event`; omit for all four.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}
