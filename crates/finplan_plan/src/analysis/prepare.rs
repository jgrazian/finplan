//! From a request to the work: check it, lower it onto the compiled plan and
//! cost it, before anything is admitted or simulated.
//!
//! What a host adds on top is its own policy (who may ask, what an account may
//! spend) and passes in what that policy decided as [`Limits`]; every bound
//! that is a property of the question itself lives here, so a plan analysed in
//! the browser and one analysed on a server are turned away for the same
//! reasons.

use finplan_core::analysis::{SolveConfig, SolveConstraint, SweepConfig, SweepParameter};
use finplan_core::config::SimulationConfig;

use super::params::{PlanParameter, parameters};
use super::request::{AxisRequest, ConstraintRequest, CreateAnalysis};
use crate::compile;
use crate::error::{PlanError, PlanResult};
use crate::graph::ScenarioGraph;
use crate::what_if;

/// Bounds on what a single request may ask for, so one browser tab cannot
/// queue an hour of CPU.
pub const MAX_STEPS: usize = 12;
pub const MAX_AXES: usize = 4;
pub const MAX_VARIED: usize = 3;

/// The real ceiling on a sweep: how many combinations it may evaluate.
///
/// Counting axes is the wrong limit once a sweep can carry more than two of
/// them — four axes of three steps is 81 cells and finishes, two axes of twelve
/// is 144 and is the grid the old two-axis ceiling allowed. What costs time is
/// the product, so that is what is capped, and at the analysis default of 250
/// iterations it holds a request to 128,000 simulations.
pub const MAX_SWEEP_POINTS: usize = 512;
pub const MAX_ANALYSIS_ITERATIONS: usize = 2_000;
pub const MIN_ITERATIONS: usize = 25;
/// Monte Carlo iterations behind a whole what-if when the request names none.
///
/// A budget for the stack, not per step: it is split evenly across the plan and
/// each layer, so adding an override does not make every nudge slower. Every
/// step runs on the same seed, which keeps the step-to-step differences stable
/// even at a few dozen iterations each.
pub const DEFAULT_WHAT_IF_ITERATIONS: usize = 500;

/// The default grid resolution: six steps an axis, which is what a heatmap can
/// label without crowding.
pub const DEFAULT_STEPS: usize = 6;

/// Simulated years an analysis may cost (simulations times horizon); beyond it
/// the request is refused before any quota is used.
pub const MAX_COST: usize = 20_000_000;

/// Every analysis is seeded, and with the same seed.
///
/// Two cells of a grid differ by the parameter or they differ by nothing;
/// letting them draw different market paths would put noise into exactly the
/// comparison the screen exists to make. It also means a re-run of an unchanged
/// question returns the same answer, which is the behaviour anyone comparing
/// two screenshots expects.
pub const ANALYSIS_SEED: u64 = 0x5EED;

/// What the host decided about this request before asking the plan crate to
/// lower it.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most iterations the caller may ask for in one analysis (their plan's or
    /// the deployment's, whichever is lower).
    pub iteration_cap: usize,
    /// Batches each Monte Carlo run is split into, which a parallel runner
    /// spreads over threads. Part of the batch plan, so it is part of the
    /// answer: the same value gives the same numbers on any runner.
    pub parallel_batches: usize,
}

/// The work an analysis will do, already lowered to engine types.
pub enum AnalysisSpec {
    Sweep {
        params: Vec<PlanParameter>,
        config: SweepConfig,
    },
    Sensitivity {
        params: Vec<PlanParameter>,
        fraction: f64,
        iterations: usize,
        parallel_batches: usize,
        seed: Option<u64>,
    },
    Solve {
        params: Vec<PlanParameter>,
        config: SolveConfig,
    },
    /// The plan and each cumulative override step, already lowered: `steps[0]`
    /// is the plan itself, `steps[i]` the plan with the first `i` layers on.
    WhatIf {
        steps: Vec<SimulationConfig>,
        iterations: usize,
        parallel_batches: usize,
        seed: Option<u64>,
        /// Retirement age before and after the layers, where the plan names one.
        plan_retirement_age: Option<f64>,
        what_if_retirement_age: Option<f64>,
    },
}

impl AnalysisSpec {
    /// Simulations the analysis will run, for the progress denominator. An
    /// upper bound where the search may stop early, which is what a progress
    /// bar wants anyway.
    #[must_use]
    pub fn budget(&self) -> usize {
        match self {
            Self::Sweep { config, .. } => (config.total_points() + 1) * config.mc_iterations,
            Self::Sensitivity {
                params, iterations, ..
            } => (params.len() * 2 + 1) * iterations,
            Self::Solve { config, .. } => config.probe_budget() * config.mc_iterations,
            Self::WhatIf {
                steps, iterations, ..
            } => steps.len() * iterations,
        }
    }
}

/// An analysis checked, lowered and costed.
pub struct Prepared {
    /// The compiled plan every probe starts from.
    pub base: SimulationConfig,
    pub spec: AnalysisSpec,
    /// Whether it is a goal seek, which hosts that meter goal seeks count.
    pub is_solve: bool,
}

/// Check `body` against `graph`, lower it and cost it. Nothing is simulated.
///
/// Whether the caller may ask at all, and the iteration cap that applies to
/// them, are the host's to decide first (see [`Limits`]).
pub fn prepare(
    graph: &ScenarioGraph,
    body: CreateAnalysis,
    limits: &Limits,
) -> PlanResult<Prepared> {
    let cap = limits.iteration_cap;
    let parallel_batches = limits.parallel_batches;
    let is_solve = matches!(&body, CreateAnalysis::Solve { .. });
    let compiled = compile::compile(graph)?;
    let available = parameters(&compiled);
    // A what-if can be all shocks and one-offs, so it is the one analysis that
    // does not need a named parameter to vary.
    if available.is_empty() && !matches!(&body, CreateAnalysis::WhatIf { .. }) {
        return Err(PlanError::unprocessable(
            "this plan has no named parameters to analyse — add parameters on the Plan tab and reference them in amounts or schedules",
        ));
    }

    let spec = match body {
        CreateAnalysis::Sweep { axes, iterations } => {
            if axes.is_empty() || axes.len() > MAX_AXES {
                return Err(PlanError::invalid(format!(
                    "a sweep takes between one and {MAX_AXES} variables"
                )));
            }
            // More axes means fewer steps each: a fourth variable at the
            // two-axis default would be 1,296 cells, which is an hour nobody
            // asked for. The client sends its own step counts; this is only
            // what an omitted one falls back to.
            let default_steps = match axes.len() {
                1 | 2 => DEFAULT_STEPS,
                3 => 4,
                _ => 3,
            };
            let (params, sweeps) = resolve(&available, &axes, default_steps)?;
            let points = sweeps.iter().map(|s| s.step_count).product::<usize>();
            if points > MAX_SWEEP_POINTS {
                return Err(PlanError::invalid(format!(
                    "that is {points} combinations; a sweep evaluates at most \
                     {MAX_SWEEP_POINTS}. Drop a variable or cut its steps."
                )));
            }
            AnalysisSpec::Sweep {
                params,
                config: SweepConfig {
                    parameters: sweeps,
                    metrics: Vec::new(),
                    mc_iterations: iterations_or_default(iterations, 250, cap)?,
                    parallel_batches,
                    seed: Some(ANALYSIS_SEED),
                },
            }
        }

        CreateAnalysis::Sensitivity {
            parameter_ids,
            fraction,
            iterations,
        } => {
            let params = if parameter_ids.is_empty() {
                available
            } else {
                parameter_ids
                    .iter()
                    .map(|id| find(&available, id).cloned())
                    .collect::<PlanResult<Vec<_>>>()?
            };
            let fraction = fraction.unwrap_or(0.2);
            if !(0.01..=1.0).contains(&fraction) {
                return Err(PlanError::invalid("fraction must be between 0.01 and 1.0"));
            }
            AnalysisSpec::Sensitivity {
                params,
                fraction,
                // A ranking is two runs a parameter and is meant to be cheap,
                // so it defaults lighter than a sweep cell does.
                iterations: iterations_or_default(iterations, 200, cap)?,
                parallel_batches,
                seed: Some(ANALYSIS_SEED),
            }
        }

        CreateAnalysis::Solve {
            vary,
            objective,
            constraint,
            min_value,
            iterations,
        } => {
            if vary.is_empty() || vary.len() > MAX_VARIED {
                return Err(PlanError::invalid(format!(
                    "a solve varies between one and {MAX_VARIED} parameters"
                )));
            }
            if !(0.0..=1.0).contains(&min_value) {
                return Err(PlanError::invalid(
                    "the constraint threshold is a fraction between 0 and 1",
                ));
            }
            // Grid search is what more than one parameter falls back to, so the
            // resolution has to stay coarse enough to finish.
            let (params, sweeps) = resolve(&available, &vary, if vary.len() > 1 { 5 } else { 2 })?;
            AnalysisSpec::Solve {
                params,
                config: SolveConfig {
                    parameters: sweeps,
                    objective: objective.into(),
                    constraint: SolveConstraint {
                        metric: constraint
                            .unwrap_or(ConstraintRequest::FundingSuccessRate)
                            .into(),
                        min_value,
                    },
                    mc_iterations: iterations_or_default(iterations, 250, cap)?,
                    parallel_batches,
                    seed: Some(ANALYSIS_SEED),
                    ..SolveConfig::default()
                },
            }
        }

        CreateAnalysis::WhatIf { layers, iterations } => {
            let lowered = what_if::lower(graph, &compiled, &layers)?;
            let ceiling = cap.clamp(MIN_ITERATIONS, MAX_ANALYSIS_ITERATIONS);
            let total = iterations
                .unwrap_or(DEFAULT_WHAT_IF_ITERATIONS)
                .clamp(MIN_ITERATIONS, ceiling);
            let per_step = total
                .div_ceil(lowered.steps.len().max(1))
                .max(MIN_ITERATIONS);
            AnalysisSpec::WhatIf {
                steps: lowered.steps,
                iterations: per_step,
                parallel_batches,
                seed: Some(ANALYSIS_SEED),
                plan_retirement_age: lowered.plan_retirement_age,
                what_if_retirement_age: lowered.what_if_retirement_age,
            }
        }
    };

    // Cost includes all probes/cells and the full horizon; reject before quota use.
    if spec.budget().saturating_mul(compiled.config.duration_years) > MAX_COST {
        return Err(PlanError::invalid(
            "Analysis is too large. Reduce iterations, years, or varied parameters.",
        ));
    }
    Ok(Prepared {
        base: compiled.config,
        spec,
        is_solve,
    })
}

/// Turn requested axes into engine sweep parameters, defaulting the range and
/// the resolution from the plan where the request left them out.
pub fn resolve(
    available: &[PlanParameter],
    axes: &[AxisRequest],
    default_steps: usize,
) -> PlanResult<(Vec<PlanParameter>, Vec<SweepParameter>)> {
    let mut params = Vec::with_capacity(axes.len());
    let mut sweeps = Vec::with_capacity(axes.len());

    for axis in axes {
        let param = find(available, &axis.parameter_id)?;
        if params
            .iter()
            .any(|p: &PlanParameter| p.id == axis.parameter_id)
        {
            return Err(PlanError::invalid(format!(
                "{} is named twice; each axis needs its own parameter",
                axis.parameter_id
            )));
        }

        let min = axis.min.unwrap_or(param.min);
        let max = axis.max.unwrap_or(param.max);
        if !min.is_finite() || !max.is_finite() || max <= min {
            return Err(PlanError::invalid(format!(
                "{} needs a range with max above min",
                axis.parameter_id
            )));
        }
        let steps = axis.steps.unwrap_or(default_steps);
        if !(2..=MAX_STEPS).contains(&steps) {
            return Err(PlanError::invalid(format!(
                "steps must be between 2 and {MAX_STEPS}"
            )));
        }

        sweeps.push(param.sweep(min, max, steps)?);
        params.push(param.clone());
    }

    Ok((params, sweeps))
}

/// A parameter by its analysis id (`parameter:5`).
pub fn find<'a>(available: &'a [PlanParameter], id: &str) -> PlanResult<&'a PlanParameter> {
    available
        .iter()
        .find(|p| p.id == id)
        .ok_or(PlanError::NotFound("parameter"))
}

/// `requested` iterations, or `fallback` capped at the ceiling, checked
/// against `deployment_max` and the analysis floor and ceiling.
pub fn iterations_or_default(
    requested: Option<usize>,
    fallback: usize,
    deployment_max: usize,
) -> PlanResult<usize> {
    let ceiling = deployment_max.min(MAX_ANALYSIS_ITERATIONS);
    let iterations = requested.unwrap_or(fallback.min(ceiling));
    if !(MIN_ITERATIONS..=ceiling).contains(&iterations) {
        return Err(PlanError::invalid(format!(
            "iterations must be between {MIN_ITERATIONS} and {ceiling}"
        )));
    }
    Ok(iterations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_and_default_analysis_iterations_respect_deployment_ceiling() {
        assert_eq!(iterations_or_default(None, 250, 100).unwrap(), 100);
        assert_eq!(iterations_or_default(None, 200, 100).unwrap(), 100);
        assert_eq!(iterations_or_default(Some(75), 250, 100).unwrap(), 75);
        assert!(iterations_or_default(Some(101), 250, 100).is_err());
        assert!(iterations_or_default(None, 250, 24).is_err());
        assert!(iterations_or_default(Some(2001), 250, 50_000).is_err());
    }
}
