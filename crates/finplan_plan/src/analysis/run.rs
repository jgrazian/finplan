//! Evaluating an analysis: the loops that decide which configs to simulate,
//! and the shaping of what comes back into the response bodies.
//!
//! Every simulation goes through the [`McRunner`] passed in, so the same code
//! answers a server's rayon-backed runner and a sequential one in the browser.
//! The loops never spawn, sleep or read a clock; progress and cancellation are
//! the runner's.

use std::fmt;

use finplan_core::analysis::{
    McRunner, SweepParameter, apply_parameter, solve_with, sweep_simulate_lazy_with,
};
use finplan_core::config::SimulationConfig;
use finplan_core::error::SimulationError;
use finplan_core::model::{MonteCarloConfig, MonteCarloStats, RealQuantilePoint};

use super::params::PlanParameter;
use super::prepare::{AnalysisSpec, Limits, Prepared, prepare};
use super::request::CreateAnalysis;
use super::results::{
    AnalysisOutcome, AnalysisParameter, AnalysisPoint, SensitivityResults, SensitivityRow,
    SolveOutcome, SweepAxis, SweepCell, SweepResults, WhatIfFan, WhatIfOutcome, WhatIfStep,
};
use crate::error::PlanError;
use crate::graph::ScenarioGraph;

/// Terminal net worth percentiles every analysis asks for, so a cell, a probe
/// and the plan baseline are all measured the same way.
const PERCENTILES: [f64; 5] = [0.05, 0.25, 0.50, 0.75, 0.95];

/// Why an analysis produced nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum AnalysisError {
    /// The request was refused before anything ran.
    Plan(PlanError),
    /// The runner reported cancellation. Not an error to show: it is the state
    /// the caller asked for.
    Cancelled,
    /// The engine, or the shaping of its output, failed.
    Failed(String),
}

impl AnalysisError {
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(error) => error.fmt(f),
            Self::Cancelled => f.write_str("analysis canceled"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for AnalysisError {}

impl From<PlanError> for AnalysisError {
    fn from(error: PlanError) -> Self {
        Self::Plan(error)
    }
}

impl From<SimulationError> for AnalysisError {
    fn from(error: SimulationError) -> Self {
        match error {
            SimulationError::Cancelled => Self::Cancelled,
            other => Self::Failed(other.to_string()),
        }
    }
}

type RunResult<T> = Result<T, AnalysisError>;

/// Check `body` against `graph`, then run it: [`prepare`] and [`run`] in one
/// call, for a host with nothing to do between the two.
pub fn analyze(
    graph: &ScenarioGraph,
    body: CreateAnalysis,
    limits: &Limits,
    runner: &mut dyn McRunner,
) -> RunResult<AnalysisOutcome> {
    let Prepared { base, spec, .. } = prepare(graph, body, limits)?;
    run(&base, &spec, runner)
}

/// Run a prepared analysis to completion on `runner`.
pub fn run(
    base: &SimulationConfig,
    spec: &AnalysisSpec,
    runner: &mut dyn McRunner,
) -> RunResult<AnalysisOutcome> {
    match spec {
        AnalysisSpec::Sweep { params, config } => Ok(AnalysisOutcome::Sweep(run_sweep(
            base, params, config, runner,
        )?)),
        AnalysisSpec::Sensitivity {
            params,
            fraction,
            iterations,
            parallel_batches,
            seed,
        } => Ok(AnalysisOutcome::Sensitivity(run_sensitivity(
            base,
            params,
            *fraction,
            (*iterations, *parallel_batches, *seed),
            runner,
        )?)),
        AnalysisSpec::Solve { params, config } => {
            let mut results = solve_with(base, config, runner)?;
            for probe in results.probes.iter_mut().chain(results.best.iter_mut()) {
                for (i, value) in probe.values.iter_mut().enumerate() {
                    *value = params[i].display_coordinate(&config.parameters[i], *value);
                }
                if let Some((lo, hi)) = probe.bracket.as_mut() {
                    *lo = params[0].display_coordinate(&config.parameters[0], *lo);
                    *hi = params[0].display_coordinate(&config.parameters[0], *hi);
                }
            }
            let described: Vec<AnalysisParameter> = params.iter().map(Into::into).collect();
            Ok(AnalysisOutcome::Solve(SolveOutcome::new(
                &results, &described,
            )))
        }
        AnalysisSpec::WhatIf {
            steps,
            iterations,
            parallel_batches,
            seed,
            plan_retirement_age,
            what_if_retirement_age,
        } => Ok(AnalysisOutcome::WhatIf(run_what_if(
            steps,
            (*iterations, *parallel_batches, *seed),
            (*plan_retirement_age, *what_if_retirement_age),
            runner,
        )?)),
    }
}

/// The Monte Carlo config every point of an analysis is simulated with.
fn mc_config(iterations: usize, parallel_batches: usize, seed: Option<u64>) -> MonteCarloConfig {
    MonteCarloConfig {
        iterations,
        percentiles: PERCENTILES.to_vec(),
        compute_mean: false,
        parallel_batches,
        seed,
        ..Default::default()
    }
}

/// Run each cumulative what-if step on the same seed, and read the plan and
/// the last step's fans off the engine's real-dollar envelope — the same
/// deflated, pointwise quantiles a run stores for its Results chart.
fn run_what_if(
    steps: &[SimulationConfig],
    (iterations, parallel_batches, seed): (usize, usize, Option<u64>),
    (plan_retirement_age, what_if_retirement_age): (Option<f64>, Option<f64>),
    runner: &mut dyn McRunner,
) -> RunResult<WhatIfOutcome> {
    let Some(plan) = steps.first() else {
        return Err(AnalysisError::Failed("a what-if has no steps".into()));
    };
    let birth_date = plan.birth_date;

    let mut outcome_steps = Vec::with_capacity(steps.len());
    let mut envelopes = Vec::with_capacity(steps.len());
    for config in steps {
        if runner.cancelled() {
            return Err(AnalysisError::Cancelled);
        }
        let mut config = config.clone();
        config.collect_ledger = false;
        let mc = mc_config(iterations, parallel_batches, seed);
        let summary = runner.summary(&config, &mc)?;
        let real = summary
            .real_net_worth
            .ok_or_else(|| AnalysisError::Failed("the run has no real-dollar envelope".into()))?;
        let at = |date: jiff::civil::Date| match birth_date {
            Some(birth) => fractional_years(date) - fractional_years(birth),
            None => fractional_years(date),
        };
        outcome_steps.push(WhatIfStep {
            point: AnalysisPoint::from(&summary.stats),
            median_end_real: real.points.last().map_or(0.0, |p| p.p50),
            p10_dry_at: real
                .points
                .iter()
                .find(|p| p.p10 <= 0.0)
                .map(|p| at(p.date)),
        });
        envelopes.push(real.points);
    }

    let fan = |points: &[RealQuantilePoint]| WhatIfFan {
        p25: points.iter().map(|p| p.p25).collect(),
        p50: points.iter().map(|p| p.p50).collect(),
        p75: points.iter().map(|p| p.p75).collect(),
    };
    let plan_points = envelopes.first().cloned().unwrap_or_default();
    let last_points = envelopes.last().cloned().unwrap_or_default();
    let years: Vec<f64> = plan_points
        .iter()
        .map(|p| fractional_years(p.date))
        .collect();
    let ages = birth_date.map(|birth| {
        let born = fractional_years(birth);
        years.iter().map(|year| year - born).collect()
    });

    Ok(WhatIfOutcome {
        steps: outcome_steps,
        ages,
        years,
        plan_fan: fan(&plan_points),
        what_if_fan: fan(&last_points),
        plan_retirement_age,
        what_if_retirement_age,
    })
}

/// A date as a fractional calendar year: 2030-07-02 is about 2030.5.
fn fractional_years(date: jiff::civil::Date) -> f64 {
    f64::from(date.year()) + (f64::from(date.day_of_year()) - 1.0) / f64::from(date.days_in_year())
}

/// The plan as configured, measured the same way every cell is.
fn plan_point(
    base: &SimulationConfig,
    batch: (usize, usize, Option<u64>),
    runner: &mut dyn McRunner,
) -> RunResult<AnalysisPoint> {
    Ok(AnalysisPoint::from(&simulate(base, batch, runner)?))
}

fn simulate(
    config: &SimulationConfig,
    (iterations, parallel_batches, seed): (usize, usize, Option<u64>),
    runner: &mut dyn McRunner,
) -> RunResult<MonteCarloStats> {
    if runner.cancelled() {
        return Err(AnalysisError::Cancelled);
    }
    let mut config = config.clone();
    // Nothing here reads the ledger, and it is the most expensive thing a
    // simulation collects.
    config.collect_ledger = false;

    let mc = mc_config(iterations, parallel_batches, seed);
    // Stats only, which is all any of the three analyses read. The second pass
    // re-runs simulations to rebuild percentile paths none of them look at, and
    // on the way it accumulates a real-dollar envelope that requires every
    // iteration to share one snapshot date grid — something a plan with a
    // balance- or net-worth-triggered event cannot promise.
    let (stats, _seeds) = runner.stats(&config, &mc)?;
    Ok(stats)
}

fn run_sweep(
    base: &SimulationConfig,
    params: &[PlanParameter],
    config: &finplan_core::analysis::SweepConfig,
    runner: &mut dyn McRunner,
) -> RunResult<SweepResults> {
    // The grid first: it resets the shared counter as it starts, so anything
    // measured before it would be counted and then forgotten.
    let grid = sweep_simulate_lazy_with(base, config, runner)?;
    let values: Vec<Vec<f64>> = config
        .all_sweep_values()
        .into_iter()
        .enumerate()
        .map(|(i, values)| {
            values
                .into_iter()
                .map(|value| {
                    params.get(i).map_or(value, |p| {
                        p.display_coordinate(&config.parameters[i], value)
                    })
                })
                .collect()
        })
        .collect();
    let plan = plan_point(
        base,
        (config.mc_iterations, config.parallel_batches, config.seed),
        runner,
    )?;

    let axes: Vec<SweepAxis> = params
        .iter()
        .zip(&values)
        .map(|(param, steps)| SweepAxis {
            parameter_id: param.id.clone(),
            label: param.name.clone(),
            role: param.name.clone(),
            kind: AnalysisParameter::from(param).kind,
            values: steps.clone(),
        })
        .collect();

    let mut cells = Vec::with_capacity(grid.total_points());
    for indices in grid.stats.indices() {
        let Some(stats) = grid.get_stats(&indices) else {
            continue;
        };
        cells.push(SweepCell {
            indices: indices.iter().map(|&i| i as u32).collect(),
            point: AnalysisPoint::from(stats),
        });
    }

    Ok(SweepResults {
        default_metric: Some("funding".to_string()),
        axes,
        cells,
        plan,
        plan_indices: plan_indices(params, &values),
        iterations: config.mc_iterations as u32,
    })
}

/// Where the plan's own values land on the swept axes.
///
/// `None` as soon as one parameter's current value falls outside the range that
/// was swept: half a marker is worse than none, because it would be drawn on a
/// cell the plan is not in.
fn plan_indices(params: &[PlanParameter], values: &[Vec<f64>]) -> Option<Vec<u32>> {
    params
        .iter()
        .zip(values)
        .map(|(param, steps)| {
            let (lo, hi) = (*steps.first()?, *steps.last()?);
            if param.current < lo.min(hi) || param.current > lo.max(hi) {
                return None;
            }
            let nearest = steps
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (*a - param.current)
                        .abs()
                        .total_cmp(&(*b - param.current).abs())
                })
                .map(|(i, _)| i as u32)?;
            Some(nearest)
        })
        .collect()
}

/// Move each parameter through its band on its own, and rank by how much
/// success moved.
///
/// Two simulations per parameter rather than a grid, which is what makes this
/// cheap enough to be the thing you run before deciding what to sweep.
fn run_sensitivity(
    base: &SimulationConfig,
    params: &[PlanParameter],
    fraction: f64,
    batch: (usize, usize, Option<u64>),
    runner: &mut dyn McRunner,
) -> RunResult<SensitivityResults> {
    let plan = plan_point(base, batch, runner)?;

    let mut rows = Vec::with_capacity(params.len());
    for param in params {
        let (low_value, high_value) = param.perturbed(fraction);
        if (high_value - low_value).abs() < f64::EPSILON {
            continue;
        }
        let mut at = |value: f64| -> RunResult<AnalysisPoint> {
            let axis: SweepParameter = param
                .sweep(value, value, 1)
                .map_err(|e| AnalysisError::Failed(e.to_string()))?;
            let modified = apply_parameter(base, &axis, axis.min_value)?;
            plan_point(&modified, batch, runner)
        };

        let low = at(low_value)?;
        let high = at(high_value)?;
        rows.push(SensitivityRow {
            parameter_id: param.id.clone(),
            label: param.name.clone(),
            kind: AnalysisParameter::from(param).kind,
            low_value,
            high_value,
            span: (high.success_rate - low.success_rate).abs() * 100.0,
            low,
            high,
        });
    }

    // Biggest mover first: the ranking is the point of the screen.
    rows.sort_by(|a, b| b.span.total_cmp(&a.span));

    Ok(SensitivityResults {
        rows,
        plan,
        fraction,
        iterations: batch.0 as u32,
    })
}
