use std::collections::HashMap;

use axum::Json;
use axum::extract::State;
use sqlx::{Row, SqlitePool};

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::models::{AllocationResponse, AllocationSlice};

pub async fn get_allocation(
    State(pool): State<SqlitePool>,
    AuthUser(user_id): AuthUser,
) -> Result<Json<AllocationResponse>, AppError> {
    let accounts = sqlx::query(
        "SELECT id, name, category, value, balance FROM accounts WHERE user_id = ? ORDER BY sort_order, id",
    )
    .bind(&user_id)
    .fetch_all(&pool)
    .await?;

    let holdings = sqlx::query(
        "SELECT h.account_id, h.asset_name, h.value FROM holdings h \
         JOIN accounts a ON h.account_id = a.id \
         WHERE a.user_id = ? \
         ORDER BY h.account_id, h.sort_order, h.id",
    )
    .bind(&user_id)
    .fetch_all(&pool)
    .await?;

    // Group holdings by account
    let mut holdings_by_account: HashMap<i64, Vec<(String, f64)>> = HashMap::new();
    for h in &holdings {
        let account_id: i64 = h.get("account_id");
        let asset_name: String = h.get("asset_name");
        let value: f64 = h.get("value");
        holdings_by_account
            .entry(account_id)
            .or_default()
            .push((asset_name, value));
    }

    let mut by_account: Vec<(String, f64)> = Vec::new();
    let mut by_category: HashMap<String, f64> = HashMap::new();
    let mut by_asset: HashMap<String, f64> = HashMap::new();

    for row in &accounts {
        let id: i64 = row.get("id");
        let name: String = row.get("name");
        let category: String = row.get("category");
        let value: Option<f64> = row.get("value");
        let balance: Option<f64> = row.get("balance");

        let account_value = match category.as_str() {
            "Investment" => {
                let acct_holdings = holdings_by_account.get(&id);
                let total: f64 = acct_holdings
                    .map(|hs| hs.iter().map(|(_, v)| v).sum())
                    .unwrap_or(0.0);

                if let Some(hs) = acct_holdings {
                    for (asset_name, val) in hs {
                        *by_asset.entry(asset_name.clone()).or_default() += val;
                    }
                }

                total
            }
            "Cash" => {
                let v = value.unwrap_or(0.0);
                *by_asset.entry("Cash".to_string()).or_default() += v;
                v
            }
            "Debt" => -(balance.unwrap_or(0.0).abs()),
            _ => 0.0,
        };

        by_account.push((name, account_value));
        *by_category.entry(category).or_default() += account_value;
    }

    let positive_total: f64 = by_account.iter().map(|(_, v)| v.max(0.0)).sum();
    let total_value: f64 = by_account.iter().map(|(_, v)| *v).sum();

    let make_slices = |items: Vec<(String, f64)>, total: f64| -> Vec<AllocationSlice> {
        items
            .into_iter()
            .filter(|(_, v)| *v != 0.0)
            .map(|(name, value)| AllocationSlice {
                percentage: if total != 0.0 {
                    value / total * 100.0
                } else {
                    0.0
                },
                name,
                value,
            })
            .collect()
    };

    let by_category_vec: Vec<(String, f64)> = by_category.into_iter().collect();
    let by_asset_vec: Vec<(String, f64)> = by_asset.into_iter().collect();

    Ok(Json(AllocationResponse {
        total_value,
        by_account: make_slices(by_account, positive_total),
        by_asset: make_slices(by_asset_vec, positive_total),
        by_category: make_slices(by_category_vec, positive_total),
    }))
}
