//! Server offload (spec 19): run a local plan on FinPlan's servers.
//!
//! A local plan lives in the browser; when the device is too slow for a run,
//! the browser may send the plan's snapshot here. The plan leaves the device
//! for this, so it is an explicit, per-run action, and the server keeps none of
//! it: the snapshot is compiled in the request, handed to a runner worker in
//! memory, and dropped. No scenario and no `run_*` row is written. What is kept,
//! for an hour after the job ends, is the finished `RunResults` JSON, which the
//! browser stores as a local run with `engine: "server"`.
//!
//! Job ids are the `compute_jobs` row ids (integers, never reused). They are
//! not secrets: every route is owner-only and answers 404 for anyone else's.

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use finplan_plan::compile;
use finplan_plan::graph::ScenarioGraph;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use ts_rs::TS;

use super::runs::{default_batch, default_parallel, run_cost, validate_run};
use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::observability::{JobKind, Origin};
use crate::runner::compute::ComputeJob;
use crate::runner::telemetry::Submission;
use crate::state::AppState;

/// The largest request body: a plan is a few hundred kilobytes at most.
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/compute/runs", post(create))
        .route("/compute/runs/{id}", get(fetch).delete(destroy))
        .route("/compute/budget", get(budget))
}

/// The run subset of `CreateRun`: what a browser decides about a run. Batch
/// shape is the server's choice, the defaults a stored run uses, so the same
/// graph and seed give the same results either way.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ComputeRunSettings {
    #[serde(default = "super::runs::default_iterations")]
    pub iterations: i64,
    #[serde(default = "super::runs::default_percentiles")]
    pub percentiles: Vec<f64>,
    /// Random when absent. The one used comes back on the job, so the run can
    /// be reproduced.
    #[serde(default)]
    pub seed: Option<i64>,
    /// Keep sampling until the median settles; `iterations` is then the
    /// minimum sample to take first.
    #[serde(default)]
    pub converge: bool,
}

impl Default for ComputeRunSettings {
    fn default() -> Self {
        Self {
            iterations: super::runs::default_iterations(),
            percentiles: super::runs::default_percentiles(),
            seed: None,
            converge: false,
        }
    }
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ComputeRunRequest {
    /// `finplan_plan::snapshot::snapshot` output: a `ScenarioGraph`, as a JSON
    /// object or as the JSON text the function returns.
    #[ts(type = "unknown")]
    pub snapshot: serde_json::Value,
    /// The engine version the snapshot was made for; must equal the server's.
    pub model_version: String,
    #[serde(default)]
    pub settings: ComputeRunSettings,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ComputeRunCreated {
    pub id: i64,
}

#[derive(Debug, Serialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ComputeRun {
    pub id: i64,
    #[ts(type = "\"queued\" | \"running\" | \"succeeded\" | \"failed\" | \"canceled\"")]
    pub status: String,
    /// Iterations finished so far; the final count once it has succeeded.
    pub completed_iterations: i64,
    /// What progress reads against: the count asked for, or a converging
    /// run's ceiling.
    pub iterations: i64,
    /// The seed the run used, the requested one or the one drawn.
    pub seed: i64,
    pub error: Option<String>,
    /// When the results are purged (UTC, `YYYY-MM-DD HH:MM:SS`); set once the
    /// job has ended.
    pub expires_at: Option<String>,
    /// The `RunResults` projection, only once the job has succeeded.
    #[ts(type = "unknown")]
    pub results: Option<Box<RawValue>>,
}

/// What a user may offload this month.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ComputeBudget {
    /// Cost units the tier gets each calendar month (UTC); 0 when offload is
    /// not available to this caller.
    pub monthly: i64,
    pub used: i64,
    pub remaining: i64,
    /// When the budget resets: the first instant of next month, UTC (RFC 3339).
    pub resets_at: String,
    /// The caller may offload a run now.
    pub available: bool,
    /// Why not: a guest has no account to charge; otherwise the month's
    /// budget is spent.
    #[ts(type = "\"guest\" | \"budget_spent\" | null")]
    pub unavailable_reason: Option<&'static str>,
}

async fn budget(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<ComputeBudget>> {
    let entitlements = crate::billing::entitlements(&state.db, &user.id, &state.config).await?;
    let monthly = crate::offload::monthly_budget(&entitlements, &state.config);
    let standing = crate::offload::standing(&state.db, &user.id).await?;
    let remaining = (monthly - standing.used).max(0);
    let unavailable_reason = if entitlements.guest {
        Some("guest")
    } else if remaining == 0 {
        Some("budget_spent")
    } else {
        None
    };
    Ok(Json(ComputeBudget {
        monthly,
        used: standing.used,
        remaining,
        resets_at: standing.resets_at,
        available: unavailable_reason.is_none(),
        unavailable_reason,
    }))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    request: Request,
) -> ApiResult<(StatusCode, Json<ComputeRunCreated>)> {
    let mut decision = Submission::new(&state.telemetry, JobKind::Offload);
    let result = submit(&state, &user, request, &mut decision).await;
    decision.result(&result);
    result
}

async fn submit(
    state: &AppState,
    user: &CurrentUser,
    request: Request,
    decision: &mut Submission,
) -> ApiResult<(StatusCode, Json<ComputeRunCreated>)> {
    if user.guest {
        return Err(ApiError::Forbidden(
            "Running on FinPlan's servers needs an account. Create a free account to use it."
                .into(),
        ));
    }

    // The cap is enforced while reading, so an oversized body is never held.
    let bytes = axum::body::to_bytes(request.into_body(), MAX_BODY_BYTES)
        .await
        .map_err(|error| {
            if error.to_string().contains("length limit") {
                ApiError::PayloadTooLarge(format!(
                    "The plan is larger than the {} MB limit for server runs.",
                    MAX_BODY_BYTES / (1024 * 1024)
                ))
            } else {
                ApiError::bad_request("could not read the request body")
            }
        })?;
    // Parse errors name a position, never the text: the body is a plan.
    let body: ComputeRunRequest = serde_json::from_slice(&bytes).map_err(|error| {
        ApiError::bad_request(format!(
            "request body is not the expected JSON (line {}, column {})",
            error.line(),
            error.column()
        ))
    })?;
    drop(bytes);

    if body.model_version != finplan_plan::snapshot::MODEL_VERSION {
        return Err(ApiError::Conflict("Reload FinPlan to update".into()));
    }

    let entitlements = crate::billing::entitlements(&state.db, &user.id, &state.config).await?;
    let tier = entitlements.tier(&state.config);
    // The budget, not a tier iteration cap, bounds offload: it exists for runs
    // too big for the device. The server-wide ceiling still applies.
    let settings = body.settings;
    let run = validate_run(
        settings.iterations,
        settings.converge,
        &settings.percentiles,
        default_batch(),
        default_parallel(),
        state.config.max_iterations,
    )?;

    let graph = parse_snapshot(body.snapshot)?;
    if graph.scenario.duration_years < 1 {
        return Err(ApiError::bad_request("duration must be at least 1 year"));
    }
    let compiled = compile::compile(&graph)?;
    let sample = run.ceiling.unwrap_or(run.iterations);
    let cost = run_cost(sample, &graph)?;
    drop(graph);

    let seed = settings
        .seed
        .unwrap_or_else(|| i64::from(rand::random::<u32>()));
    let config = crate::runner::mc_config(
        run.iterations,
        Some(seed),
        default_batch(),
        default_parallel(),
        true,
        run.ceiling,
        run.percentiles,
    );

    let admission =
        crate::billing::admit_compute_observed(&user.id, tier, &state.telemetry, Origin::Request)?;
    let monthly = crate::offload::monthly_budget(&entitlements, &state.config);
    let month = crate::offload::reserve(&state.db, &user.id, cost, monthly).await?;
    let inserted: Result<i64, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO compute_jobs (user_id, iterations, seed, cost, month)
         VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
    )
    .bind(&user.id)
    .bind(sample)
    .bind(seed)
    .bind(cost)
    .bind(&month)
    .fetch_one(&state.db)
    .await;
    let id = match inserted {
        Ok(id) => id,
        Err(error) => {
            crate::offload::refund(&state.db, &user.id, &month, cost).await;
            return Err(error.into());
        }
    };

    decision.accepted();
    state.telemetry.offload_cost(tier, cost as u64);
    state.runs.submit_compute(ComputeJob {
        id,
        user_id: user.id.clone(),
        tier,
        compiled,
        config,
        cost,
        admission,
    });
    Ok((StatusCode::ACCEPTED, Json(ComputeRunCreated { id })))
}

/// The snapshot as a plan: a JSON object, or the JSON text `snapshot()`
/// returns.
fn parse_snapshot(value: serde_json::Value) -> ApiResult<ScenarioGraph> {
    let parsed = match value {
        serde_json::Value::String(text) => serde_json::from_str(&text),
        other => serde_json::from_value(other),
    };
    parsed.map_err(|_| ApiError::bad_request("snapshot is not a valid plan snapshot"))
}

async fn fetch(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<ComputeRun>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        status: String,
        progress: i64,
        iterations: i64,
        seed: i64,
        error: Option<String>,
        expires_at: Option<String>,
        result_json: Option<String>,
    }
    // An expired job is gone even before the maintenance loop deletes it.
    let row: Option<Row> = sqlx::query_as(
        "SELECT status, progress, iterations, seed, error, expires_at, result_json
           FROM compute_jobs
          WHERE id = ?1 AND user_id = ?2
            AND (expires_at IS NULL OR expires_at > datetime('now'))",
    )
    .bind(id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;
    let row = row.ok_or(ApiError::NotFound("compute run"))?;
    let results = row
        .result_json
        .map(RawValue::from_string)
        .transpose()
        .map_err(|_| ApiError::internal("stored results are not valid JSON"))?;
    Ok(Json(ComputeRun {
        id,
        status: row.status,
        completed_iterations: row.progress,
        iterations: row.iterations,
        seed: row.seed,
        error: row.error,
        expires_at: row.expires_at,
        results,
    }))
}

/// Cancel a queued or running job and delete it, finished or not. A job that
/// never started gives its charge back; one that ran does not.
async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let deleted: Option<(String, String, i64)> = sqlx::query_as(
        "DELETE FROM compute_jobs WHERE id = ?1 AND user_id = ?2 RETURNING status, month, cost",
    )
    .bind(id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;
    let (status, month, cost) = deleted.ok_or(ApiError::NotFound("compute run"))?;
    state.runs.cancel_compute(id);
    if status == "queued" {
        crate::offload::refund(&state.db, &user.id, &month, cost).await;
    }
    Ok(StatusCode::NO_CONTENT)
}
