//! A run's results, shaped from the engine's output without a database.
//!
//! The server writes a finished Monte Carlo run into the `run_*` tables and
//! reads it back, shaped by SQL, for `GET /runs/{id}/results`. A browser has no
//! tables, so [`project`] does the shaping straight from a `MonteCarloSummary`:
//! it flattens the summary, through the compile's id map, into a
//! [`RunResults`]. The server persists *from* that value, so there is one
//! shaping path, and [`RunResults::results`] and [`RunResults::ledger_page`]
//! answer what the read endpoints answer.
//!
//! A server test runs the same seeded run through both paths and requires the
//! response bodies to be equal.

use std::collections::{BTreeMap, HashMap, HashSet};

use finplan_core::model::{
    AccountId, MonteCarloSummary, SimulationResult, WarningKind, final_net_worth,
};
use serde::{Deserialize, Serialize};

use crate::compile::CompiledScenario;
use crate::error::PlanResult;

pub mod funding;
pub mod ledger;
pub mod shape;
pub mod view;

use funding::{FundingDiagnostics, funding_view};
use shape::{LEDGER_PAGE_MAX, check_category, factor_for, path_id, resolve_series, year_of};
use view::{
    AccountSeries, Band, CashFlow, InflationPoint, LedgerEntry, LedgerPage, LedgerYear,
    PercentileValue, RealNetWorthSummary, RealQuantilePoint, RealTerminalStats, Results, Stats,
    Warning,
};

/// What a run was asked to do, as far as shaping its results is concerned.
///
/// Almost everything about a run (iterations, seed, percentiles, convergence)
/// is already in the summary the engine returned, so the shaping needs little
/// besides this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSettings {
    /// Flatten each path's ledger. The server always does; a caller that never
    /// shows the ledger can skip the work, and gets a run with none.
    pub include_ledger: bool,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            include_ledger: true,
        }
    }
}

/// One point of a path's net worth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetWorthPoint {
    pub date: String,
    pub net_worth: f64,
}

/// One account's value at one snapshot of a path. `step` is the snapshot's
/// index in the path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountPoint {
    pub account_id: i64,
    pub step: usize,
    pub value: f64,
}

/// A year's cash flows on one path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CashFlowRow {
    pub year: i64,
    pub income: f64,
    pub expenses: f64,
    pub contributions: f64,
    pub withdrawals: f64,
    pub appreciation: f64,
    pub net_cash_flow: f64,
}

/// A year's tax summary on one path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaxRow {
    pub year: i64,
    pub ordinary_income: f64,
    pub capital_gains: f64,
    pub tax_free_withdrawals: f64,
    pub federal_tax: f64,
    pub state_tax: f64,
    pub total_tax: f64,
    pub early_withdrawal_penalties: f64,
}

/// An account that can have a series, in display order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountLabel {
    pub account_id: i64,
    pub label: String,
}

/// Everything stored for one representative path, or for the mean.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathResults {
    /// `None` is the synthetic nominal mean.
    pub percentile: Option<f64>,
    pub net_worth: Vec<NetWorthPoint>,
    /// In snapshot order, then the snapshot's account order.
    pub account_points: Vec<AccountPoint>,
    pub cash_flows: Vec<CashFlowRow>,
    pub taxes: Vec<TaxRow>,
    /// One point per plan year, the first being the plan's first year.
    pub inflation: Vec<InflationPoint>,
    /// Flattened, in order. Empty when the plan collects no ledger, for the
    /// mean, and when [`RunSettings::include_ledger`] is off.
    pub ledger: Vec<LedgerEntry>,
    pub warnings: Vec<Warning>,
}

/// A finished run, flattened into what its results endpoints return.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResults {
    pub stats: Stats,
    pub real_net_worth: Option<RealNetWorthSummary>,
    pub funding_diagnostics: Option<FundingDiagnostics>,
    /// The plan's accounts in display order (their sort order, then id).
    /// Per-account series are labelled and ordered by this.
    pub account_labels: Vec<AccountLabel>,
    /// One per stored path: the representative percentiles in the order the
    /// engine returned them, then the mean when one was computed.
    pub paths: Vec<PathResults>,
}

fn warning_kind_name(kind: WarningKind) -> &'static str {
    match kind {
        WarningKind::EffectSkipped => "EffectSkipped",
        WarningKind::EvaluationFailed => "EvaluationFailed",
        WarningKind::IterationLimitHit => "IterationLimitHit",
        WarningKind::CashShortfall => "CashShortfall",
    }
}

/// Total tax paid across the whole horizon of a single path.
fn lifetime_taxes(result: &SimulationResult) -> f64 {
    result
        .yearly_taxes
        .iter()
        .map(|t| t.total_tax + t.early_withdrawal_penalties)
        .sum()
}

/// Shape a finished Monte Carlo run: the same figures the server stores and
/// serves, keyed by database ids through `compiled`.
#[must_use]
pub fn project(
    compiled: &CompiledScenario,
    summary: &MonteCarloSummary,
    settings: &RunSettings,
) -> RunResults {
    let stats = &summary.stats;

    // Lifetime taxes are reported from the median path when it is available,
    // since a mean-of-paths tax figure is not attributable to any real run.
    let median_taxes = summary
        .percentile_runs
        .iter()
        .min_by(|a, b| {
            (a.0 - 0.5)
                .abs()
                .partial_cmp(&(b.0 - 0.5).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(_, result)| lifetime_taxes(result))
        .unwrap_or(0.0);

    // One value per percentile, the last one given winning, ascending.
    let mut percentile_values: Vec<PercentileValue> = Vec::new();
    for &(percentile, final_net_worth) in &stats.percentile_values {
        match percentile_values
            .iter_mut()
            .find(|v| v.percentile == percentile)
        {
            Some(existing) => existing.final_net_worth = final_net_worth,
            None => percentile_values.push(PercentileValue {
                percentile,
                final_net_worth,
            }),
        }
    }
    percentile_values.sort_by(|a, b| a.percentile.total_cmp(&b.percentile));

    let real_net_worth = summary.real_net_worth.as_ref().map(|real| {
        let mut points: Vec<RealQuantilePoint> = real
            .points
            .iter()
            .map(|point| RealQuantilePoint {
                date: point.date.to_string(),
                p5: point.p5,
                p10: Some(point.p10),
                p25: Some(point.p25),
                p50: point.p50,
                p75: Some(point.p75),
                p90: Some(point.p90),
                p95: point.p95,
            })
            .collect();
        points.sort_by(|a, b| a.date.cmp(&b.date));
        RealNetWorthSummary {
            terminal: RealTerminalStats {
                base_date: real.base_date.to_string(),
                num_iterations: real.num_iterations as i64,
                mean: real.terminal.mean,
                std_dev: real.terminal.std_dev,
                min: real.terminal.min,
                max: real.terminal.max,
            },
            points,
        }
    });

    let account_labels = (0u16..)
        .map(AccountId)
        .map_while(|id| compiled.id_map.account_db_id(id))
        .map(|account_id| AccountLabel {
            account_id,
            label: compiled
                .account_names
                .get(&account_id)
                .cloned()
                .unwrap_or_default(),
        })
        .collect();

    let mut paths: Vec<PathResults> = summary
        .percentile_runs
        .iter()
        .map(|(percentile, result)| project_path(Some(*percentile), compiled, result, settings))
        .collect();
    if let Some(mean) = summary.get_mean_result() {
        paths.push(project_path(None, compiled, &mean, settings));
    }

    RunResults {
        stats: Stats {
            num_iterations: stats.num_iterations as i64,
            success_rate: stats.success_rate,
            funding_success_rate: stats.funding_success_rate,
            mean_final_net_worth: stats.mean_final_net_worth,
            std_dev_final_net_worth: stats.std_dev_final_net_worth,
            min_final_net_worth: stats.min_final_net_worth,
            max_final_net_worth: stats.max_final_net_worth,
            lifetime_taxes: median_taxes,
            converged: stats.converged,
            convergence_metric: stats.convergence_metric.as_ref().map(|m| format!("{m:?}")),
            convergence_value: stats.convergence_value,
            percentile_values,
        },
        real_net_worth,
        // Already translated to row ids, the shape the API serves.
        funding_diagnostics: summary
            .funding
            .as_ref()
            .map(|funding| funding_view(&compiled.id_map, funding)),
        account_labels,
        paths,
    }
}

/// Flatten one path (a percentile run, or the mean).
fn project_path(
    percentile: Option<f64>,
    compiled: &CompiledScenario,
    result: &SimulationResult,
    settings: &RunSettings,
) -> PathResults {
    let mut net_worth = Vec::with_capacity(result.wealth_snapshots.len());
    let mut account_points = Vec::new();
    for (step, snapshot) in result.wealth_snapshots.iter().enumerate() {
        net_worth.push(NetWorthPoint {
            date: snapshot.date.to_string(),
            net_worth: snapshot.accounts.iter().map(|a| a.total_value()).sum(),
        });
        for account in &snapshot.accounts {
            // An account created mid-simulation by a `CreateAccount` effect has
            // no database row to attribute to; its value still counts toward net
            // worth above, it just gets no per-account series.
            let Some(account_id) = compiled.id_map.account_db_id(account.account_id) else {
                continue;
            };
            account_points.push(AccountPoint {
                account_id,
                step,
                value: account.total_value(),
            });
        }
    }

    let cash_flows = result
        .yearly_cash_flows
        .iter()
        .map(|flow| CashFlowRow {
            year: i64::from(flow.year),
            income: flow.income,
            expenses: flow.expenses,
            contributions: flow.contributions,
            withdrawals: flow.withdrawals,
            appreciation: flow.appreciation,
            net_cash_flow: flow.net_cash_flow,
        })
        .collect();

    let taxes = result
        .yearly_taxes
        .iter()
        .map(|tax| TaxRow {
            year: i64::from(tax.year),
            ordinary_income: tax.ordinary_income,
            capital_gains: tax.capital_gains,
            tax_free_withdrawals: tax.tax_free_withdrawals,
            federal_tax: tax.federal_tax,
            state_tax: tax.state_tax,
            total_tax: tax.total_tax,
            early_withdrawal_penalties: tax.early_withdrawal_penalties,
        })
        .collect();

    // The path's own realised inflation, keyed by calendar year so a client can
    // deflate any figure it holds without knowing the plan's step cadence.
    // Index 0 is the plan's first year, where the factor is 1.0 by definition.
    let inflation = match result.wealth_snapshots.first() {
        Some(first) => {
            let start_year = i64::from(first.date.year());
            result
                .cumulative_inflation
                .iter()
                .enumerate()
                .map(|(offset, factor)| InflationPoint {
                    year: start_year + offset as i64,
                    factor: *factor,
                })
                .collect()
        }
        None => Vec::new(),
    };

    // The ledger, flattened. Empty when the scenario has `collect_ledger` off,
    // and for the synthetic mean path, which averages figures rather than
    // replaying any one sequence of events.
    let names = ledger::Names::new(compiled);
    let mut entries = Vec::new();
    if settings.include_ledger {
        for entry in &result.ledger {
            let Some(flat) = ledger::flatten(entry, &names) else {
                continue;
            };
            entries.push(LedgerEntry {
                position: entries.len() as i64,
                date: entry.date.to_string(),
                year: i64::from(entry.date.year()),
                category: flat.category.to_string(),
                kind: flat.kind,
                detail: flat.detail,
                amount: flat.amount,
                basis: flat.basis,
                basis_label: flat.basis_label.map(str::to_string),
                account_id: flat.account_id,
                event_id: flat.event_id,
            });
        }
    }

    let warnings = result
        .warnings
        .iter()
        .map(|warning| Warning {
            kind: warning_kind_name(warning.kind).to_string(),
            date: Some(warning.date.to_string()),
            event_id: warning
                .event_id
                .and_then(|id| compiled.id_map.event_db_id(id)),
            account_id: warning.account_id.and_then(|id| names.account_row(id)),
            message: names.readable(&warning.message),
        })
        .collect();

    // Keep the engine's own net-worth helper honest against what we stored.
    debug_assert!(
        result.wealth_snapshots.is_empty()
            || (final_net_worth(result) - net_worth.last().map_or(0.0, |p| p.net_worth)).abs()
                < 1.0
    );

    PathResults {
        percentile,
        net_worth,
        account_points,
        cash_flows,
        taxes,
        inflation,
        ledger: entries,
        warnings,
    }
}

impl PathResults {
    fn factors(&self) -> Vec<(i64, f64)> {
        let mut factors: Vec<(i64, f64)> =
            self.inflation.iter().map(|p| (p.year, p.factor)).collect();
        factors.sort_by_key(|(year, _)| *year);
        factors
    }

    /// The per-year ledger index: entry counts per bucket, and the events that
    /// started in each year.
    fn ledger_years(&self) -> Vec<LedgerYear> {
        let mut years: BTreeMap<i64, LedgerYear> = BTreeMap::new();
        for entry in &self.ledger {
            let year = years.entry(entry.year).or_insert_with(|| LedgerYear {
                year: entry.year,
                cash: 0,
                asset: 0,
                tax: 0,
                event: 0,
                total: 0,
                tags: Vec::new(),
            });
            match entry.category.as_str() {
                "cash" => year.cash += 1,
                "asset" => year.asset += 1,
                "tax" => year.tax += 1,
                "event" => year.event += 1,
                _ => {}
            }
            year.total += 1;
        }

        // What makes a year worth a second look is an event *starting* in it —
        // retiring, a pension beginning, RMDs coming due. Two things it is not:
        // ranking entry kinds would tag every year after retirement, since every
        // one of them withdraws and sells, and a monthly event fires in all of
        // them. So each event tags only the first year it triggered in, and the
        // tags of a year are in the order the events fired. The name stands in
        // for a deleted event, which has no id left to tell it apart by.
        let mut seen: HashSet<(i64, &str)> = HashSet::new();
        for entry in self.ledger.iter().filter(|e| e.kind == "Triggered") {
            if seen.insert((entry.event_id.unwrap_or(-1), entry.detail.as_str()))
                && let Some(year) = years.get_mut(&entry.year)
            {
                year.tags.push(entry.detail.clone());
            }
        }
        years.into_values().collect()
    }
}

impl RunResults {
    /// The stored paths, the mean first and then by ascending percentile: the
    /// order the results are served in.
    fn stored(&self) -> Vec<&PathResults> {
        let mut stored: Vec<&PathResults> = self
            .paths
            .iter()
            .filter(|path| !path.net_worth.is_empty())
            .collect();
        stored.sort_by(|a, b| match (a.percentile, b.percentile) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(a), Some(b)) => a.total_cmp(&b),
        });
        stored
    }

    fn path(&self, percentile: Option<f64>) -> Option<&PathResults> {
        self.paths.iter().find(|path| path.percentile == percentile)
    }

    /// What `GET /runs/{id}/results` answers for a run that has kept its path
    /// details. `series` is the endpoint's query: `mean`, a percentile, or
    /// none for the one nearest the median.
    pub fn results(
        &self,
        run_id: i64,
        scenario_id: i64,
        series: Option<&str>,
    ) -> PlanResult<Results> {
        let stored = self.stored();
        let stored_percentiles: Vec<Option<f64>> = stored.iter().map(|p| p.percentile).collect();

        let bands = stored
            .iter()
            .map(|path| {
                // Deflating a representative path does NOT turn it into a
                // pointwise real quantile; those come from `real_net_worth`.
                let factors = path.factors();
                Band {
                    path_id: path_id(path.percentile),
                    percentile: path.percentile,
                    dates: path.net_worth.iter().map(|p| p.date.clone()).collect(),
                    net_worth: path.net_worth.iter().map(|p| p.net_worth).collect(),
                    inflation: path
                        .net_worth
                        .iter()
                        .map(|p| factor_for(&factors, year_of(&p.date)))
                        .collect(),
                }
            })
            .collect();

        // Which path the per-account series and cash flows describe.
        let path_details = !stored.is_empty();
        let series_percentile = if path_details {
            resolve_series(series, &stored_percentiles)?
        } else {
            None
        };
        let shown = self.path(series_percentile);

        let mut account_series = Vec::new();
        if let Some(shown) = shown {
            let mut values: HashMap<i64, Vec<f64>> = HashMap::new();
            for point in &shown.account_points {
                values
                    .entry(point.account_id)
                    .or_default()
                    .push(point.value);
            }
            for label in &self.account_labels {
                if let Some(values) = values.remove(&label.account_id) {
                    account_series.push(AccountSeries {
                        account_id: label.account_id,
                        label: label.label.clone(),
                        values,
                    });
                }
            }
        }

        let cash_flows = shown
            .map(|path| {
                let mut flows: Vec<&CashFlowRow> = path.cash_flows.iter().collect();
                flows.sort_by_key(|flow| flow.year);
                flows
                    .into_iter()
                    .map(|flow| CashFlow {
                        year: flow.year,
                        income: flow.income,
                        expenses: flow.expenses,
                        contributions: flow.contributions,
                        withdrawals: flow.withdrawals,
                        appreciation: flow.appreciation,
                        net_cash_flow: flow.net_cash_flow,
                        taxes: path
                            .taxes
                            .iter()
                            .find(|tax| tax.year == flow.year)
                            .map(|tax| tax.total_tax + tax.early_withdrawal_penalties)
                            .unwrap_or(0.0),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let warnings = shown.map(|path| path.warnings.clone()).unwrap_or_default();

        let inflation = shown
            .map(|path| {
                let mut points = path.inflation.clone();
                points.sort_by_key(|p| p.year);
                points
            })
            .unwrap_or_default();

        Ok(Results {
            run_id,
            scenario_id,
            stats: self.stats.clone(),
            bands,
            real_net_worth: self.real_net_worth.clone(),
            path_details,
            series_id: if path_details {
                path_id(series_percentile)
            } else {
                String::new()
            },
            account_series,
            series_percentile,
            cash_flows,
            warnings,
            inflation,
            ledger_years: shown.map(PathResults::ledger_years).unwrap_or_default(),
            funding_diagnostics: self.funding_diagnostics.clone(),
        })
    }

    /// What `GET /runs/{id}/ledger` answers: a page of the named path's
    /// ledger, filtered by `year` and `category`.
    pub fn ledger_page(
        &self,
        run_id: i64,
        series: Option<&str>,
        year: Option<i64>,
        category: Option<&str>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> PlanResult<LedgerPage> {
        let stored: Vec<Option<f64>> = self.stored().iter().map(|p| p.percentile).collect();
        let percentile = resolve_series(series, &stored)?;
        check_category(category)?;

        let limit = limit.unwrap_or(LEDGER_PAGE_MAX).clamp(1, LEDGER_PAGE_MAX);
        let offset = offset.unwrap_or(0).max(0);

        let matching: Vec<&LedgerEntry> = self
            .path(percentile)
            .map(|path| {
                path.ledger
                    .iter()
                    .filter(|e| year.is_none_or(|y| e.year == y))
                    .filter(|e| category.is_none_or(|c| e.category == c))
                    .collect()
            })
            .unwrap_or_default();

        Ok(LedgerPage {
            run_id,
            series_id: path_id(percentile),
            total: matching.len() as i64,
            entries: matching
                .into_iter()
                .skip(offset as usize)
                .take(limit as usize)
                .cloned()
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests;
