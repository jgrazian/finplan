//! Validate editable amount source in the context of the selected effect.
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use serde::Deserialize;
use ts_rs::TS;

use finplan_plan::expressions::{ExpressionValidation, validate_effect};

use crate::{auth::session::CurrentUser, error::ApiResult, state::AppState};
use finplan_plan::specs::EffectSpec;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/scenarios/{scenario_id}/expressions/validate",
        post(validate),
    )
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ExpressionValidationRequest {
    pub effect: EffectSpec,
}

async fn validate(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ExpressionValidationRequest>,
) -> ApiResult<Json<ExpressionValidation>> {
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    Ok(Json(validate_effect(&graph, &body.effect)?))
}
