//! Tax configurations and their bracket tables.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::auth::activity::{ActivityFields, Submitted};
use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::observability::{EventFields, Operation, Resource};
use crate::state::AppState;
use ts_rs::TS;

use finplan_plan::specs::taxes::NAME_TAKEN;
use finplan_plan::specs::taxes::{
    Bracket, CreateTaxConfig, UpdateTaxConfig, checked, validate_brackets, validate_deductions,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tax-configs", get(list).post(create))
        .route(
            "/tax-configs/{id}",
            get(fetch).patch(update).delete(destroy),
        )
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
    /// Federal standard deduction, in the brackets' dollars.
    pub standard_deduction: f64,
    /// Added to the deduction from the tax year the person turns 65.
    pub age_65_extra_deduction: f64,
    pub federal_brackets: Vec<Bracket>,
}

async fn load(state: &AppState, id: i64, user_id: &str) -> ApiResult<TaxConfig> {
    #[allow(clippy::type_complexity)]
    let row: Option<(i64, String, Option<String>, f64, f64, f64, f64, f64)> = sqlx::query_as(
        "SELECT id, name, description, state_rate, capital_gains_rate,
                early_withdrawal_penalty_rate, standard_deduction, age_65_extra_deduction
           FROM tax_configs WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    let (
        id,
        name,
        description,
        state_rate,
        capital_gains_rate,
        early,
        standard_deduction,
        age_65_extra_deduction,
    ) = row.ok_or(ApiError::NotFound("tax config"))?;

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
        standard_deduction,
        age_65_extra_deduction,
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
    Json(Submitted { body, fields }): Json<Submitted<CreateTaxConfig>>,
) -> ApiResult<(StatusCode, Json<TaxConfig>)> {
    let mut tx = state.db.begin().await?;
    let id = create_in(&mut tx, &user.id, &body).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::TaxConfig,
        Operation::Created,
        &EventFields {
            user_id: Some(&user.id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );

    Ok((StatusCode::CREATED, Json(load(&state, id, &user.id).await?)))
}

/// Insert a tax config and its bracket table, as `POST /tax-configs` does;
/// returns its id. The suggestion path writes through this too, inside its
/// transaction.
pub(crate) async fn create_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    body: &CreateTaxConfig,
) -> ApiResult<i64> {
    let brackets = checked(body)?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tax_configs
            (user_id, name, description, state_rate, capital_gains_rate,
             early_withdrawal_penalty_rate, standard_deduction, age_65_extra_deduction)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8) RETURNING id",
    )
    .bind(user_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(body.state_rate)
    .bind(body.capital_gains_rate)
    .bind(body.early_withdrawal_penalty_rate)
    .bind(body.standard_deduction)
    .bind(body.age_65_extra_deduction)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, NAME_TAKEN))?;

    for bracket in &brackets {
        sqlx::query("INSERT INTO tax_brackets (tax_config_id, threshold, rate) VALUES (?1,?2,?3)")
            .bind(id)
            .bind(bracket.threshold)
            .bind(bracket.rate)
            .execute(&mut **tx)
            .await?;
    }
    Ok(id)
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(Submitted { body, fields }): Json<Submitted<UpdateTaxConfig>>,
) -> ApiResult<Json<TaxConfig>> {
    // Confirm ownership up front so a miss is a 404, not a silent no-op.
    load(&state, id, &user.id).await?;
    validate_deductions([
        ("standard_deduction", body.standard_deduction),
        ("age_65_extra_deduction", body.age_65_extra_deduction),
    ])?;

    let brackets = body
        .federal_brackets
        .as_deref()
        .map(validate_brackets)
        .transpose()?;

    let mut tx = state.db.begin().await?;

    let affected = sqlx::query(
        "UPDATE tax_configs SET
            name                          = COALESCE(?3, name),
            description                   = COALESCE(?4, description),
            state_rate                    = COALESCE(?5, state_rate),
            capital_gains_rate            = COALESCE(?6, capital_gains_rate),
            early_withdrawal_penalty_rate = COALESCE(?7, early_withdrawal_penalty_rate),
            standard_deduction            = COALESCE(?8, standard_deduction),
            age_65_extra_deduction        = COALESCE(?9, age_65_extra_deduction),
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
    .bind(body.standard_deduction)
    .bind(body.age_65_extra_deduction)
    .execute(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, NAME_TAKEN))?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("tax config"));
    }
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

    sqlx::query("UPDATE scenarios SET updated_at = datetime('now') WHERE tax_config_id = ?1 AND user_id = ?2")
        .bind(id).bind(&user.id).execute(&mut *tx).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::TaxConfig,
        Operation::Updated,
        &EventFields {
            user_id: Some(&user.id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );

    Ok(Json(load(&state, id, &user.id).await?))
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE scenarios SET updated_at = datetime('now') WHERE tax_config_id = ?1 AND user_id = ?2")
        .bind(id).bind(&user.id).execute(&mut *tx).await?;
    let affected = sqlx::query("DELETE FROM tax_configs WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("tax config"));
    }
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::TaxConfig,
        Operation::Deleted,
        &EventFields {
            user_id: Some(&user.id),
            resource_id: Some(id),
            ..Default::default()
        },
    );

    Ok(StatusCode::NO_CONTENT)
}

impl ActivityFields for CreateTaxConfig {
    const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "state_rate",
        "capital_gains_rate",
        "early_withdrawal_penalty_rate",
        "standard_deduction",
        "age_65_extra_deduction",
        "federal_brackets",
    ];
}

impl ActivityFields for UpdateTaxConfig {
    const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "state_rate",
        "capital_gains_rate",
        "early_withdrawal_penalty_rate",
        "standard_deduction",
        "age_65_extra_deduction",
        "federal_brackets",
    ];
}
