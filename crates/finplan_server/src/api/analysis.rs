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
use finplan_core::analysis::{SolveConfig, SolveConstraint, SolveConstraintMetric, SolveObjective};
use serde::Serialize;
use ts_rs::TS;

use crate::analysis::cache;
use crate::analysis::jobs::JobSpec;
use crate::analysis::params::{PlanParameter, parameters};
use crate::analysis::results::{AnalysisOutcome, AnalysisParameter, CachedSweep};
use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::observability::{JobKind as MetricKind, Origin, Tier};
use crate::runner::telemetry::Submission;
use crate::state::AppState;
use finplan_core::config::SimulationConfig;
use finplan_plan::analysis::Limits;
use finplan_plan::compile;
use finplan_plan::graph::ScenarioGraph;

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

#[cfg(test)]
use finplan_plan::analysis::iterations_or_default;
pub use finplan_plan::analysis::{
    ANALYSIS_SEED, AxisRequest, ConstraintRequest, CreateAnalysis, ObjectiveRequest,
};

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
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    Ok(Json(finplan_plan::analysis::discover(&graph)?))
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
        tier,
    } = prepare(state, user, scenario_id, body).await?;
    let admission =
        crate::billing::admit_compute_observed(&user.id, tier, &state.telemetry, Origin::Request)?;
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
    /// The caller's tier, which admission is counted under.
    pub tier: Tier,
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
    let cap = crate::billing::iteration_cap(&entitlements, &state.config);
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
    // The deployment's resource ceiling is request validation, independent of
    // paid access; below it, the account's own cap is a plan limit.
    if requested_iterations.is_some_and(|n| n > cap) {
        return Err(
            if requested_iterations.is_some_and(|n| n > state.config.max_iterations) {
                ApiError::bad_request(format!(
                    "iterations must not exceed {}",
                    state.config.max_iterations
                ))
            } else {
                ApiError::Forbidden(format!("Your plan allows at most {cap} iterations."))
            },
        );
    }
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    let lowered = finplan_plan::analysis::prepare(
        &graph,
        body,
        &Limits {
            iteration_cap: cap,
            parallel_batches: state.config.sim_workers.max(1),
        },
    )?;
    Ok(Prepared {
        base: lowered.base,
        spec: lowered.spec,
        is_solve,
        tier: entitlements.tier(&state.config),
    })
}

/// The label the caller's admission is counted under.
async fn caller_tier(state: &AppState, user: &CurrentUser) -> ApiResult<Tier> {
    Ok(
        crate::billing::entitlements(&state.db, &user.id, &state.config)
            .await?
            .tier(&state.config),
    )
}

// ── the AI tool ─────────────────────────────────────────────────────────────

/// Monte Carlo iterations behind each probe of a model's goal seek: fewer than
/// the analysis screen's 250, since the tool is charged to a preview budget.
const AI_GOAL_SEEK_ITERATIONS: usize = 150;
/// Bisection probes after the two ends and the baseline.
const AI_GOAL_SEEK_PROBES: usize = 10;
/// Most values a grid search over an age or a date evaluates.
const AI_GOAL_SEEK_GRID: usize = 16;

/// `goal_seek` for the review and drafting loops: search one named parameter
/// of `graph`'s plan for the value at which the metric reaches the target, as
/// the analysis screen's goal seek does, but on fewer iterations and without
/// spending the user's monthly goal-seek quota (a model's tool call is not the
/// user's request; the loop's own cap and preview budget bound it). Errors are
/// in words the model may read.
pub(crate) async fn ai_goal_seek(
    state: &AppState,
    user: &CurrentUser,
    graph: &ScenarioGraph,
    request: crate::suggest::ai::tools::goal_seek::GoalSeekRequest,
) -> ApiResult<serde_json::Value> {
    use crate::suggest::ai::tools::goal_seek::{Direction, Metric};
    use finplan_core::analysis::{SolveMethod, SolveProbe, SolveResults};
    use serde_json::json;

    let compiled = compile::compile(graph)?;
    let available = parameters(&compiled);
    if available.is_empty() {
        return Err(ApiError::unprocessable(
            "this plan has no named parameters to search; add one and reference it in an amount or trigger",
        ));
    }
    let param = lookup(&available, &request.parameter)?.clone();
    let min = request.min.unwrap_or(param.min);
    let max = request.max.unwrap_or(param.max);
    if !min.is_finite() || !max.is_finite() || max <= min {
        return Err(ApiError::bad_request(format!(
            "{} needs a range with max above min",
            param.name
        )));
    }
    // A grid search (ages and dates are discrete) evaluates every step, so it
    // is set to about a step a year across the range, capped.
    let steps = match param.kind {
        crate::analysis::params::ParamKind::Age => (max - min).ceil() as usize + 1,
        crate::analysis::params::ParamKind::Date => ((max - min) / 365.0).ceil() as usize + 1,
        _ => 2,
    }
    .clamp(2, AI_GOAL_SEEK_GRID);
    let sweep = param.sweep(min, max, steps)?;
    let direction =
        request
            .direction
            .unwrap_or(if param.kind == crate::analysis::params::ParamKind::Age {
                Direction::Smallest
            } else {
                Direction::Largest
            });
    let metric = match request.metric {
        Metric::SuccessRate => SolveConstraintMetric::SuccessRate,
        Metric::FundingSuccessRate => SolveConstraintMetric::FundingSuccessRate,
    };
    let config = SolveConfig {
        parameters: vec![sweep],
        objective: match direction {
            Direction::Smallest => SolveObjective::MinParameter,
            Direction::Largest => SolveObjective::MaxParameter,
        },
        constraint: SolveConstraint {
            metric,
            min_value: request.target,
        },
        mc_iterations: AI_GOAL_SEEK_ITERATIONS,
        parallel_batches: state.config.sim_workers.max(1),
        seed: Some(ANALYSIS_SEED),
        max_probes: AI_GOAL_SEEK_PROBES,
        ..SolveConfig::default()
    };
    if config
        .probe_budget()
        .saturating_mul(config.mc_iterations)
        .saturating_mul(compiled.config.duration_years)
        > 20_000_000
    {
        return Err(ApiError::bad_request(
            "this goal seek is too large for the plan's horizon",
        ));
    }

    let tier = caller_tier(state, user).await?;
    let permit =
        crate::billing::admit_compute_observed(&user.id, tier, &state.telemetry, Origin::Request)?;
    let progress = finplan_core::analysis::SweepProgress::new(0);
    let guard = CancelOnDrop(progress.clone());
    let base = compiled.config.clone();
    let solved: Result<SolveResults, _> = tokio::task::spawn_blocking({
        let (config, progress) = (config.clone(), progress.clone());
        move || {
            let _permit = permit;
            finplan_core::analysis::solve(&base, &config, Some(&progress))
        }
    })
    .await
    .map_err(|_| ApiError::internal("the goal seek panicked"))?;
    drop(guard);
    let mut results =
        solved.map_err(|e| ApiError::unprocessable(format!("the plan failed to simulate: {e}")))?;
    for probe in results.probes.iter_mut().chain(results.best.iter_mut()) {
        for value in &mut probe.values {
            *value = param.display_coordinate(&config.parameters[0], *value);
        }
    }

    let achieved = |probe: &SolveProbe| match metric {
        SolveConstraintMetric::SuccessRate => Some(probe.success_rate),
        SolveConstraintMetric::FundingSuccessRate => probe.funding_success_rate,
    };
    let point = |probe: &SolveProbe| {
        let value = probe.values.first().copied().unwrap_or(f64::NAN);
        json!({
            "value": value,
            "value_text": value_text(param.kind, value),
            "success_rate": probe.success_rate,
            "funding_success_rate": probe.funding_success_rate,
        })
    };
    let closest = results
        .probes
        .iter()
        .filter(|p| achieved(p).is_some())
        .max_by(|a, b| {
            achieved(a)
                .partial_cmp(&achieved(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    let mut out = json!({
        "parameter": {
            "id": param.id,
            "name": param.name,
            "kind": param.kind.as_str(),
            "unit": unit_of(param.kind),
            "current": param.current,
            "current_text": value_text(param.kind, param.current),
            "searched_range": [min, max],
        },
        "metric": request.metric.as_str(),
        "target": request.target,
        "direction": match direction {
            Direction::Smallest => "smallest",
            Direction::Largest => "largest",
        },
        "method": match results.method {
            SolveMethod::Bisection => "bisection",
            SolveMethod::GridSearch => "grid_search",
        },
        "plan_as_it_stands": {
            "success_rate": results.baseline.success_rate,
            "funding_success_rate": results.baseline.funding_success_rate,
        },
        "simulations": results.probes.len() + 1,
        "iterations_each": results.mc_iterations,
        "std_error": results.constraint_std_error(),
        "note": "Fixed-seed simulation of the plan as it stands; the rates carry sampling error (std_error). A bisection assumes the metric rises or falls steadily with the parameter.",
    });
    match &results.best {
        Some(best) => {
            out["found"] = json!(true);
            out["result"] = point(best);
            if results.method == SolveMethod::Bisection && results.probes.len() == 1 {
                out["at_range_edge"] = json!(true);
                out["note"] = json!(
                    "The target already holds at the edge of the searched range, so the range is what binds; widen min or max to look further."
                );
            }
        }
        None => {
            out["found"] = json!(false);
            if let Some(closest) = closest {
                out["closest"] = point(closest);
            }
            out["note"] = json!(
                "No value in the searched range reaches the target; `closest` is the best value tried. Widen the range or change another parameter."
            );
        }
    }
    Ok(out)
}

/// Monte Carlo iterations behind each simulation of a model's sensitivity
/// ranking: the goal seek's, for the same reason.
const AI_SENSITIVITY_ITERATIONS: usize = 150;

/// `sensitivity` for the review loop: rank `graph`'s named parameters by how
/// far moving each down and up moves the metric, as the analysis screen's
/// ranking does, on fewer iterations. A refusal is anything turned away before
/// a simulation started; the loop charges only for the rest. Messages are in
/// words the model may read.
pub(crate) async fn ai_sensitivity(
    state: &AppState,
    user: &CurrentUser,
    graph: &ScenarioGraph,
    request: crate::suggest::ai::tools::sensitivity::SensitivityRequest,
) -> Result<serde_json::Value, crate::suggest::ai::tools::sensitivity::SensitivityError> {
    use crate::analysis::jobs::{InlineFailure, run_inline};
    use crate::suggest::ai::tools::goal_seek::Metric;
    use crate::suggest::ai::tools::sensitivity::{MAX_PARAMETERS, SensitivityError};
    use serde_json::json;

    let refused = |error: ApiError| match error {
        ApiError::Database(_) | ApiError::Internal(_) => {
            tracing::warn!(event = "review_ai.sensitivity_failed", error = %error);
            SensitivityError::Refused(
                "the sensitivity ranking could not run; use the results you have or stop".into(),
            )
        }
        other => SensitivityError::Refused(other.to_string()),
    };
    let compiled = compile::compile(graph).map_err(|error| refused(error.into()))?;
    let available = parameters(&compiled);
    if available.is_empty() {
        return Err(SensitivityError::Refused(
            "this plan has no named parameters to move; add one and reference it in an amount or trigger".into(),
        ));
    }
    let params: Vec<PlanParameter> = if request.parameters.is_empty() {
        if available.len() > MAX_PARAMETERS {
            return Err(SensitivityError::Refused(format!(
                "this plan has {} parameters and one ranking moves at most {MAX_PARAMETERS}; name the ones to try: {}",
                available.len(),
                available
                    .iter()
                    .map(|p| format!("{} ({})", p.name, p.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        available.clone()
    } else {
        let mut chosen: Vec<PlanParameter> = Vec::new();
        for wanted in &request.parameters {
            let found = lookup(&available, wanted).map_err(refused)?;
            if !chosen.iter().any(|p| p.id == found.id) {
                chosen.push(found.clone());
            }
        }
        chosen
    };
    let fraction = request.fraction.unwrap_or(0.2);
    let metric = request.metric.unwrap_or(Metric::FundingSuccessRate);
    let spec = JobSpec::Sensitivity {
        params: params.clone(),
        fraction,
        iterations: AI_SENSITIVITY_ITERATIONS,
        parallel_batches: state.config.sim_workers.max(1),
        seed: Some(ANALYSIS_SEED),
    };
    if spec.budget().saturating_mul(compiled.config.duration_years) > 20_000_000 {
        return Err(SensitivityError::Refused(
            "this ranking is too large for the plan's horizon; name fewer parameters".into(),
        ));
    }

    let tier = caller_tier(state, user).await.map_err(refused)?;
    let permit =
        crate::billing::admit_compute_observed(&user.id, tier, &state.telemetry, Origin::Request)
            .map_err(refused)?;
    let progress = finplan_core::analysis::SweepProgress::new(spec.budget());
    let guard = CancelOnDrop(progress.clone());
    let base = compiled.config;
    let outcome = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        run_inline(&base, &spec, &progress)
    })
    .await;
    drop(guard);
    let results = match outcome {
        Ok(Ok(AnalysisOutcome::Sensitivity(results))) => results,
        Ok(Err(InlineFailure::Cancelled)) => {
            return Err(SensitivityError::Failed("the ranking was canceled".into()));
        }
        Ok(Ok(_)) | Ok(Err(InlineFailure::Failed)) | Err(_) => {
            tracing::warn!(
                event = "review_ai.sensitivity_failed",
                error = "simulation failed"
            );
            return Err(SensitivityError::Failed(
                "the plan failed to simulate; use the results you have or stop".into(),
            ));
        }
    };

    let rate = |point: &crate::analysis::results::AnalysisPoint| match metric {
        Metric::SuccessRate => point.success_rate,
        Metric::FundingSuccessRate => point.funding_success_rate.unwrap_or(point.success_rate),
    };
    let mut rows = results.rows;
    // The screen ranks by success rate; the model asked for its own metric.
    rows.sort_by(|a, b| {
        let span =
            |r: &crate::analysis::results::SensitivityRow| (rate(&r.high) - rate(&r.low)).abs();
        span(b).total_cmp(&span(a))
    });
    let ranking: Vec<serde_json::Value> = rows
        .iter()
        .filter_map(|row| {
            let param = params.iter().find(|p| p.id == row.parameter_id)?;
            let point = |value: f64, at: &crate::analysis::results::AnalysisPoint| {
                json!({
                    "value": value,
                    "value_text": value_text(param.kind, value),
                    "success_rate": at.success_rate,
                    "funding_success_rate": at.funding_success_rate,
                })
            };
            Some(json!({
                "parameter": {
                    "id": param.id,
                    "name": param.name,
                    "kind": param.kind.as_str(),
                    "unit": unit_of(param.kind),
                    "current": param.current,
                    "current_text": value_text(param.kind, param.current),
                },
                "low": point(row.low_value, &row.low),
                "high": point(row.high_value, &row.high),
                "span_points": ((rate(&row.high) - rate(&row.low)).abs() * 1000.0).round() / 10.0,
            }))
        })
        .collect();
    let skipped: Vec<&str> = params
        .iter()
        .filter(|p| !rows.iter().any(|r| r.parameter_id == p.id))
        .map(|p| p.name.as_str())
        .collect();
    Ok(json!({
        "metric": metric.as_str(),
        "fraction": fraction,
        "plan_as_it_stands": {
            "success_rate": results.plan.success_rate,
            "funding_success_rate": results.plan.funding_success_rate,
        },
        "ranking": ranking,
        "skipped": skipped,
        "simulations": rows.len() * 2 + 1,
        "iterations_each": results.iterations,
        "note": "Each parameter moved on its own, everything else as the plan stands, all on the same fixed draws, so the differences are paired. span_points is how far the metric moved between low and high, in percentage points; a span of a point or less is within sampling error. Amounts and rates moved by the fraction, ages by 5 years, dates by 1 year, so the ranking compares those moves, not equally likely ones. `skipped` had no room to move within its range.",
    }))
}

/// A parameter by id (`parameter:5` or `5`) or by name (exact, ignoring case,
/// else the one name that contains the text).
fn lookup<'a>(available: &'a [PlanParameter], wanted: &str) -> ApiResult<&'a PlanParameter> {
    let wanted = wanted.trim();
    let bare = wanted.strip_prefix("parameter:").unwrap_or(wanted);
    if let Some(found) = available
        .iter()
        .find(|p| p.id == wanted || p.parameter_id.to_string() == bare)
    {
        return Ok(found);
    }
    let lower = wanted.to_lowercase();
    if let Some(found) = available.iter().find(|p| p.name.to_lowercase() == lower) {
        return Ok(found);
    }
    let partial: Vec<&PlanParameter> = available
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&lower))
        .collect();
    match partial.as_slice() {
        [one] if !lower.is_empty() => Ok(one),
        _ => Err(ApiError::bad_request(format!(
            "there is no parameter `{wanted}`; the plan's parameters are: {}",
            available
                .iter()
                .map(|p| format!("{} ({})", p.name, p.id))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn unit_of(kind: crate::analysis::params::ParamKind) -> &'static str {
    use crate::analysis::params::ParamKind;
    match kind {
        ParamKind::Age => "years of age",
        ParamKind::Amount => "dollars",
        ParamKind::Rate => "fraction (0.04 is 4%)",
        ParamKind::Date => "days since 1970-01-01",
    }
}

/// A parameter value in words: an age as years and months, a date as ISO.
fn value_text(kind: crate::analysis::params::ParamKind, value: f64) -> String {
    use crate::analysis::params::ParamKind;
    match kind {
        ParamKind::Age => {
            let months = (value * 12.0).round() as i64;
            match (months / 12, months % 12) {
                (years, 0) => format!("age {years}"),
                (years, months) => format!("age {years} and {months} months"),
            }
        }
        ParamKind::Date => jiff::civil::Date::constant(1970, 1, 1)
            .checked_add(jiff::Span::new().days(value.round() as i64))
            .map_or_else(|_| value.to_string(), |d| d.to_string()),
        ParamKind::Rate => format!("{:.2}%", value * 100.0),
        ParamKind::Amount => format!("${value:.0}"),
    }
}

/// Cancels the goal seek when its request is dropped.
struct CancelOnDrop(finplan_core::analysis::SweepProgress);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
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
