//! Sweeps, sensitivity rankings and goal seeks.
//!
//! Three questions, one job type. They differ in what they compute and not in
//! how they are driven — POST to start, GET to poll, GET results when it is
//! done, POST to cancel — so they share a route family and the client polls one
//! endpoint whatever it asked for.
//!
//! Requests name parameters by the ids `GET /scenarios/{id}/analysis/parameters` hands
//! out. Nothing here takes an event id and a target from the client: what a
//! plan can vary is derived from the compiled plan, so a request can only ask
//! for something the engine can actually do.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use finplan_core::analysis::{
    SolveConfig, SolveConstraint, SolveConstraintMetric, SolveObjective, SweepConfig,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::analysis::cache;
use crate::analysis::jobs::JobSpec;
use crate::analysis::params::{PlanParameter, parameters};
use crate::analysis::results::{AnalysisOutcome, AnalysisParameter, CachedSweep};
use crate::auth::session::CurrentUser;
use crate::compile::{self, rows::ScenarioGraph};
use crate::error::{ApiError, ApiResult};
use crate::observability::{JobKind as MetricKind, Origin};
use crate::runner::telemetry::Submission;
use crate::state::AppState;
use finplan_core::config::SimulationConfig;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/scenarios/{scenario_id}/analysis/parameters",
            get(list_parameters),
        )
        .route("/scenarios/{scenario_id}/analyses", post(create))
        .route("/scenarios/{scenario_id}/analyses/sweep", get(cached_sweep))
        .route(
            "/scenarios/{scenario_id}/analyses/sweep/layout",
            put(save_layout),
        )
        .route("/analyses/{id}", get(fetch))
        .route("/analyses/{id}/cancel", post(cancel))
        .route("/analyses/{id}/results", get(results))
}

/// Bounds on what a single request may ask for, so one browser tab cannot
/// queue an hour of CPU.
const MAX_STEPS: usize = 12;
const MAX_AXES: usize = 4;
const MAX_VARIED: usize = 3;

/// The real ceiling on a sweep: how many combinations it may evaluate.
///
/// Counting axes is the wrong limit once a sweep can carry more than two of
/// them — four axes of three steps is 81 cells and finishes, two axes of twelve
/// is 144 and is the grid the old two-axis ceiling allowed. What costs time is
/// the product, so that is what is capped, and at the analysis default of 250
/// iterations it holds a request to 128,000 simulations.
const MAX_SWEEP_POINTS: usize = 512;
const MAX_ANALYSIS_ITERATIONS: usize = 2_000;
const MIN_ITERATIONS: usize = 25;
/// Monte Carlo iterations behind a whole what-if when the request names none.
///
/// A budget for the stack, not per step: it is split evenly across the plan and
/// each layer, so adding an override does not make every nudge slower. Every
/// step runs on the same seed, which keeps the step-to-step differences stable
/// even at a few dozen iterations each.
const DEFAULT_WHAT_IF_ITERATIONS: usize = 500;

/// The default grid resolution: six steps an axis, which is what a heatmap can
/// label without crowding.
const DEFAULT_STEPS: usize = 6;

/// Every analysis is seeded, and with the same seed.
///
/// Two cells of a grid differ by the parameter or they differ by nothing;
/// letting them draw different market paths would put noise into exactly the
/// comparison the screen exists to make. It also means a re-run of an unchanged
/// question returns the same answer, which is the behaviour anyone comparing
/// two screenshots expects.
const ANALYSIS_SEED: u64 = 0x5EED;

/// One axis of a requested sweep, or one parameter a solve may vary.
#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct AxisRequest {
    /// An id from `GET /scenarios/{id}/analysis/parameters`.
    pub parameter_id: String,
    /// Range to cover. Omitted, the parameter's own suggested range is used.
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// Points along the axis. Ignored by a bisecting solve, which chooses its
    /// own probes.
    #[serde(default)]
    pub steps: Option<usize>,
}

/// What to optimise for. Named rather than free-form: a client cannot ask for
/// an objective the solver has no way to evaluate.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ObjectiveRequest {
    /// The largest value of the varied parameter that still clears the
    /// constraint — a maximum sustainable withdrawal.
    MaxParameter,
    /// The smallest such value — an earliest retirement age.
    MinParameter,
    /// The highest median terminal net worth.
    MaxMedianNetWorth,
    /// The highest 5th-percentile terminal net worth: the best floor.
    MaxFloorNetWorth,
}

impl From<ObjectiveRequest> for SolveObjective {
    fn from(value: ObjectiveRequest) -> Self {
        match value {
            ObjectiveRequest::MaxParameter => Self::MaxParameter,
            ObjectiveRequest::MinParameter => Self::MinParameter,
            ObjectiveRequest::MaxMedianNetWorth => Self::MaxMedianNetWorth,
            ObjectiveRequest::MaxFloorNetWorth => Self::MaxFloorNetWorth,
        }
    }
}

/// The outcome measure a solve's constraint is written against.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ConstraintRequest {
    SuccessRate,
    FundingSuccessRate,
}

impl From<ConstraintRequest> for SolveConstraintMetric {
    fn from(value: ConstraintRequest) -> Self {
        match value {
            ConstraintRequest::SuccessRate => Self::SuccessRate,
            ConstraintRequest::FundingSuccessRate => Self::FundingSuccessRate,
        }
    }
}

/// The analysis to run. `kind` selects which of the three, and the fields that
/// do not apply to it are ignored.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[ts(export, optional_fields = nullable)]
pub enum CreateAnalysis {
    /// A grid over one to four parameters, capped by the product of the steps
    /// rather than by the count: the client lays graphs out over the result and
    /// each picks its own one or two axes from it.
    Sweep {
        axes: Vec<AxisRequest>,
        #[serde(default)]
        iterations: Option<usize>,
    },
    /// Every parameter moved on its own, ranked by what it did.
    Sensitivity {
        /// Which parameters to rank. Empty means all of them.
        #[serde(default)]
        parameter_ids: Vec<String>,
        /// Band width as a fraction of each parameter's value: `0.2` for ±20%.
        #[serde(default)]
        fraction: Option<f64>,
        #[serde(default)]
        iterations: Option<usize>,
    },
    /// The best values of one to three parameters, subject to a constraint.
    Solve {
        vary: Vec<AxisRequest>,
        objective: ObjectiveRequest,
        #[serde(default)]
        constraint: Option<ConstraintRequest>,
        /// The floor, as a fraction: `0.95` for "success ≥ 95%".
        min_value: f64,
        #[serde(default)]
        iterations: Option<usize>,
    },
    /// The plan with an ordered stack of overrides applied cumulatively: one
    /// step for the plan and one more per layer.
    WhatIf {
        /// The enabled layers only, in order. At most eight.
        layers: Vec<crate::api::what_if::WhatIfLayer>,
        /// Simulations for the whole stack, split evenly across its steps
        /// (each gets at least the analysis minimum).
        #[serde(default)]
        iterations: Option<usize>,
    },
}

/// A queued or finished analysis.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Analysis {
    pub id: i64,
    pub scenario_id: i64,
    /// `"sweep"`, `"sensitivity"`, `"solve"` or `"what-if"`.
    pub kind: String,
    /// `"queued"`, `"running"`, `"succeeded"`, `"failed"` or `"canceled"`.
    pub status: String,
    /// Simulations finished, against the number budgeted for.
    pub completed: i64,
    pub total: i64,
    pub error_message: Option<String>,
    /// Wall-clock time once it has finished, for the run footer.
    pub elapsed_ms: Option<i64>,
}

impl From<crate::analysis::jobs::JobView> for Analysis {
    fn from(view: crate::analysis::jobs::JobView) -> Self {
        Self {
            id: view.id,
            scenario_id: view.scenario_id,
            kind: view.kind.as_str().to_string(),
            status: view.status.as_str().to_string(),
            completed: view.completed as i64,
            total: view.total as i64,
            error_message: view.error,
            elapsed_ms: view.elapsed_ms.map(|ms| ms as i64),
        }
    }
}

/// Everything this plan could vary, with the range each axis defaults to.
async fn list_parameters(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Vec<AnalysisParameter>>> {
    let (_, params) = plan(&state, scenario_id, &user.id).await?;
    Ok(Json(params.iter().map(Into::into).collect()))
}

/// Compile the scenario and read its parameters, so both are one call.
async fn plan(
    state: &AppState,
    scenario_id: i64,
    user_id: &str,
) -> ApiResult<(compile::CompiledScenario, Vec<PlanParameter>)> {
    let graph = ScenarioGraph::load(&state.db, scenario_id, user_id).await?;
    let compiled = compile::compile(&graph)?;
    let params = parameters(&compiled);
    Ok((compiled, params))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<CreateAnalysis>,
) -> ApiResult<(StatusCode, Json<Analysis>)> {
    let kind = match &body {
        CreateAnalysis::Sweep { .. } => MetricKind::Sweep,
        CreateAnalysis::Sensitivity { .. } => MetricKind::Sensitivity,
        CreateAnalysis::Solve { .. } => MetricKind::Solve,
        CreateAnalysis::WhatIf { .. } => MetricKind::WhatIf,
    };
    let mut decision = Submission::new(&state.telemetry, kind);
    let result = create_analysis(&state, &user, scenario_id, body, &mut decision).await;
    decision.result(&result);
    result
}

async fn create_analysis(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    body: CreateAnalysis,
    decision: &mut Submission,
) -> ApiResult<(StatusCode, Json<Analysis>)> {
    let Prepared {
        base,
        spec,
        is_solve,
    } = prepare(state, user, scenario_id, body).await?;
    let admission =
        crate::billing::admit_compute_observed(&user.id, &state.telemetry, Origin::Request)?;
    if is_solve {
        crate::billing::reserve_goal_seek(&state.db, &user.id, &state.config).await?;
    }
    let handle = state
        .analyses
        .start(scenario_id, &user.id, base, spec, admission);
    decision.accepted();
    let view = state.analyses.view(handle.id, &user.id)?;
    Ok((StatusCode::ACCEPTED, Json(view.into())))
}

/// An analysis checked, lowered and costed, but not yet admitted or started.
pub(crate) struct Prepared {
    pub base: SimulationConfig,
    pub spec: JobSpec,
    is_solve: bool,
}

/// Everything short of admission: access, iteration bounds, compiling the
/// plan and lowering the request. Shared by the job route and the
/// synchronous quick what-if, so the two cannot drift on what they accept.
pub(crate) async fn prepare(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    body: CreateAnalysis,
) -> ApiResult<Prepared> {
    let entitlements = crate::billing::entitlements(&state.db, &user.id, &state.config).await?;
    let is_solve = matches!(&body, CreateAnalysis::Solve { .. });
    if !is_solve {
        crate::billing::require_pro(&state.db, &user.id, &state.config).await?;
    }
    // A what-if's `iterations` is a budget for the whole stack, clamped to the
    // plan's ceiling below rather than refused: the screen asks for a fixed
    // refinement size and should get the most the account allows.
    let requested_iterations = match &body {
        CreateAnalysis::Sweep { iterations, .. }
        | CreateAnalysis::Sensitivity { iterations, .. }
        | CreateAnalysis::Solve { iterations, .. } => *iterations,
        CreateAnalysis::WhatIf { .. } => None,
    };
    // A deployment resource ceiling is request validation, independent of paid access.
    if requested_iterations.is_some_and(|n| n > state.config.max_iterations) {
        return Err(ApiError::bad_request(format!(
            "iterations must not exceed {}",
            state.config.max_iterations
        )));
    }
    if requested_iterations.is_some_and(|n| n > entitlements.max_iterations) {
        return Err(ApiError::Forbidden(format!(
            "Your plan allows at most {} iterations.",
            entitlements.max_iterations
        )));
    }
    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    let compiled = compile::compile(&graph)?;
    let available = parameters(&compiled);
    // A what-if can be all shocks and one-offs, so it is the one analysis that
    // does not need a named parameter to vary.
    if available.is_empty() && !matches!(&body, CreateAnalysis::WhatIf { .. }) {
        return Err(ApiError::unprocessable(
            "this plan has no named parameters to analyse — add parameters on the Plan tab and reference them in amounts or schedules",
        ));
    }

    let parallel_batches = state.config.sim_workers.max(1);
    let spec = match body {
        CreateAnalysis::Sweep { axes, iterations } => {
            if axes.is_empty() || axes.len() > MAX_AXES {
                return Err(ApiError::bad_request(format!(
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
                return Err(ApiError::bad_request(format!(
                    "that is {points} combinations; a sweep evaluates at most \
                     {MAX_SWEEP_POINTS}. Drop a variable or cut its steps."
                )));
            }
            JobSpec::Sweep {
                params,
                config: SweepConfig {
                    parameters: sweeps,
                    metrics: Vec::new(),
                    mc_iterations: iterations_or_default(
                        iterations,
                        250,
                        entitlements.max_iterations,
                    )?,
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
                    .collect::<ApiResult<Vec<_>>>()?
            };
            let fraction = fraction.unwrap_or(0.2);
            if !(0.01..=1.0).contains(&fraction) {
                return Err(ApiError::bad_request(
                    "fraction must be between 0.01 and 1.0",
                ));
            }
            JobSpec::Sensitivity {
                params,
                fraction,
                // A ranking is two runs a parameter and is meant to be cheap,
                // so it defaults lighter than a sweep cell does.
                iterations: iterations_or_default(iterations, 200, entitlements.max_iterations)?,
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
                return Err(ApiError::bad_request(format!(
                    "a solve varies between one and {MAX_VARIED} parameters"
                )));
            }
            if !(0.0..=1.0).contains(&min_value) {
                return Err(ApiError::bad_request(
                    "the constraint threshold is a fraction between 0 and 1",
                ));
            }
            // Grid search is what more than one parameter falls back to, so the
            // resolution has to stay coarse enough to finish.
            let (params, sweeps) = resolve(&available, &vary, if vary.len() > 1 { 5 } else { 2 })?;
            JobSpec::Solve {
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
                    mc_iterations: iterations_or_default(
                        iterations,
                        250,
                        entitlements.max_iterations,
                    )?,
                    parallel_batches,
                    seed: Some(ANALYSIS_SEED),
                    ..SolveConfig::default()
                },
            }
        }

        CreateAnalysis::WhatIf { layers, iterations } => {
            let lowered = crate::api::what_if::lower(&graph, &compiled, &layers)?;
            let ceiling = entitlements
                .max_iterations
                .clamp(MIN_ITERATIONS, MAX_ANALYSIS_ITERATIONS);
            let total = iterations
                .unwrap_or(DEFAULT_WHAT_IF_ITERATIONS)
                .clamp(MIN_ITERATIONS, ceiling);
            let per_step = total
                .div_ceil(lowered.steps.len().max(1))
                .max(MIN_ITERATIONS);
            JobSpec::WhatIf {
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
    if spec.budget().saturating_mul(compiled.config.duration_years) > 20_000_000 {
        return Err(ApiError::bad_request(
            "Analysis is too large. Reduce iterations, years, or varied parameters.",
        ));
    }
    Ok(Prepared {
        base: compiled.config,
        spec,
        is_solve,
    })
}

/// The scenario's most recent sweep, or `null` if it has never been swept.
///
/// Jobs are held in memory and their ids do not survive a restart, so this is
/// keyed by scenario rather than by job: the client asks what the plan was last
/// swept over, not what some id it used to hold produced.
async fn cached_sweep(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Option<CachedSweep>>> {
    crate::api::owned_scenario(&state.db, scenario_id, &user.id).await?;
    Ok(Json(
        cache::load(&state.db, scenario_id, &user.id, &state.telemetry).await?,
    ))
}

/// The biggest layout the server will hold.
///
/// A workspace is a handful of cards and each is a few hundred bytes; a
/// megabyte of it is a client gone wrong, not a screen anyone arranged.
const MAX_LAYOUT_BYTES: usize = 64 * 1024;

/// Store how the Analysis screen's graphs are arranged over this scenario's
/// sweep.
///
/// The body is the client's own graph specs, kept opaque: what a card draws and
/// how is the client's business from end to end, and a typed contract here
/// would mean a server deploy to add a chart kind. The only checks are the ones
/// storage genuinely needs — it must be an array, and it must be small.
async fn save_layout(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<StatusCode> {
    crate::api::owned_scenario(&state.db, scenario_id, &user.id).await?;
    if !body.is_array() {
        return Err(ApiError::bad_request("a layout is an array of graphs"));
    }
    if body.to_string().len() > MAX_LAYOUT_BYTES {
        return Err(ApiError::bad_request(format!(
            "a layout must be under {} KB",
            MAX_LAYOUT_BYTES / 1024
        )));
    }
    cache::save_layout(&state.db, scenario_id, &user.id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Analysis>> {
    Ok(Json(state.analyses.view(id, &user.id)?.into()))
}

async fn cancel(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Analysis>> {
    Ok(Json(state.analyses.cancel(id, &user.id)?.into()))
}

async fn results(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<AnalysisOutcome>> {
    Ok(Json(state.analyses.outcome(id, &user.id)?))
}

/// Turn requested axes into engine sweep parameters, defaulting the range and
/// the resolution from the plan where the request left them out.
fn resolve(
    available: &[PlanParameter],
    axes: &[AxisRequest],
    default_steps: usize,
) -> ApiResult<(
    Vec<PlanParameter>,
    Vec<finplan_core::analysis::SweepParameter>,
)> {
    let mut params = Vec::with_capacity(axes.len());
    let mut sweeps = Vec::with_capacity(axes.len());

    for axis in axes {
        let param = find(available, &axis.parameter_id)?;
        if params
            .iter()
            .any(|p: &PlanParameter| p.id == axis.parameter_id)
        {
            return Err(ApiError::bad_request(format!(
                "{} is named twice; each axis needs its own parameter",
                axis.parameter_id
            )));
        }

        let min = axis.min.unwrap_or(param.min);
        let max = axis.max.unwrap_or(param.max);
        if !min.is_finite() || !max.is_finite() || max <= min {
            return Err(ApiError::bad_request(format!(
                "{} needs a range with max above min",
                axis.parameter_id
            )));
        }
        let steps = axis.steps.unwrap_or(default_steps);
        if !(2..=MAX_STEPS).contains(&steps) {
            return Err(ApiError::bad_request(format!(
                "steps must be between 2 and {MAX_STEPS}"
            )));
        }

        sweeps.push(param.sweep(min, max, steps)?);
        params.push(param.clone());
    }

    Ok((params, sweeps))
}

fn find<'a>(available: &'a [PlanParameter], id: &str) -> ApiResult<&'a PlanParameter> {
    available
        .iter()
        .find(|p| p.id == id)
        .ok_or(ApiError::NotFound("parameter"))
}

fn iterations_or_default(
    requested: Option<usize>,
    fallback: usize,
    deployment_max: usize,
) -> ApiResult<usize> {
    let ceiling = deployment_max.min(MAX_ANALYSIS_ITERATIONS);
    let iterations = requested.unwrap_or(fallback.min(ceiling));
    if !(MIN_ITERATIONS..=ceiling).contains(&iterations) {
        return Err(ApiError::bad_request(format!(
            "iterations must be between {MIN_ITERATIONS} and {ceiling}"
        )));
    }
    Ok(iterations)
}

#[cfg(test)]
mod iteration_limit_tests {
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
