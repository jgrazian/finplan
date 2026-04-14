use axum::Json;
use axum::extract::{Path, State};
use sqlx::{Row, SqlitePool};

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::models::{AssetMappingResponse, UpsertMappingRequest};

pub async fn list_mappings(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
) -> Result<Json<Vec<AssetMappingResponse>>, AppError> {
    let rows = sqlx::query(
        "SELECT m.asset_name, m.profile_id, p.name AS profile_name \
         FROM asset_mappings m JOIN return_profiles p ON m.profile_id = p.id \
         WHERE m.user_id = ? ORDER BY m.asset_name",
    )
    .bind(&user_id)
    .fetch_all(&pool)
    .await?;

    let mappings = rows
        .into_iter()
        .map(|row| AssetMappingResponse {
            asset_name: row.get("asset_name"),
            profile_id: row.get("profile_id"),
            profile_name: row.get("profile_name"),
        })
        .collect();

    Ok(Json(mappings))
}

pub async fn upsert_mapping(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(asset_name): Path<String>,
    Json(req): Json<UpsertMappingRequest>,
) -> Result<Json<AssetMappingResponse>, AppError> {
    let asset_name = asset_name.trim().to_string();
    if asset_name.is_empty() {
        return Err(AppError::BadRequest("Asset name is required".into()));
    }

    // Verify the profile belongs to this user.
    let profile_name: Option<String> =
        sqlx::query_scalar("SELECT name FROM return_profiles WHERE id = ? AND user_id = ?")
            .bind(req.profile_id)
            .bind(&user_id)
            .fetch_optional(&pool)
            .await?;

    let profile_name = profile_name
        .ok_or_else(|| AppError::BadRequest(format!("Profile {} not found", req.profile_id)))?;

    sqlx::query(
        "INSERT INTO asset_mappings (user_id, asset_name, profile_id) \
         VALUES (?, ?, ?) \
         ON CONFLICT(user_id, asset_name) DO UPDATE SET \
             profile_id = excluded.profile_id, \
             updated_at = datetime('now')",
    )
    .bind(&user_id)
    .bind(&asset_name)
    .bind(req.profile_id)
    .execute(&pool)
    .await?;

    Ok(Json(AssetMappingResponse {
        asset_name,
        profile_id: req.profile_id,
        profile_name,
    }))
}

pub async fn delete_mapping(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(asset_name): Path<String>,
) -> Result<axum::http::StatusCode, AppError> {
    let result = sqlx::query("DELETE FROM asset_mappings WHERE user_id = ? AND asset_name = ?")
        .bind(&user_id)
        .bind(&asset_name)
        .execute(&pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!(
            "Mapping for asset '{asset_name}' not found"
        )));
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}
