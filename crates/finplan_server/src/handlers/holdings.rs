use axum::Json;
use axum::extract::{Path, State};
use sqlx::{Row, SqlitePool};

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::models::{CreateHoldingRequest, HoldingRow, UpdateHoldingRequest};

/// Verify that an account belongs to the given user and is an investment account.
async fn verify_investment_account(
    pool: &SqlitePool,
    account_id: i64,
    user_id: &str,
) -> Result<(), AppError> {
    let row = sqlx::query("SELECT category FROM accounts WHERE id = ? AND user_id = ?")
        .bind(account_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Account {account_id} not found")))?;

    let category: &str = row.get("category");
    if category != "Investment" {
        return Err(AppError::BadRequest(
            "Only investment accounts have holdings".into(),
        ));
    }
    Ok(())
}

/// Verify that a holding belongs to an account owned by the given user.
async fn verify_holding_ownership(
    pool: &SqlitePool,
    holding_id: i64,
    user_id: &str,
) -> Result<(), AppError> {
    let exists = sqlx::query(
        "SELECT h.id FROM holdings h \
         JOIN accounts a ON h.account_id = a.id \
         WHERE h.id = ? AND a.user_id = ?",
    )
    .bind(holding_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    if exists.is_none() {
        return Err(AppError::NotFound(format!(
            "Holding {holding_id} not found"
        )));
    }
    Ok(())
}

pub async fn list_holdings(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(account_id): Path<i64>,
) -> Result<Json<Vec<HoldingRow>>, AppError> {
    verify_investment_account(&pool, account_id, &user_id).await?;

    let holdings = sqlx::query_as::<_, HoldingRow>(
        "SELECT id, account_id, asset_name, value FROM holdings WHERE account_id = ? ORDER BY sort_order, id",
    )
    .bind(account_id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(holdings))
}

pub async fn create_holding(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(account_id): Path<i64>,
    Json(req): Json<CreateHoldingRequest>,
) -> Result<(axum::http::StatusCode, Json<HoldingRow>), AppError> {
    verify_investment_account(&pool, account_id, &user_id).await?;

    if req.asset_name.trim().is_empty() {
        return Err(AppError::BadRequest("Asset name is required".into()));
    }

    let holding = sqlx::query_as::<_, HoldingRow>(
        "INSERT INTO holdings (account_id, asset_name, value) \
         VALUES (?, ?, ?) \
         RETURNING id, account_id, asset_name, value",
    )
    .bind(account_id)
    .bind(&req.asset_name)
    .bind(req.value)
    .fetch_one(&pool)
    .await?;

    Ok((axum::http::StatusCode::CREATED, Json(holding)))
}

pub async fn update_holding(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(holding_id): Path<i64>,
    Json(req): Json<UpdateHoldingRequest>,
) -> Result<Json<HoldingRow>, AppError> {
    verify_holding_ownership(&pool, holding_id, &user_id).await?;

    if let Some(ref name) = req.asset_name
        && name.trim().is_empty()
    {
        return Err(AppError::BadRequest("Asset name cannot be empty".into()));
    }

    let holding = sqlx::query_as::<_, HoldingRow>(
        "UPDATE holdings SET \
            asset_name = COALESCE(?, asset_name), \
            value = COALESCE(?, value), \
            updated_at = datetime('now') \
         WHERE id = ? \
         RETURNING id, account_id, asset_name, value",
    )
    .bind(&req.asset_name)
    .bind(req.value)
    .bind(holding_id)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Holding {holding_id} not found")))?;

    Ok(Json(holding))
}

pub async fn delete_holding(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(holding_id): Path<i64>,
) -> Result<axum::http::StatusCode, AppError> {
    verify_holding_ownership(&pool, holding_id, &user_id).await?;

    sqlx::query("DELETE FROM holdings WHERE id = ?")
        .bind(holding_id)
        .execute(&pool)
        .await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}
