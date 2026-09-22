//! Paid report/comparison views over immutable saved run inputs and results.
use super::runs::{self, Results, ResultsQuery, RunInputs};
use crate::{auth::session::CurrentUser, error::ApiResult, state::AppState};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/runs/{id}/report", get(report))
        .route("/run-comparisons", post(compare))
}
#[derive(Serialize, TS)]
#[ts(export)]
pub struct RunReport {
    pub results: Results,
    pub inputs: RunInputs,
}
#[derive(Deserialize, TS)]
#[ts(export)]
pub struct CompareRuns {
    pub left_run_id: i64,
    pub right_run_id: i64,
}
#[derive(Serialize, TS)]
#[ts(export)]
pub struct RunComparison {
    pub left: RunReport,
    pub right: RunReport,
}
async fn bundle(state: AppState, user: CurrentUser, id: i64) -> ApiResult<RunReport> {
    let Json(inputs) = runs::inputs(State(state.clone()), user.clone(), Path(id)).await?;
    let Json(results) = runs::results(
        State(state),
        user,
        Path(id),
        Query(ResultsQuery { series: None }),
    )
    .await?;
    Ok(RunReport { results, inputs })
}
async fn report(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<RunReport>> {
    crate::billing::require_pro(&state.db, &user.id, &state.config).await?;
    Ok(Json(bundle(state, user, id).await?))
}
async fn compare(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(request): Json<CompareRuns>,
) -> ApiResult<Json<RunComparison>> {
    crate::billing::require_pro(&state.db, &user.id, &state.config).await?;
    let left = bundle(state.clone(), user.clone(), request.left_run_id).await?;
    let right = bundle(state, user, request.right_run_id).await?;
    Ok(Json(RunComparison { left, right }))
}
