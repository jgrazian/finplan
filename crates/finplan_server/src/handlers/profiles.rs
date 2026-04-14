use axum::Json;
use axum::extract::{Path, State};
use sqlx::SqlitePool;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::models::{
    ProfileRequest, ReturnProfileResponse, ReturnProfileRow, is_valid_bootstrap_preset,
    is_valid_profile_type,
};

fn validate_request(req: &ProfileRequest) -> Result<(), AppError> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("Profile name is required".into()));
    }
    if !is_valid_profile_type(&req.profile_type) {
        return Err(AppError::BadRequest(format!(
            "Invalid profile_type: {}",
            req.profile_type
        )));
    }
    match req.profile_type.as_str() {
        "Fixed" => {
            if req.rate.is_none() {
                return Err(AppError::BadRequest("Fixed profile requires a rate".into()));
            }
        }
        "Normal" | "LogNormal" => {
            if req.mean.is_none() || req.std_dev.is_none() {
                return Err(AppError::BadRequest(format!(
                    "{} profile requires mean and std_dev",
                    req.profile_type
                )));
            }
        }
        "StudentT" => {
            if req.mean.is_none() || req.scale.is_none() || req.df.is_none() {
                return Err(AppError::BadRequest(
                    "StudentT profile requires mean, scale, and df".into(),
                ));
            }
        }
        "Bootstrap" => {
            let preset = req.preset.as_deref().ok_or_else(|| {
                AppError::BadRequest("Bootstrap profile requires a preset".into())
            })?;
            if !is_valid_bootstrap_preset(preset) {
                return Err(AppError::BadRequest(format!(
                    "Unknown historical preset: {preset}"
                )));
            }
            if let Some(bs) = req.block_size
                && bs < 1
            {
                return Err(AppError::BadRequest(
                    "block_size must be >= 1 (or omitted for i.i.d. sampling)".into(),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

pub async fn list_profiles(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
) -> Result<Json<Vec<ReturnProfileResponse>>, AppError> {
    let rows = sqlx::query_as::<_, ReturnProfileRow>(
        "SELECT id, user_id, name, description, profile_type, rate, mean, std_dev, scale, df, preset, block_size \
         FROM return_profiles WHERE user_id = ? ORDER BY sort_order, id",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

pub async fn create_profile(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Json(req): Json<ProfileRequest>,
) -> Result<(axum::http::StatusCode, Json<ReturnProfileResponse>), AppError> {
    validate_request(&req)?;

    let row = sqlx::query_as::<_, ReturnProfileRow>(
        "INSERT INTO return_profiles \
         (user_id, name, description, profile_type, rate, mean, std_dev, scale, df, preset, block_size) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         RETURNING id, user_id, name, description, profile_type, rate, mean, std_dev, scale, df, preset, block_size",
    )
    .bind(user_id)
    .bind(req.name.trim())
    .bind(&req.description)
    .bind(&req.profile_type)
    .bind(req.rate)
    .bind(req.mean)
    .bind(req.std_dev)
    .bind(req.scale)
    .bind(req.df)
    .bind(&req.preset)
    .bind(req.block_size)
    .fetch_one(&pool)
    .await
    .map_err(|err| match err {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            AppError::BadRequest("A profile with that name already exists".into())
        }
        other => other.into(),
    })?;

    Ok((axum::http::StatusCode::CREATED, Json(row.into())))
}

pub async fn update_profile(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<ProfileRequest>,
) -> Result<Json<ReturnProfileResponse>, AppError> {
    validate_request(&req)?;

    // Capture the old name so we can propagate renames to accounts.return_profile.
    let old_name: Option<String> =
        sqlx::query_scalar("SELECT name FROM return_profiles WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(&user_id)
            .fetch_optional(&pool)
            .await?;

    let old_name = old_name.ok_or_else(|| AppError::NotFound(format!("Profile {id} not found")))?;

    let new_name = req.name.trim().to_string();

    let row = sqlx::query_as::<_, ReturnProfileRow>(
        "UPDATE return_profiles SET \
            name = ?, description = ?, profile_type = ?, \
            rate = ?, mean = ?, std_dev = ?, scale = ?, df = ?, \
            preset = ?, block_size = ?, \
            updated_at = datetime('now') \
         WHERE id = ? AND user_id = ? \
         RETURNING id, user_id, name, description, profile_type, rate, mean, std_dev, scale, df, preset, block_size",
    )
    .bind(&new_name)
    .bind(&req.description)
    .bind(&req.profile_type)
    .bind(req.rate)
    .bind(req.mean)
    .bind(req.std_dev)
    .bind(req.scale)
    .bind(req.df)
    .bind(&req.preset)
    .bind(req.block_size)
    .bind(id)
    .bind(&user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|err| match err {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            AppError::BadRequest("A profile with that name already exists".into())
        }
        other => other.into(),
    })?
    .ok_or_else(|| AppError::NotFound(format!("Profile {id} not found")))?;

    // Propagate rename to any account whose return_profile pointed to the old name.
    if old_name != new_name {
        sqlx::query(
            "UPDATE accounts SET return_profile = ?, updated_at = datetime('now') \
             WHERE user_id = ? AND return_profile = ?",
        )
        .bind(&new_name)
        .bind(&user_id)
        .bind(&old_name)
        .execute(&pool)
        .await?;
    }

    Ok(Json(row.into()))
}

pub async fn delete_profile(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, AppError> {
    // Clear any account references to this profile (by name).
    let name: Option<String> =
        sqlx::query_scalar("SELECT name FROM return_profiles WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(&user_id)
            .fetch_optional(&pool)
            .await?;

    let name = name.ok_or_else(|| AppError::NotFound(format!("Profile {id} not found")))?;

    // Remove account references by name.
    sqlx::query(
        "UPDATE accounts SET return_profile = NULL, updated_at = datetime('now') \
         WHERE user_id = ? AND return_profile = ?",
    )
    .bind(&user_id)
    .bind(&name)
    .execute(&pool)
    .await?;

    // Asset mappings cascade via FK.
    let result = sqlx::query("DELETE FROM return_profiles WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(&user_id)
        .execute(&pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Profile {id} not found")));
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}
