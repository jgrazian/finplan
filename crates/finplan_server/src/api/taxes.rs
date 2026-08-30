//! Tax configurations and their bracket tables.

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
        .route("/tax-configs", get(list).post(create))
        .route(
            "/tax-configs/{id}",
            get(fetch).patch(update).delete(destroy),
        )
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Bracket {
    pub threshold: f64,
    pub rate: f64,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct TaxConfig {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub state_rate: f64,
    pub capital_gains_rate: f64,
    pub early_withdrawal_penalty_rate: f64,
    pub federal_brackets: Vec<Bracket>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateTaxConfig {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub state_rate: f64,
    #[serde(default = "default_cap_gains")]
    pub capital_gains_rate: f64,
    #[serde(default = "default_penalty")]
    pub early_withdrawal_penalty_rate: f64,
    pub federal_brackets: Vec<Bracket>,
}

fn default_cap_gains() -> f64 {
    0.15
}

fn default_penalty() -> f64 {
    0.10
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateTaxConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub state_rate: Option<f64>,
    #[serde(default)]
    pub capital_gains_rate: Option<f64>,
    #[serde(default)]
    pub early_withdrawal_penalty_rate: Option<f64>,
    /// Replaces the whole bracket table when present.
    #[serde(default)]
    pub federal_brackets: Option<Vec<Bracket>>,
}

/// The engine walks brackets assuming ascending, gap-free thresholds beginning
/// at zero, so validate that here rather than producing quietly wrong tax.
fn validate_brackets(brackets: &[Bracket]) -> ApiResult<Vec<Bracket>> {
    if brackets.is_empty() {
        return Err(ApiError::bad_request(
            "a tax config needs at least one federal bracket",
        ));
    }

    let mut sorted = brackets.to_vec();
    sorted.sort_by(|a, b| {
        a.threshold
            .partial_cmp(&b.threshold)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if sorted[0].threshold != 0.0 {
        return Err(ApiError::bad_request(
            "the lowest federal bracket must start at a threshold of 0",
        ));
    }

    for pair in sorted.windows(2) {
        if pair[0].threshold == pair[1].threshold {
            return Err(ApiError::bad_request(format!(
                "duplicate bracket threshold {}",
                pair[0].threshold
            )));
        }
    }

    for bracket in &sorted {
        if !(0.0..=1.0).contains(&bracket.rate) {
            return Err(ApiError::bad_request(format!(
                "bracket rate {} is outside 0..1; rates are fractions, not percentages",
                bracket.rate
            )));
        }
    }

    Ok(sorted)
}

async fn load(state: &AppState, id: i64, user_id: &str) -> ApiResult<TaxConfig> {
    let row: Option<(i64, String, Option<String>, f64, f64, f64)> = sqlx::query_as(
        "SELECT id, name, description, state_rate, capital_gains_rate,
                early_withdrawal_penalty_rate
           FROM tax_configs WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    let (id, name, description, state_rate, capital_gains_rate, early) =
        row.ok_or(ApiError::NotFound("tax config"))?;

    let brackets: Vec<(f64, f64)> = sqlx::query_as(
        "SELECT threshold, rate FROM tax_brackets WHERE tax_config_id = ?1 ORDER BY threshold",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(TaxConfig {
        id,
        name,
        description,
        state_rate,
        capital_gains_rate,
        early_withdrawal_penalty_rate: early,
        federal_brackets: brackets
            .into_iter()
            .map(|(threshold, rate)| Bracket { threshold, rate })
            .collect(),
    })
}

async fn list(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Vec<TaxConfig>>> {
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM tax_configs WHERE user_id = ?1 ORDER BY name")
            .bind(&user.id)
            .fetch_all(&state.db)
            .await?;

    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(load(&state, id, &user.id).await?);
    }
    Ok(Json(out))
}

async fn fetch(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<TaxConfig>> {
    Ok(Json(load(&state, id, &user.id).await?))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<CreateTaxConfig>,
) -> ApiResult<(StatusCode, Json<TaxConfig>)> {
    let brackets = validate_brackets(&body.federal_brackets)?;

    let mut tx = state.db.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tax_configs
            (user_id, name, description, state_rate, capital_gains_rate,
             early_withdrawal_penalty_rate)
         VALUES (?1,?2,?3,?4,?5,?6) RETURNING id",
    )
    .bind(&user.id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(body.state_rate)
    .bind(body.capital_gains_rate)
    .bind(body.early_withdrawal_penalty_rate)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "a tax config with that name already exists"))?;

    for bracket in &brackets {
        sqlx::query("INSERT INTO tax_brackets (tax_config_id, threshold, rate) VALUES (?1,?2,?3)")
            .bind(id)
            .bind(bracket.threshold)
            .bind(bracket.rate)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(load(&state, id, &user.id).await?)))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateTaxConfig>,
) -> ApiResult<Json<TaxConfig>> {
    // Confirm ownership up front so a miss is a 404, not a silent no-op.
    load(&state, id, &user.id).await?;

    let brackets = body
        .federal_brackets
        .as_deref()
        .map(validate_brackets)
        .transpose()?;

    let mut tx = state.db.begin().await?;

    sqlx::query(
        "UPDATE tax_configs SET
            name                          = COALESCE(?3, name),
            description                   = COALESCE(?4, description),
            state_rate                    = COALESCE(?5, state_rate),
            capital_gains_rate            = COALESCE(?6, capital_gains_rate),
            early_withdrawal_penalty_rate = COALESCE(?7, early_withdrawal_penalty_rate),
            updated_at                    = datetime('now')
          WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(&user.id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(&body.description)
    .bind(body.state_rate)
    .bind(body.capital_gains_rate)
    .bind(body.early_withdrawal_penalty_rate)
    .execute(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "a tax config with that name already exists"))?;

    if let Some(brackets) = brackets {
        sqlx::query("DELETE FROM tax_brackets WHERE tax_config_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for bracket in &brackets {
            sqlx::query(
                "INSERT INTO tax_brackets (tax_config_id, threshold, rate) VALUES (?1,?2,?3)",
            )
            .bind(id)
            .bind(bracket.threshold)
            .bind(bracket.rate)
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;
    Ok(Json(load(&state, id, &user.id).await?))
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let affected = sqlx::query("DELETE FROM tax_configs WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(&user.id)
        .execute(&state.db)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("tax config"));
    }
    Ok(StatusCode::NO_CONTENT)
}
