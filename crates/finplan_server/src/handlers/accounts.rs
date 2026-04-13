use axum::Json;
use axum::extract::{Path, State};
use sqlx::SqlitePool;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::models::{
    AccountResponse, AccountRow, CreateAccountRequest, HoldingRow, UpdateAccountRequest,
    category_for_type,
};

async fn fetch_holdings(
    pool: &SqlitePool,
    account_id: i64,
) -> Result<Vec<HoldingRow>, sqlx::Error> {
    sqlx::query_as::<_, HoldingRow>(
        "SELECT id, account_id, asset_name, value FROM holdings WHERE account_id = ? ORDER BY sort_order, id",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await
}

pub async fn list_accounts(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
) -> Result<Json<Vec<AccountResponse>>, AppError> {
    let rows = sqlx::query_as::<_, AccountRow>(
        "SELECT id, user_id, name, description, account_type, category, value, return_profile, balance, interest_rate \
         FROM accounts WHERE user_id = ? ORDER BY sort_order, id",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let mut accounts = Vec::with_capacity(rows.len());
    for row in rows {
        let holdings = if row.category == "Investment" {
            Some(fetch_holdings(&pool, row.id).await?)
        } else {
            None
        };
        accounts.push(row.into_response(holdings));
    }

    Ok(Json(accounts))
}

pub async fn get_account(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<AccountResponse>, AppError> {
    let row = sqlx::query_as::<_, AccountRow>(
        "SELECT id, user_id, name, description, account_type, category, value, return_profile, balance, interest_rate \
         FROM accounts WHERE id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Account {id} not found")))?;

    let holdings = if row.category == "Investment" {
        Some(fetch_holdings(&pool, row.id).await?)
    } else {
        None
    };

    Ok(Json(row.into_response(holdings)))
}

pub async fn create_account(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Json(req): Json<CreateAccountRequest>,
) -> Result<(axum::http::StatusCode, Json<AccountResponse>), AppError> {
    let category = category_for_type(&req.account_type).ok_or_else(|| {
        AppError::BadRequest(format!("Invalid account_type: {}", req.account_type))
    })?;

    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("Account name is required".into()));
    }

    let row = sqlx::query_as::<_, AccountRow>(
        "INSERT INTO accounts (user_id, name, description, account_type, category, value, return_profile, balance, interest_rate) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
         RETURNING id, user_id, name, description, account_type, category, value, return_profile, balance, interest_rate",
    )
    .bind(user_id)
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.account_type)
    .bind(category)
    .bind(req.value)
    .bind(&req.return_profile)
    .bind(req.balance)
    .bind(req.interest_rate)
    .fetch_one(&pool)
    .await?;

    let holdings = if row.category == "Investment" {
        Some(vec![])
    } else {
        None
    };

    Ok((
        axum::http::StatusCode::CREATED,
        Json(row.into_response(holdings)),
    ))
}

pub async fn update_account(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<UpdateAccountRequest>,
) -> Result<Json<AccountResponse>, AppError> {
    if let Some(ref name) = req.name
        && name.trim().is_empty()
    {
        return Err(AppError::BadRequest("Account name cannot be empty".into()));
    }

    let row = sqlx::query_as::<_, AccountRow>(
        "UPDATE accounts SET \
            name = COALESCE(?, name), \
            description = COALESCE(?, description), \
            value = COALESCE(?, value), \
            return_profile = COALESCE(?, return_profile), \
            balance = COALESCE(?, balance), \
            interest_rate = COALESCE(?, interest_rate), \
            updated_at = datetime('now') \
         WHERE id = ? AND user_id = ? \
         RETURNING id, user_id, name, description, account_type, category, value, return_profile, balance, interest_rate",
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(req.value)
    .bind(&req.return_profile)
    .bind(req.balance)
    .bind(req.interest_rate)
    .bind(id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Account {id} not found")))?;

    let holdings = if row.category == "Investment" {
        Some(fetch_holdings(&pool, id).await?)
    } else {
        None
    };

    Ok(Json(row.into_response(holdings)))
}

pub async fn delete_account(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, AppError> {
    let result = sqlx::query("DELETE FROM accounts WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(&pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Account {id} not found")));
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}
