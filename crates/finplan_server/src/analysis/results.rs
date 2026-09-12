//! What an analysis hands back.
//!
//! Deliberately flat and small. A sweep's grid is a list of cells with their
//! indices, not a nested array, so the client can index it however the screen
//! wants — a heatmap reads it as a grid, the frontier table reads it as rows,
//! and the slice charts read it as two lines through one point. The threshold,
//! the frontier and the slices are all derived from the same cells on the
//! client, which is why changing the threshold does not need another run.

use finplan_core::analysis::{SolveMethod, SolveProbe, SolveResults};
use finplan_core::model::MonteCarloStats;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::params::{ParamKind, PlanParameter};

/// A number in the plan that a sweep axis, a sensitivity row or a solve can
/// vary, and the range it defaults to.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AnalysisParameter {
    /// `event:<event id>:<slot>`, stable for as long as the event exists.
    pub id: String,
    pub event_id: i64,
    pub event_name: String,
    /// What varies — "age", "amount", "starts at age".
    pub role: String,
    /// `"age"` or `"amount"`: how to format it, and what a step means.
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
            event_id: p.event_id,
            event_name: p.event_name.clone(),
            role: p.role.to_string(),
            kind: match p.kind {
                ParamKind::Age => "age",
                ParamKind::Amount => "amount",
            }
            .to_string(),
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
    /// Standard error of the success rate at the answer, as a fraction. Says
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

/// The results of whichever analysis was asked for, tagged so the client can
/// narrow on `kind` rather than on which field happens to be present.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[ts(export)]
pub enum AnalysisOutcome {
    Sweep(SweepResults),
    Sensitivity(SensitivityResults),
    Solve(SolveOutcome),
}
