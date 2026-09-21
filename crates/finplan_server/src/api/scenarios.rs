//! Scenario CRUD, plus a compile-check endpoint.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::activity::{ActivityFields, Submitted};
use crate::auth::session::CurrentUser;
use crate::compile::{self, rows::ScenarioGraph};
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::observability::{EventFields, Operation, Resource};
use crate::state::AppState;
use ts_rs::TS;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios", get(list).post(create))
        .route("/scenarios/{id}", get(fetch).patch(update).delete(destroy))
        .route("/scenarios/{id}/duplicate", post(duplicate))
        .route("/scenarios/{id}/compile", post(compile_check))
}

#[derive(Debug, Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct Scenario {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub start_date: String,
    pub birth_date: Option<String>,
    pub duration_years: i64,
    pub inflation_profile_id: Option<i64>,
    pub tax_config_id: Option<i64>,
    pub collect_ledger: bool,
    pub created_at: String,
    pub updated_at: String,
    /// When this scenario last produced results, and what they said. Carried
    /// on the row so a list of scenarios can be shown with its own history
    /// without a request per scenario.
    pub last_run_at: Option<String>,
    pub last_success_rate: Option<f64>,
}

/// The trailing two columns describe the scenario's run, and only a succeeded
/// one: a scenario holds a single run (see `0008_one_run_per_scenario.sql`), so
/// where that run failed the card shows no figure rather than a stale one.
const SCENARIO_COLUMNS: &str = "id, name, description, start_date, birth_date, duration_years,
     inflation_profile_id, tax_config_id, collect_ledger, created_at, updated_at,
     (SELECT r.finished_at FROM runs r
       WHERE r.scenario_id = scenarios.id AND r.status = 'succeeded'
       ORDER BY r.finished_at DESC LIMIT 1) AS last_run_at,
     (SELECT st.success_rate FROM run_stats st JOIN runs r ON r.id = st.run_id
       WHERE r.scenario_id = scenarios.id AND r.status = 'succeeded'
       ORDER BY r.finished_at DESC LIMIT 1) AS last_success_rate";

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateScenario {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub start_date: String,
    #[serde(default)]
    pub birth_date: Option<String>,
    #[serde(default = "default_duration")]
    pub duration_years: i64,
    #[serde(default)]
    pub inflation_profile_id: Option<i64>,
    #[serde(default)]
    pub tax_config_id: Option<i64>,
}

fn default_duration() -> i64 {
    30
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateScenario {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub birth_date: Option<String>,
    #[serde(default)]
    pub duration_years: Option<i64>,
    #[serde(default)]
    pub inflation_profile_id: Option<i64>,
    #[serde(default)]
    pub tax_config_id: Option<i64>,
    #[serde(default)]
    pub collect_ledger: Option<bool>,
}

fn validate_date(text: &str, field: &str) -> ApiResult<String> {
    text.parse::<jiff::civil::Date>()
        .map(|d| d.to_string())
        .map_err(|e| ApiError::bad_request(format!("invalid {field} '{text}': {e}")))
}

async fn list(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Vec<Scenario>>> {
    let rows: Vec<Scenario> = sqlx::query_as(&format!(
        "SELECT {SCENARIO_COLUMNS} FROM scenarios WHERE user_id = ?1 ORDER BY updated_at DESC"
    ))
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn fetch(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Scenario>> {
    let row: Option<Scenario> = sqlx::query_as(&format!(
        "SELECT {SCENARIO_COLUMNS} FROM scenarios WHERE id = ?1 AND user_id = ?2"
    ))
    .bind(id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;
    row.map(Json).ok_or(ApiError::NotFound("scenario"))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(Submitted { body, fields }): Json<Submitted<CreateScenario>>,
) -> ApiResult<(StatusCode, Json<Scenario>)> {
    owned_assumptions(
        &state,
        &user.id,
        body.inflation_profile_id,
        body.tax_config_id,
    )
    .await?;
    let start_date = validate_date(&body.start_date, "start_date")?;
    let birth_date = body
        .birth_date
        .as_deref()
        .map(|d| validate_date(d, "birth_date"))
        .transpose()?;

    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("scenario name cannot be empty"));
    }

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    crate::billing::check_plan_slot(&mut tx, &user.id, state.config.hosted, 1).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO scenarios
            (user_id, name, description, start_date, birth_date, duration_years,
             inflation_profile_id, tax_config_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8) RETURNING id",
    )
    .bind(&user.id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(&start_date)
    .bind(&birth_date)
    .bind(body.duration_years)
    .bind(body.inflation_profile_id)
    .bind(body.tax_config_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| on_unique_violation(e, "a scenario with that name already exists"))?;

    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Scenario,
        Operation::Created,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );

    let row: Scenario = sqlx::query_as(&format!(
        "SELECT {SCENARIO_COLUMNS} FROM scenarios WHERE id = ?1"
    ))
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(row)))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(Submitted { body, fields }): Json<Submitted<UpdateScenario>>,
) -> ApiResult<Json<Scenario>> {
    super::owned_scenario(&state.db, id, &user.id).await?;
    if body
        .name
        .as_deref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(ApiError::bad_request("scenario name cannot be empty"));
    }
    owned_assumptions(
        &state,
        &user.id,
        body.inflation_profile_id,
        body.tax_config_id,
    )
    .await?;

    let start_date = body
        .start_date
        .as_deref()
        .map(|d| validate_date(d, "start_date"))
        .transpose()?;
    let birth_date = body
        .birth_date
        .as_deref()
        .map(|d| validate_date(d, "birth_date"))
        .transpose()?;

    // COALESCE leaves any field the caller omitted untouched.
    let affected = sqlx::query(
        "UPDATE scenarios SET
            name                 = COALESCE(?2, name),
            description          = COALESCE(?3, description),
            start_date           = COALESCE(?4, start_date),
            birth_date           = COALESCE(?5, birth_date),
            duration_years       = COALESCE(?6, duration_years),
            inflation_profile_id = COALESCE(?7, inflation_profile_id),
            tax_config_id        = COALESCE(?8, tax_config_id),
            collect_ledger       = COALESCE(?9, collect_ledger),
            updated_at           = datetime('now')
          WHERE id = ?1",
    )
    .bind(id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(&body.description)
    .bind(&start_date)
    .bind(&birth_date)
    .bind(body.duration_years)
    .bind(body.inflation_profile_id)
    .bind(body.tax_config_id)
    .bind(body.collect_ledger.map(i64::from))
    .execute(&state.db)
    .await
    .map_err(|e| on_unique_violation(e, "a scenario with that name already exists"))?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("scenario"));
    }

    state.telemetry.mutation(
        Resource::Scenario,
        Operation::Updated,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );
    let row: Scenario = sqlx::query_as(&format!(
        "SELECT {SCENARIO_COLUMNS} FROM scenarios WHERE id = ?1"
    ))
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(row))
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let affected = sqlx::query("DELETE FROM scenarios WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(&user.id)
        .execute(&state.db)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("scenario"));
    }

    state.telemetry.mutation(
        Resource::Scenario,
        Operation::Deleted,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(id),
            resource_id: Some(id),
            ..Default::default()
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct DuplicateRequest {
    pub name: String,
}

/// Deep-copy a scenario, remapping every internal foreign key to the new rows.
///
/// The copy is done id-by-id rather than with a bulk `INSERT ... SELECT`
/// because assets, accounts, events, triggers, amounts and effects all point at
/// each other; a table-at-a-time copy would leave the clone referencing the
/// original's rows.
async fn duplicate(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(Submitted { body, fields }): Json<Submitted<DuplicateRequest>>,
) -> ApiResult<(StatusCode, Json<Scenario>)> {
    let graph = ScenarioGraph::load(&state.db, id, &user.id).await?;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    crate::billing::check_plan_slot(&mut tx, &user.id, state.config.hosted, 1).await?;
    let new_id = crate::domain::clone_into(&mut tx, &graph, body.name.trim()).await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::Scenario,
        Operation::Duplicated,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(new_id),
            resource_id: Some(new_id),
            fields: &fields,
            count: Some(
                (1 + graph.accounts.len() + graph.assets.len() + graph.events.len()) as u64,
            ),
            ..Default::default()
        },
    );

    let row: Scenario = sqlx::query_as(&format!(
        "SELECT {SCENARIO_COLUMNS} FROM scenarios WHERE id = ?1"
    ))
    .bind(new_id)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(row)))
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CompileReport {
    pub ok: bool,
    pub accounts: usize,
    pub assets: usize,
    pub events: usize,
    pub return_profiles: usize,
    pub duration_years: usize,
}

/// Lower the scenario without running it. Lets the UI surface configuration
/// errors before a user commits to a long Monte Carlo run.
async fn compile_check(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<CompileReport>> {
    let graph = ScenarioGraph::load(&state.db, id, &user.id).await?;
    let compiled = compile::compile(&graph)?;

    Ok(Json(CompileReport {
        ok: true,
        accounts: compiled.config.accounts.len(),
        assets: compiled.config.asset_prices.len(),
        events: compiled.config.events.len(),
        return_profiles: compiled.config.return_profiles.len(),
        duration_years: compiled.config.duration_years,
    }))
}

async fn owned_assumptions(
    state: &AppState,
    user: &str,
    inflation: Option<i64>,
    tax: Option<i64>,
) -> ApiResult<()> {
    for (table, id) in [("inflation_profiles", inflation), ("tax_configs", tax)] {
        if let Some(id) = id {
            let found: bool = sqlx::query_scalar(&format!(
                "SELECT EXISTS(SELECT 1 FROM {table} WHERE id=? AND user_id=?)"
            ))
            .bind(id)
            .bind(user)
            .fetch_one(&state.db)
            .await?;
            if !found {
                return Err(ApiError::bad_request(
                    "assumption must belong to your account",
                ));
            }
        }
    }
    Ok(())
}

impl ActivityFields for CreateScenario {
    const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "start_date",
        "birth_date",
        "duration_years",
        "inflation_profile_id",
        "tax_config_id",
    ];
}

impl ActivityFields for UpdateScenario {
    const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "start_date",
        "birth_date",
        "duration_years",
        "inflation_profile_id",
        "tax_config_id",
        "collect_ledger",
    ];
}

impl ActivityFields for DuplicateRequest {
    const FIELDS: &'static [&'static str] = &["name"];
}
