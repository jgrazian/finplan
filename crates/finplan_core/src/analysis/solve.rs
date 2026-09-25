//! Goal seek: the best value of one or more plan parameters, subject to a
//! constraint on the outcome.
//!
//! A sweep answers "what happens across this grid". Solving answers the
//! question the grid is usually a proxy for — "how much can I spend and still
//! clear 95%?" — at a chosen search resolution.
//!
//! The search follows the selection rather than being chosen:
//!
//! * One parameter, and the objective *is* that parameter (largest spend,
//!   earliest retirement): [`SolveMethod::Bisection`]. The method assumes the constraint is
//!   monotone in the parameter, so the
//!   boundary can be bracketed and halved — a dozen probes rather than a grid.
//! * Anything else — several parameters, or an objective read off the
//!   simulation instead of the parameter: [`SolveMethod::GridSearch`]. Every
//!   combination is evaluated and the best feasible one wins.
//!
//! Each probe is recorded with the bracket it was taken from, so the caller can
//! draw the convergence rather than only its answer.

use serde::{Deserialize, Serialize};

use crate::config::SimulationConfig;
use crate::error::SimulationError;
use crate::model::{MonteCarloConfig, MonteCarloStats};
use crate::simulation::monte_carlo_stats_only;

use super::{SweepConfig, SweepParameter, SweepProgress, apply_parameter, sweep_simulate_lazy};

/// What the solver is trying to make as large — or as small — as possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolveObjective {
    /// The largest value of the varied parameter that still clears the
    /// constraint. With `retirement-spend` varied this is the maximum
    /// sustainable withdrawal.
    MaxParameter,
    /// The smallest such value — an earliest retirement age, a minimum
    /// contribution that still funds the plan.
    MinParameter,
    /// The highest median terminal net worth among feasible points.
    MaxMedianNetWorth,
    /// The highest *floor*: the 5th-percentile terminal net worth. Picks the
    /// setting whose bad outcomes are least bad, rather than its typical one.
    MaxFloorNetWorth,
}

impl SolveObjective {
    /// Whether the objective is the parameter itself, which is what makes a
    /// single-parameter solve bisectable.
    #[must_use]
    pub fn is_parameter(self) -> bool {
        matches!(self, Self::MaxParameter | Self::MinParameter)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::MaxParameter => "Maximum sustainable value",
            Self::MinParameter => "Minimum sufficient value",
            Self::MaxMedianNetWorth => "Max median terminal net worth",
            Self::MaxFloorNetWorth => "Max P5 terminal net worth",
        }
    }
}

/// The outcome measure a constraint is written against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolveConstraintMetric {
    /// Fraction of runs ending with positive net worth.
    SuccessRate,
    /// Fraction of runs that met every cash need on time. Stricter, and `None`
    /// on results produced before the check existed — such a point is read as
    /// infeasible rather than silently passing.
    FundingSuccessRate,
}

/// The bar a candidate has to clear to count as an answer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SolveConstraint {
    pub metric: SolveConstraintMetric,
    /// Inclusive floor, as a fraction: `0.95` for "success ≥ 95%".
    pub min_value: f64,
}

impl Default for SolveConstraint {
    fn default() -> Self {
        Self {
            metric: SolveConstraintMetric::SuccessRate,
            min_value: 0.95,
        }
    }
}

/// How the answer was found, so a caller can say so rather than guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolveMethod {
    Bisection,
    GridSearch,
}

/// One simulation the solver ran, and the bracket it was taken from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveProbe {
    /// Parameter values at this probe, one per varied parameter.
    pub values: Vec<f64>,
    pub success_rate: f64,
    pub funding_success_rate: Option<f64>,
    /// Terminal net worth by percentile, as `(percentile, value)` in nominal
    /// dollars — the same pairs a Monte Carlo run reports.
    pub final_percentiles: Vec<(f64, f64)>,
    /// Whether this probe clears the constraint.
    pub feasible: bool,
    /// The quantity being optimised, at this probe.
    pub objective_value: f64,
    /// The bracket this probe halved, for a bisection. `None` under grid
    /// search, which has no bracket to draw.
    pub bracket: Option<(f64, f64)>,
}

impl SolveProbe {
    /// Terminal net worth at a percentile, if the run reported one.
    #[must_use]
    pub fn percentile(&self, percentile: f64) -> Option<f64> {
        self.final_percentiles
            .iter()
            .find(|(p, _)| (p - percentile).abs() < 1e-6)
            .map(|(_, v)| *v)
    }
}

/// Everything a solve produced: the answer, the plan it is measured against,
/// and the path taken to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveResults {
    /// Constraint used for feasibility and sampling uncertainty. Legacy results used terminal wealth.
    #[serde(default = "legacy_constraint_metric")]
    pub constraint_metric: SolveConstraintMetric,
    pub method: SolveMethod,
    pub param_labels: Vec<String>,
    /// The unmodified plan, evaluated once so every figure has a baseline.
    pub baseline: SolveProbe,
    /// Every probe, in the order it was taken.
    pub probes: Vec<SolveProbe>,
    /// The best feasible probe, or `None` when nothing in range clears the
    /// constraint.
    pub best: Option<SolveProbe>,
    /// Monte Carlo iterations behind each probe, for reporting the error on the
    /// answer.
    pub mc_iterations: usize,
}

fn legacy_constraint_metric() -> SolveConstraintMetric {
    SolveConstraintMetric::SuccessRate
}

impl SolveResults {
    /// Standard error of the constraint metric at the optimum, as a fraction.
    ///
    /// A success rate is a proportion, so its sampling error is
    /// `sqrt(p(1-p)/n)` — the number that says whether "95.0%" is really 95.
    #[must_use]
    pub fn constraint_std_error(&self) -> Option<f64> {
        let best = self.best.as_ref()?;
        let p = match self.constraint_metric {
            SolveConstraintMetric::SuccessRate => best.success_rate,
            SolveConstraintMetric::FundingSuccessRate => best.funding_success_rate?,
        };
        if self.mc_iterations == 0 {
            return None;
        }
        Some((p * (1.0 - p) / self.mc_iterations as f64).sqrt())
    }
}

/// A goal-seek run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveConfig {
    /// The parameters to vary. Their `min_value`/`max_value` bound the search;
    /// `step_count` is the grid resolution, and is ignored under bisection.
    pub parameters: Vec<SweepParameter>,
    pub objective: SolveObjective,
    pub constraint: SolveConstraint,
    /// Monte Carlo iterations behind every probe.
    pub mc_iterations: usize,
    pub parallel_batches: usize,
    /// Fixed seed, so the bisection is not chasing sampling noise between
    /// probes. Left `None` the search still terminates, but two probes a dollar
    /// apart can disagree for reasons that have nothing to do with the dollar.
    pub seed: Option<u64>,
    /// Stop bisecting once the bracket is this narrow, in the parameter's own
    /// units. Zero falls back to a thousandth of the range.
    pub tolerance: f64,
    /// Hard ceiling on bisection probes, whatever the tolerance asks for.
    pub max_probes: usize,
}

impl Default for SolveConfig {
    fn default() -> Self {
        Self {
            parameters: Vec::new(),
            objective: SolveObjective::MaxParameter,
            constraint: SolveConstraint::default(),
            mc_iterations: 250,
            parallel_batches: default_parallel_batches(),
            seed: Some(0),
            tolerance: 0.0,
            max_probes: 16,
        }
    }
}

fn default_parallel_batches() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1)
}

impl SolveConfig {
    /// The method this configuration selects. The caller never picks it.
    #[must_use]
    pub fn method(&self) -> SolveMethod {
        let discrete = self.parameters.iter().any(|p| {
            matches!(
                &p.target,
                super::SweepTarget::Parameter(parameter) if parameter.is_discrete()
            )
        });
        if self.parameters.len() == 1 && self.objective.is_parameter() && !discrete {
            SolveMethod::Bisection
        } else {
            SolveMethod::GridSearch
        }
    }

    /// Upper bound on the simulations a solve will run, for a progress total.
    #[must_use]
    pub fn probe_budget(&self) -> usize {
        // The baseline is a probe like any other, and is always taken.
        1 + match self.method() {
            SolveMethod::Bisection => self.max_probes,
            SolveMethod::GridSearch => self
                .parameters
                .iter()
                .map(|p| p.step_count.max(1))
                .product::<usize>(),
        }
    }
}

/// Percentiles every probe reports, so terminal-net-worth objectives and the
/// answer's own detail read off the same set.
const PROBE_PERCENTILES: [f64; 5] = [0.05, 0.25, 0.50, 0.75, 0.95];

/// Solve for the best parameter values under the constraint.
///
/// Returns the answer alongside every probe taken, including the ones that
/// failed the constraint — the failures are what make the bracket legible.
pub fn solve(
    base_config: &SimulationConfig,
    config: &SolveConfig,
    progress: Option<&SweepProgress>,
) -> Result<SolveResults, SimulationError> {
    if config.parameters.is_empty() {
        return Err(SimulationError::Config(
            "At least one parameter to vary is required".to_string(),
        ));
    }
    if !(0.0..=1.0).contains(&config.constraint.min_value) {
        return Err(SimulationError::Config(
            "Constraint threshold must be a fraction between 0 and 1".to_string(),
        ));
    }
    for param in &config.parameters {
        if param.max_value <= param.min_value {
            return Err(SimulationError::Config(format!(
                "{} has an empty range",
                param.label()
            )));
        }
    }

    if let Some(p) = progress {
        p.reset(config.probe_budget() * config.mc_iterations);
    }

    let labels: Vec<String> = config
        .parameters
        .iter()
        .map(SweepParameter::label)
        .collect();
    let baseline = evaluate(base_config, config, &[], progress)?;

    let mut results = match config.method() {
        SolveMethod::Bisection => bisect(base_config, config, progress)?,
        SolveMethod::GridSearch => grid_search(base_config, config, progress)?,
    };
    results.param_labels = labels;
    results.baseline = baseline;
    results.mc_iterations = config.mc_iterations;
    Ok(results)
}

/// Run one probe: apply the values, simulate, and score it.
///
/// An empty `values` evaluates the plan as configured, which is how the
/// baseline is taken.
fn evaluate(
    base_config: &SimulationConfig,
    config: &SolveConfig,
    values: &[f64],
    progress: Option<&SweepProgress>,
) -> Result<SolveProbe, SimulationError> {
    if let Some(p) = progress
        && p.is_cancelled()
    {
        return Err(SimulationError::Cancelled);
    }

    let mut modified = base_config.clone();
    modified.collect_ledger = false;
    for (param, value) in config.parameters.iter().zip(values) {
        modified = apply_parameter(&modified, param, *value)?;
    }

    let mc_config = MonteCarloConfig {
        iterations: config.mc_iterations,
        percentiles: PROBE_PERCENTILES.to_vec(),
        compute_mean: false,
        parallel_batches: config.parallel_batches,
        seed: config.seed,
        ..Default::default()
    };

    // Stats only. A probe is scored on its success rate and its terminal
    // percentiles, both of which the first pass produces; the second pass
    // re-runs simulations to rebuild percentile paths nothing here reads, and
    // builds a real-dollar envelope that refuses any plan whose event schedule
    // depends on the market path.
    let mc_progress = progress
        .map(SweepProgress::as_mc_progress)
        .unwrap_or_default();
    let (stats, _seeds) = monte_carlo_stats_only(&modified, &mc_config, &mc_progress)?;
    Ok(probe_from(&stats, config, values.to_vec(), None))
}

fn probe_from(
    stats: &MonteCarloStats,
    config: &SolveConfig,
    values: Vec<f64>,
    bracket: Option<(f64, f64)>,
) -> SolveProbe {
    let observed = match config.constraint.metric {
        SolveConstraintMetric::SuccessRate => Some(stats.success_rate),
        // A result recorded before funding was tracked cannot be shown to meet
        // a funding constraint, so it does not.
        SolveConstraintMetric::FundingSuccessRate => stats.funding_success_rate,
    };
    let feasible = observed.is_some_and(|v| v >= config.constraint.min_value);

    let percentile = |p: f64| {
        stats
            .percentile_values
            .iter()
            .find(|(q, _)| (q - p).abs() < 1e-6)
            .map_or(0.0, |(_, v)| *v)
    };
    let objective_value = match config.objective {
        SolveObjective::MaxParameter | SolveObjective::MinParameter => {
            values.first().copied().unwrap_or(0.0)
        }
        SolveObjective::MaxMedianNetWorth => percentile(0.50),
        SolveObjective::MaxFloorNetWorth => percentile(0.05),
    };

    SolveProbe {
        values,
        success_rate: stats.success_rate,
        funding_success_rate: stats.funding_success_rate,
        final_percentiles: stats.percentile_values.clone(),
        feasible,
        objective_value,
        bracket,
    }
}

/// Is `a` a better answer than `b`?
fn better(objective: SolveObjective, a: f64, b: f64) -> bool {
    match objective {
        SolveObjective::MinParameter => a < b,
        _ => a > b,
    }
}

/// Halve the bracket between the end of the range the objective wants and the
/// nearest point that still clears the constraint.
///
/// This method assumes a monotone constraint with a single crossing. Arbitrary
/// event rules can violate that assumption; use a grid to inspect such plans.
fn bisect(
    base_config: &SimulationConfig,
    config: &SolveConfig,
    progress: Option<&SweepProgress>,
) -> Result<SolveResults, SimulationError> {
    let param = &config.parameters[0];
    let (lo, hi) = (param.min_value, param.max_value);
    let tolerance = if config.tolerance > 0.0 {
        config.tolerance
    } else {
        (hi - lo) / 1000.0
    };

    // `wanted` is the end of the range the objective is reaching for; `other`
    // is the end most likely to satisfy the constraint.
    let (mut wanted, mut other) = match config.objective {
        SolveObjective::MinParameter => (lo, hi),
        _ => (hi, lo),
    };

    let mut probes = Vec::new();
    let mut best: Option<SolveProbe> = None;

    let take = |probes: &mut Vec<SolveProbe>,
                best: &mut Option<SolveProbe>,
                value: f64,
                bracket: Option<(f64, f64)>|
     -> Result<bool, SimulationError> {
        let mut probe = evaluate(base_config, config, &[value], progress)?;
        probe.bracket = bracket;
        let feasible = probe.feasible;
        if feasible
            && best
                .as_ref()
                .is_none_or(|b| better(config.objective, probe.objective_value, b.objective_value))
        {
            *best = Some(probe.clone());
        }
        probes.push(probe);
        Ok(feasible)
    };

    // The end the objective wants: if the constraint holds there, the range
    // itself is what binds and there is nothing to search for.
    if take(&mut probes, &mut best, wanted, Some(span(wanted, other)))? {
        return Ok(finish(SolveMethod::Bisection, probes, best, config));
    }
    // The other end fails too, so nothing in range clears the constraint. The
    // probes still say how far off it was.
    if !take(&mut probes, &mut best, other, Some(span(wanted, other)))? {
        return Ok(finish(SolveMethod::Bisection, probes, best, config));
    }

    while (wanted - other).abs() > tolerance && probes.len() < config.max_probes {
        let mid = f64::midpoint(wanted, other);
        // Guard the degenerate bracket a coarse tolerance can leave behind.
        if mid == wanted || mid == other {
            break;
        }
        let bracket = span(wanted, other);
        if take(&mut probes, &mut best, mid, Some(bracket))? {
            other = mid;
        } else {
            wanted = mid;
        }
    }

    Ok(finish(SolveMethod::Bisection, probes, best, config))
}

/// The bracket as an ordered pair, whichever end the objective is reaching for.
fn span(wanted: f64, other: f64) -> (f64, f64) {
    (wanted.min(other), wanted.max(other))
}

/// Evaluate every combination and keep the best feasible one.
///
/// Reuses the sweep's own grid runner, so a solve over two parameters costs
/// exactly what the equivalent sweep costs and reports the same statistics.
fn grid_search(
    base_config: &SimulationConfig,
    config: &SolveConfig,
    progress: Option<&SweepProgress>,
) -> Result<SolveResults, SimulationError> {
    let sweep_config = SweepConfig {
        parameters: config.parameters.clone(),
        metrics: Vec::new(),
        mc_iterations: config.mc_iterations,
        parallel_batches: config.parallel_batches,
        seed: config.seed,
    };
    let grid = sweep_simulate_lazy(base_config, &sweep_config, progress)?;
    let values = sweep_config.all_sweep_values();

    let mut probes = Vec::new();
    let mut best: Option<SolveProbe> = None;
    for indices in grid.stats.indices() {
        let Some(stats) = grid.get_stats(&indices) else {
            continue;
        };
        let at: Vec<f64> = indices
            .iter()
            .enumerate()
            .map(|(dim, &idx)| values[dim][idx])
            .collect();
        let probe = probe_from(stats, config, at, None);
        if probe.feasible
            && best
                .as_ref()
                .is_none_or(|b| better(config.objective, probe.objective_value, b.objective_value))
        {
            best = Some(probe.clone());
        }
        probes.push(probe);
    }

    Ok(finish(SolveMethod::GridSearch, probes, best, config))
}

/// Assemble the result. `param_labels`, `baseline` and `mc_iterations` are
/// filled in by [`solve`], which is the only caller that has them.
fn finish(
    method: SolveMethod,
    probes: Vec<SolveProbe>,
    best: Option<SolveProbe>,
    config: &SolveConfig,
) -> SolveResults {
    SolveResults {
        constraint_metric: config.constraint.metric,
        method,
        param_labels: Vec::new(),
        baseline: SolveProbe {
            values: Vec::new(),
            success_rate: 0.0,
            funding_success_rate: None,
            final_percentiles: Vec::new(),
            feasible: false,
            objective_value: 0.0,
            bracket: None,
        },
        probes,
        best,
        mc_iterations: config.mc_iterations,
    }
}
