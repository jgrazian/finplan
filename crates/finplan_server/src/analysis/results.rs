//! What an analysis hands back.
//!
//! Deliberately flat and small. A sweep's grid is a list of cells with their
//! indices, not a nested array, so the client can index it however the screen
//! wants — a heatmap reads it as a grid, the frontier table reads it as rows,
//! and the slice charts read it as two lines through one point. The threshold,
//! the frontier and the slices are all derived from the same cells on the
//! client, which is why changing the threshold does not need another run.

use finplan_core::analysis::{SolveConstraintMetric, SolveMethod, SolveProbe, SolveResults};
use finplan_core::model::MonteCarloStats;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::params::PlanParameter;

/// A number in the plan that a sweep axis, a sensitivity row or a solve can
/// vary, and the range it defaults to.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AnalysisParameter {
    /// `parameter:<database id>`, stable across renames.
    pub id: String,
    pub parameter_id: i64,
    pub name: String,
    /// `age` (years), `amount` (money), `rate` (fraction), or `date` (UTC epoch days).
    pub kind: String,
    /// The plan's own value today.
    pub current: f64,
    pub min: f64,
    pub max: f64,
}

impl From<&PlanParameter> for AnalysisParameter {
    fn from(p: &PlanParameter) -> Self {
        Self {
            id: p.id.clone(),
            parameter_id: p.parameter_id,
            name: p.name.clone(),
            kind: p.kind.as_str().to_string(),
            current: p.current,
            min: p.min,
            max: p.max,
        }
    }
}

/// One outcome, wherever it was measured — a grid cell, a probe, a baseline.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisPoint {
    /// Fraction of runs ending solvent, 0–1.
    pub success_rate: f64,
    /// Fraction of runs that met every cash need on time; `null` where the run
    /// predates the check.
    pub funding_success_rate: Option<f64>,
    /// Terminal net worth at the 5th, 50th and 95th percentile, nominal.
    pub p5: f64,
    pub p50: f64,
    pub p95: f64,
}

impl From<&MonteCarloStats> for AnalysisPoint {
    fn from(stats: &MonteCarloStats) -> Self {
        let at = |p: f64| {
            stats
                .percentile_values
                .iter()
                .find(|(q, _)| (q - p).abs() < 1e-6)
                .map_or(0.0, |(_, v)| *v)
        };
        Self {
            success_rate: stats.success_rate,
            funding_success_rate: stats.funding_success_rate,
            p5: at(0.05),
            p50: at(0.50),
            p95: at(0.95),
        }
    }
}

/// The same, read off a solve probe rather than a grid point.
fn point_from_probe(probe: &SolveProbe) -> AnalysisPoint {
    let at = |p: f64| probe.percentile(p).unwrap_or(0.0);
    AnalysisPoint {
        success_rate: probe.success_rate,
        funding_success_rate: probe.funding_success_rate,
        p5: at(0.05),
        p50: at(0.50),
        p95: at(0.95),
    }
}

/// One axis of a sweep: which parameter, and the values it was stepped over.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SweepAxis {
    pub parameter_id: String,
    /// The event and what varies — "Retirement spending · amount".
    pub label: String,
    /// Just what varies — "amount". Short enough for a table header or a
    /// stat label, where the surrounding heading already names the event.
    pub role: String,
    pub kind: String,
    pub values: Vec<f64>,
}

/// One evaluated combination. `indices` positions it on the axes above, in the
/// same order, and carries one entry per swept variable.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SweepCell {
    pub indices: Vec<u32>,
    #[serde(flatten)]
    #[ts(flatten)]
    pub point: AnalysisPoint,
}

/// A finished sweep.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SweepResults {
    /// New graphs use this metric; absent legacy caches used terminal wealth.
    #[serde(default)]
    pub default_metric: Option<String>,
    /// The swept variables, in the order the cells' indices follow. A graph
    /// picks one or two of these for its own axes and holds the rest.
    pub axes: Vec<SweepAxis>,
    /// Row-major over the axes: the last axis varies fastest.
    pub cells: Vec<SweepCell>,
    /// The plan as it stands, run once, so every cell has something to be a
    /// change from.
    pub plan: AnalysisPoint,
    /// Where the plan's own values sit on the axes, to the nearest step, or
    /// `null` when they fall outside a swept range.
    pub plan_indices: Option<Vec<u32>>,
    /// Monte Carlo iterations behind each cell.
    pub iterations: u32,
}

/// A sweep read back from the cache rather than from the job that ran it.
///
/// Carries when it was run, because a restored grid is the one thing on the
/// Analysis screen that may be older than the plan it describes: the screen
/// says so in its footer instead of passing it off as this session's answer.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CachedSweep {
    pub scenario_id: i64,
    /// When the sweep finished, UTC, `YYYY-MM-DD HH:MM:SS`.
    pub created_at: String,
    pub results: SweepResults,
    /// The graphs arranged over this grid, exactly as the client stored them,
    /// or `null` where nobody has arranged any. Opaque here: what a graph is
    /// drawn as, against what, and sliced where are the client's choices, and
    /// typing them server-side would mean a deploy to add a chart kind.
    #[ts(type = "unknown")]
    pub layout: Option<serde_json::Value>,
}

/// One parameter's ±band and what moving it did.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SensitivityRow {
    pub parameter_id: String,
    pub label: String,
    pub kind: String,
    pub low_value: f64,
    pub high_value: f64,
    pub low: AnalysisPoint,
    pub high: AnalysisPoint,
    /// Success-rate spread across the band, in points. The ranking is by this.
    pub span: f64,
}

/// A finished sensitivity ranking, worst-moving parameter last.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SensitivityResults {
    pub rows: Vec<SensitivityRow>,
    pub plan: AnalysisPoint,
    /// The band each parameter was moved through, as a fraction of its value.
    pub fraction: f64,
    pub iterations: u32,
}

/// One simulation the solver ran.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SolveStep {
    /// Parameter values at this probe, one per varied parameter.
    pub values: Vec<f64>,
    /// Whether it cleared the constraint.
    pub feasible: bool,
    /// The bracket this probe halved. `null` under grid search.
    pub bracket_low: Option<f64>,
    pub bracket_high: Option<f64>,
    #[serde(flatten)]
    #[ts(flatten)]
    pub point: AnalysisPoint,
}

/// A finished goal seek.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SolveOutcome {
    /// Metric actually used by the solver.
    pub constraint: String,
    /// `"bisection"` or `"grid-search"` — the method the selection implied.
    pub method: String,
    /// The varied parameters, in the order every `values` array follows.
    pub parameters: Vec<AnalysisParameter>,
    /// The plan as it stands.
    pub plan: AnalysisPoint,
    /// Every probe, in the order taken.
    pub steps: Vec<SolveStep>,
    /// The answer, or `null` when nothing in range clears the constraint.
    pub best: Option<SolveStep>,
    /// Standard error of the selected constraint at the answer, as a fraction. Says
    /// whether the last digit of the answer means anything.
    pub std_error: Option<f64>,
    pub iterations: u32,
}

impl SolveOutcome {
    /// Convert an engine result, given the parameters it was asked about.
    #[must_use]
    pub fn new(results: &SolveResults, parameters: &[AnalysisParameter]) -> Self {
        let step = |probe: &SolveProbe| SolveStep {
            values: probe.values.clone(),
            feasible: probe.feasible,
            bracket_low: probe.bracket.map(|(lo, _)| lo),
            bracket_high: probe.bracket.map(|(_, hi)| hi),
            point: point_from_probe(probe),
        };
        Self {
            constraint: match results.constraint_metric {
                SolveConstraintMetric::SuccessRate => "success-rate",
                SolveConstraintMetric::FundingSuccessRate => "funding-success-rate",
            }
            .to_string(),
            method: match results.method {
                SolveMethod::Bisection => "bisection",
                SolveMethod::GridSearch => "grid-search",
            }
            .to_string(),
            parameters: parameters.to_vec(),
            plan: point_from_probe(&results.baseline),
            steps: results.probes.iter().map(step).collect(),
            best: results.best.as_ref().map(step),
            std_error: results.constraint_std_error(),
            iterations: results.mc_iterations as u32,
        }
    }
}

/// One cumulative step of a what-if: the plan with the first `i` enabled
/// layers applied.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WhatIfStep {
    pub point: AnalysisPoint,
    /// Real (today's $) median net worth at plan end.
    pub median_end_real: f64,
    /// First age (or year if no birth_date) at which the P10 path's net worth
    /// hits <= 0; null = never.
    pub p10_dry_at: Option<f64>,
}

/// Pointwise real-dollar net worth quantiles, one value per fan point.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WhatIfFan {
    pub p25: Vec<f64>,
    pub p50: Vec<f64>,
    pub p75: Vec<f64>,
}

/// A finished what-if: the plan, then each enabled layer applied on top of
/// the ones before it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WhatIfOutcome {
    /// steps[0] = the plan with no overrides; steps[i] = plan + layers[0..i]
    /// (so steps.len() == layers.len() + 1).
    pub steps: Vec<WhatIfStep>,
    /// Owner age at each point of the fan (whole or fractional years); null when
    /// the scenario has no birth_date, in which case `years` is used.
    pub ages: Option<Vec<f64>>,
    /// Calendar year (fractional ok) at each point of the fan.
    pub years: Vec<f64>,
    /// Fan for steps[0] and for the last step, TODAY'S dollars (deflated).
    pub plan_fan: WhatIfFan,
    pub what_if_fan: WhatIfFan,
    /// Retirement age of plan / what-if, where the plan has a parameter of kind
    /// age whose name contains "retire" (case-insensitive); null otherwise.
    pub plan_retirement_age: Option<f64>,
    pub what_if_retirement_age: Option<f64>,
}

/// The results of whichever analysis was asked for, tagged so the client can
/// narrow on `kind` rather than on which field happens to be present.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[ts(export)]
pub enum AnalysisOutcome {
    Sweep(SweepResults),
    Sensitivity(SensitivityResults),
    Solve(SolveOutcome),
    WhatIf(WhatIfOutcome),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_sweep_preserves_its_default_and_missing_funding() {
        let restored: SweepResults = serde_json::from_value(serde_json::json!({
            "axes": [], "cells": [], "plan_indices": null, "iterations": 100,
            "plan": { "success_rate": 1.0, "funding_success_rate": null, "p5": 0, "p50": 0, "p95": 0 }
        })).unwrap();
        assert!(restored.default_metric.is_none());
        assert!(restored.plan.funding_success_rate.is_none());
    }

    #[test]
    fn historical_solver_uncertainty_keeps_terminal_wealth_semantics() {
        let probe = serde_json::json!({ "values": [], "success_rate": 1.0,
            "funding_success_rate": 0.5, "final_percentiles": [], "feasible": true,
            "objective_value": 0, "bracket": null });
        let restored: SolveResults = serde_json::from_value(serde_json::json!({
            "method": "Bisection", "param_labels": [], "baseline": probe,
            "probes": [], "best": probe, "mc_iterations": 100
        }))
        .unwrap();
        assert_eq!(
            restored.constraint_metric,
            SolveConstraintMetric::SuccessRate
        );
        assert_eq!(restored.constraint_std_error(), Some(0.0));
    }
}
