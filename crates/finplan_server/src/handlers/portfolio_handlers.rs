use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use std::collections::HashMap;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::*;
use crate::validation;

pub type DbConn = std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>;

pub async fn list_portfolios(State(db): State<DbConn>) -> ApiResult<Json<Vec<PortfolioListItem>>> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare("SELECT id, name, description, accounts, created_at, updated_at FROM portfolios ORDER BY updated_at DESC")?;

    let portfolios = stmt
        .query_map([], |row| {
            let accounts_json: String = row.get(3)?;
            let accounts: Vec<PortfolioAccount> =
                serde_json::from_str(&accounts_json).unwrap_or_default();
            let total_value: f64 = accounts
                .iter()
                .flat_map(|a| a.assets.iter())
                .map(|asset| asset.initial_value)
                .sum();
            Ok(PortfolioListItem {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                total_value,
                account_count: accounts.len(),
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(portfolios))
}

pub async fn create_portfolio(
    State(db): State<DbConn>,
    Json(req): Json<CreatePortfolioRequest>,
) -> ApiResult<Json<SavedPortfolio>> {
    // Validate input
    validation::validate_portfolio_name(&req.name)?;
    validation::validate_portfolio_has_accounts(req.accounts.len())?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let accounts_json = serde_json::to_string(&req.accounts)?;

    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO portfolios (id, name, description, accounts, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, req.name, req.description, accounts_json, now, now],
    )?;

    Ok(Json(SavedPortfolio {
        id,
        name: req.name,
        description: req.description,
        accounts: req.accounts,
        created_at: now.clone(),
        updated_at: now,
    }))
}

pub async fn get_portfolio(
    State(db): State<DbConn>,
    Path(id): Path<String>,
) -> ApiResult<Json<SavedPortfolio>> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare("SELECT id, name, description, accounts, created_at, updated_at FROM portfolios WHERE id = ?1")?;

    let portfolio = stmt
        .query_row([&id], |row| {
            let accounts_json: String = row.get(3)?;
            let accounts: Vec<PortfolioAccount> =
                serde_json::from_str(&accounts_json).map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(SavedPortfolio {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                accounts,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ApiError::PortfolioNotFound(id.clone()),
            _ => ApiError::from(e),
        })?;

    Ok(Json(portfolio))
}

pub async fn update_portfolio(
    State(db): State<DbConn>,
    Path(id): Path<String>,
    Json(req): Json<UpdatePortfolioRequest>,
) -> ApiResult<Json<SavedPortfolio>> {
    let conn = db.lock()?;

    // Validate name if provided
    if let Some(ref name) = req.name {
        validation::validate_portfolio_name(name)?;
    }

    // Validate accounts if provided
    if let Some(ref accounts) = req.accounts {
        validation::validate_portfolio_has_accounts(accounts.len())?;
    }

    // Get existing portfolio
    let mut stmt = conn
        .prepare("SELECT name, description, accounts, created_at FROM portfolios WHERE id = ?1")?;

    let (current_name, current_desc, current_accounts_json, created_at): (
        String,
        Option<String>,
        String,
        String,
    ) = stmt
        .query_row([&id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ApiError::PortfolioNotFound(id.clone()),
            _ => ApiError::from(e),
        })?;

    let name = req.name.unwrap_or(current_name);
    let description = req.description.or(current_desc);
    let accounts = if let Some(a) = req.accounts {
        a
    } else {
        serde_json::from_str(&current_accounts_json)?
    };

    let accounts_json = serde_json::to_string(&accounts)?;
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "UPDATE portfolios SET name = ?1, description = ?2, accounts = ?3, updated_at = ?4 WHERE id = ?5",
        rusqlite::params![name, description, accounts_json, now, id],
    )?;

    Ok(Json(SavedPortfolio {
        id,
        name,
        description,
        accounts,
        created_at,
        updated_at: now,
    }))
}

pub async fn delete_portfolio(
    State(db): State<DbConn>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let conn = db.lock()?;

    let affected = conn.execute("DELETE FROM portfolios WHERE id = ?1", [&id])?;

    if affected == 0 {
        Err(ApiError::PortfolioNotFound(id))
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

pub async fn get_portfolio_networth(
    State(db): State<DbConn>,
    Path(id): Path<String>,
) -> ApiResult<Json<PortfolioNetworth>> {
    let conn = db.lock()?;
    let mut stmt = conn.prepare("SELECT accounts FROM portfolios WHERE id = ?1")?;

    let accounts_json: String = stmt
        .query_row([&id], |row| row.get(0))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ApiError::PortfolioNotFound(id.clone()),
            _ => ApiError::from(e),
        })?;

    let accounts: Vec<PortfolioAccount> = serde_json::from_str(&accounts_json)?;

    let mut total_value = 0.0;
    let mut by_account_type: HashMap<String, f64> = HashMap::new();
    let mut by_asset_class: HashMap<String, f64> = HashMap::new();

    for account in &accounts {
        let account_type = format!("{:?}", account.tax_status);
        for asset in &account.assets {
            total_value += asset.initial_value;
            *by_account_type.entry(account_type.clone()).or_insert(0.0) += asset.initial_value;
            let asset_class = asset.asset_class.clone().unwrap_or_else(|| "Unknown".to_string());
            *by_asset_class.entry(asset_class).or_insert(0.0) += asset.initial_value;
        }
    }

    Ok(Json(PortfolioNetworth {
        total_value,
        by_account_type,
        by_asset_class,
    }))
}
