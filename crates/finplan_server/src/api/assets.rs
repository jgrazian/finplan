//! Assets are scenario-scoped: a price and a return profile assignment.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::state::AppState;
use ts_rs::TS;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios/{scenario_id}/assets", get(list).post(create))
        .route(
            "/scenarios/{scenario_id}/assets/{id}",
            get(fetch).patch(update).delete(destroy),
        )
}

#[derive(Debug, Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct Asset {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub initial_price: f64,
    pub return_profile_id: i64,
    pub tracking_error: Option<f64>,
    pub sort_order: i64,
}

const COLUMNS: &str =
    "id, name, description, initial_price, return_profile_id, tracking_error, sort_order";

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateAsset {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "one")]
    pub initial_price: f64,
    pub return_profile_id: i64,
    #[serde(default)]
    pub tracking_error: Option<f64>,
    #[serde(default)]
    pub sort_order: i64,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateAsset {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub initial_price: Option<f64>,
    #[serde(default)]
    pub return_profile_id: Option<i64>,
    #[serde(default)]
    pub tracking_error: Option<f64>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

/// Confirm a return profile exists in the caller's library before pointing an
/// asset at it. The FK only proves the row exists, not that it is theirs.
async fn owned_profile(state: &AppState, profile_id: i64, user_id: &str) -> ApiResult<()> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT id FROM return_profiles WHERE id = ?1 AND user_id = ?2")
            .bind(profile_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    found
        .map(|_| ())
        .ok_or(ApiError::NotFound("return profile"))
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Vec<Asset>>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let rows: Vec<Asset> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM assets WHERE scenario_id = ?1 ORDER BY sort_order, id"
    ))
    .bind(scenario_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn fetch(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<Json<Asset>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let row: Option<Asset> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM assets WHERE id = ?1 AND scenario_id = ?2"
    ))
    .bind(id)
    .bind(scenario_id)
    .fetch_optional(&state.db)
    .await?;
    row.map(Json).ok_or(ApiError::NotFound("asset"))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<CreateAsset>,
) -> ApiResult<(StatusCode, Json<Asset>)> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    owned_profile(&state, body.return_profile_id, &user.id).await?;

    if body.initial_price <= 0.0 {
        return Err(ApiError::bad_request("initial_price must be positive"));
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO assets
            (scenario_id, name, description, initial_price, return_profile_id,
             tracking_error, sort_order)
         VALUES (?1,?2,?3,?4,?5,?6,?7) RETURNING id",
    )
    .bind(scenario_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(body.initial_price)
    .bind(body.return_profile_id)
    .bind(body.tracking_error)
    .bind(body.sort_order)
    .fetch_one(&state.db)
    .await
    .map_err(|e| on_unique_violation(e, "an asset with that name already exists"))?;

    super::touch_scenario(&state.db, scenario_id).await?;

    let row: Asset = sqlx::query_as(&format!("SELECT {COLUMNS} FROM assets WHERE id = ?1"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
    Json(body): Json<UpdateAsset>,
) -> ApiResult<Json<Asset>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    if let Some(profile_id) = body.return_profile_id {
        owned_profile(&state, profile_id, &user.id).await?;
    }

    let affected = sqlx::query(
        "UPDATE assets SET
            name              = COALESCE(?3, name),
            description       = COALESCE(?4, description),
            initial_price     = COALESCE(?5, initial_price),
            return_profile_id = COALESCE(?6, return_profile_id),
            tracking_error    = COALESCE(?7, tracking_error),
            sort_order        = COALESCE(?8, sort_order),
            updated_at        = datetime('now')
          WHERE id = ?1 AND scenario_id = ?2",
    )
    .bind(id)
    .bind(scenario_id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(&body.description)
    .bind(body.initial_price)
    .bind(body.return_profile_id)
    .bind(body.tracking_error)
    .bind(body.sort_order)
    .execute(&state.db)
    .await
    .map_err(|e| on_unique_violation(e, "an asset with that name already exists"))?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("asset"));
    }
    super::touch_scenario(&state.db, scenario_id).await?;

    let row: Asset = sqlx::query_as(&format!("SELECT {COLUMNS} FROM assets WHERE id = ?1"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(row))
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((scenario_id, id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;

    // `account_property.asset_id` is ON DELETE RESTRICT, so deleting an asset a
    // property account is built on fails at the database. Report that clearly.
    let held_by: Option<String> = sqlx::query_scalar(
        "SELECT a.name FROM account_property p JOIN accounts a ON a.id = p.account_id
          WHERE p.asset_id = ?1 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    if let Some(account) = held_by {
        return Err(ApiError::Conflict(format!(
            "asset is the underlying value of property account '{account}'; delete that account first"
        )));
    }

    let affected = sqlx::query("DELETE FROM assets WHERE id = ?1 AND scenario_id = ?2")
        .bind(id)
        .bind(scenario_id)
        .execute(&state.db)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("asset"));
    }
    super::touch_scenario(&state.db, scenario_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
