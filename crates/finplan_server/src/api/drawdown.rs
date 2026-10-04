//! Drawdown (spec 20): re-simulate a run's input snapshot on its median path's
//! seed with each withdrawal strategy swapped in, and compare strategies over a
//! small Monte Carlo. The folding is `finplan_plan::drawdown`; this only finds
//! the run's inputs and puts the CPU work off the async runtime.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};

use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::observability::Origin;
use crate::state::AppState;
use finplan_core::analysis::{ProgressRunner, SweepProgress};
use finplan_plan::analysis::AnalysisError;
use finplan_plan::drawdown::{
    self, CompareRequest, DEFAULT_COMPARE_ITERATIONS, DrawdownBody, DrawdownComparison,
    DrawdownRequest,
};
use finplan_plan::graph::ScenarioGraph;

use super::what_if::CancelOnDrop;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/runs/{run_id}/drawdown", post(project))
        .route("/runs/{run_id}/drawdown/compare", post(compare))
}

const STALE: &str = "Run the plan again to see drawdown.";

/// The run's input snapshot and its median path's seed, or 409 when the run is
/// not finished or predates stored seeds or snapshots.
async fn run_inputs(
    state: &AppState,
    user: &CurrentUser,
    run_id: i64,
) -> ApiResult<(ScenarioGraph, u64)> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT status, snapshot_json FROM runs WHERE id = ?1 AND user_id = ?2")
            .bind(run_id)
            .bind(&user.id)
            .fetch_optional(&state.db)
            .await?;
    let (status, snapshot) = row.ok_or(ApiError::NotFound("run"))?;
    let seed = crate::db::median_seed(&state.db, run_id).await?;
    let (true, Some(snapshot), Some(seed)) = (status == "succeeded", snapshot, seed) else {
        return Err(ApiError::Conflict(STALE.into()));
    };
    let graph = serde_json::from_str(&snapshot)
        .map_err(|_| ApiError::internal("the run's saved inputs cannot be read"))?;
    Ok((graph, seed))
}

/// `POST /runs/{id}/drawdown`: the yearly rows of the median path under each
/// strategy in the request (a default list when it names none).
async fn project(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(run_id): Path<i64>,
    Json(request): Json<DrawdownRequest>,
) -> ApiResult<Json<DrawdownBody>> {
    let (graph, seed) = run_inputs(&state, &user, run_id).await?;
    // A handful of single simulations: no admission, but off the async threads.
    let body = tokio::task::spawn_blocking(move || drawdown::project(&graph, seed, &request)).await;
    match body {
        Ok(result) => Ok(Json(result?)),
        Err(_) => Err(ApiError::Internal("drawdown failed".into())),
    }
}

/// `POST /runs/{id}/drawdown/compare`: a Monte Carlo per strategy on one common
/// seed.
async fn compare(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(run_id): Path<i64>,
    Json(body): Json<CompareRequest>,
) -> ApiResult<Json<DrawdownComparison>> {
    let (graph, _) = run_inputs(&state, &user, run_id).await?;
    let tier = crate::billing::entitlements(&state.db, &user.id, &state.config)
        .await?
        .tier(&state.config);
    let permit =
        crate::billing::admit_compute_observed(&user.id, tier, &state.telemetry, Origin::Request)?;

    let choices = body
        .request
        .as_ref()
        .and_then(|r| r.strategies.as_ref())
        .map_or(7, Vec::len);
    let budget = (body.iterations.unwrap_or(DEFAULT_COMPARE_ITERATIONS) + 1) * choices;
    let progress = SweepProgress::new(budget);
    let guard = CancelOnDrop(progress.clone());
    let outcome = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        drawdown::compare(&graph, &body, &mut ProgressRunner::new(Some(&progress)))
    })
    .await;
    drop(guard);
    match outcome {
        Ok(Ok(comparison)) => Ok(Json(comparison)),
        Ok(Err(AnalysisError::Plan(error))) => Err(error.into()),
        Ok(Err(AnalysisError::Cancelled)) => Err(ApiError::Conflict("comparison canceled".into())),
        Ok(Err(AnalysisError::Failed(_))) | Err(_) => {
            Err(ApiError::Internal("comparison failed".into()))
        }
    }
}
