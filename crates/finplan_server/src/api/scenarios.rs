//! Scenario CRUD, plus a compile-check endpoint.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use crate::auth::activity::{ActivityFields, Submitted};
use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::observability::{EventFields, Operation, Resource};
use crate::state::AppState;
use finplan_plan::specs::scenarios::{CompileReport, Scenario, ScenarioStatus};
use ts_rs::TS;

use finplan_plan::specs::scenarios::{CreateScenario, SetFunding, UpdateScenario, validate_date};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios", get(list).post(create))
        .route("/scenarios/{id}", get(fetch).patch(update).delete(destroy))
        .route("/scenarios/{id}/funding", put(set_funding))
        .route("/scenarios/{id}/duplicate", post(duplicate))
        .route("/scenarios/{id}/compile", post(compile_check))
}

/// `last_run_at` and `last_success_rate` describe the scenario's latest
/// successful run; failed or pending runs do not replace it. `funding` is the
/// funding policy as one JSON object (decoded by `Scenario`), null when off.
pub(crate) const SCENARIO_COLUMNS: &str =
    "id, slug, name, description, start_date, birth_date, duration_years,
     inflation_profile_id, tax_config_id, collect_ledger, status, created_at, updated_at,
     (SELECT r.finished_at FROM runs r
       WHERE r.scenario_id = scenarios.id AND r.status = 'succeeded'
       ORDER BY r.finished_at DESC LIMIT 1) AS last_run_at,
     (SELECT st.success_rate FROM run_stats st JOIN runs r ON r.id = st.run_id
       WHERE r.scenario_id = scenarios.id AND r.status = 'succeeded'
       ORDER BY r.finished_at DESC LIMIT 1) AS last_success_rate,
     CASE WHEN funding_strategy IS NULL THEN NULL ELSE json_object(
       'strategy', funding_strategy,
       'bracket_ceiling', funding_bracket_ceiling,
       'exclude_accounts', (SELECT json_group_array(account_id) FROM
         (SELECT account_id FROM scenario_funding_excludes
           WHERE scenario_id = scenarios.id ORDER BY account_id))) END AS funding";

async fn list(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Vec<Scenario>>> {
    let rows: Vec<Scenario> = sqlx::query_as(&format!(
        "SELECT {SCENARIO_COLUMNS} FROM scenarios
          WHERE user_id = ?1 AND status = 'active' ORDER BY updated_at DESC"
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
    let mut conn = state.db.acquire().await?;
    let (start_date, birth_date) = check_new(&mut conn, &user.id, &body).await?;
    drop(conn);

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    crate::billing::check_plan_slot(&mut tx, &user.id, &state.config, 1).await?;
    let id = insert_new(&mut tx, &user.id, &body, &start_date, birth_date.as_deref()).await?;

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

/// What `POST /scenarios` checks before it takes a plan slot: the assumptions
/// are the caller's, the dates parse, the name is not blank. Returns the start
/// and birth dates as stored.
pub(crate) async fn check_new(
    conn: &mut sqlx::SqliteConnection,
    user_id: &str,
    body: &CreateScenario,
) -> ApiResult<(String, Option<String>)> {
    owned_assumptions(conn, user_id, body.inflation_profile_id, body.tax_config_id).await?;
    let start_date = validate_date(&body.start_date, "start_date")?;
    let birth_date = body
        .birth_date
        .as_deref()
        .map(|d| validate_date(d, "birth_date"))
        .transpose()?;

    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("scenario name cannot be empty"));
    }
    Ok((start_date, birth_date))
}

/// Insert the scenario row `POST /scenarios` creates, from what [`check_new`]
/// returned; returns its id.
pub(crate) async fn insert_new(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    body: &CreateScenario,
    start_date: &str,
    birth_date: Option<&str>,
) -> ApiResult<i64> {
    sqlx::query_scalar(
        "INSERT INTO scenarios
            (user_id, name, description, start_date, birth_date, duration_years,
             inflation_profile_id, tax_config_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8) RETURNING id",
    )
    .bind(user_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(start_date)
    .bind(birth_date)
    .bind(body.duration_years)
    .bind(body.inflation_profile_id)
    .bind(body.tax_config_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, "a scenario with that name already exists"))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(Submitted { body, fields }): Json<Submitted<UpdateScenario>>,
) -> ApiResult<Json<Scenario>> {
    super::owned_scenario(&state.db, id, &user.id).await?;
    let mut conn = state.db.acquire().await?;
    update_in(&mut conn, &user.id, id, &body).await?;
    drop(conn);

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

/// Apply a settings update, as `PATCH /scenarios/{id}` does: name, dates,
/// horizon and the assumptions the plan uses. Fields the body omits are left
/// alone. The suggestion path writes through this too, inside its transaction.
pub(crate) async fn update_in(
    conn: &mut sqlx::SqliteConnection,
    user_id: &str,
    id: i64,
    body: &UpdateScenario,
) -> ApiResult<()> {
    body.check_name()?;
    body.check_duration()?;
    owned_assumptions(conn, user_id, body.inflation_profile_id, body.tax_config_id).await?;
    let (start_date, birth_date) = body.dates()?;

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
    .execute(&mut *conn)
    .await
    .map_err(|e| on_unique_violation(e, "a scenario with that name already exists"))?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::NotFound("scenario"));
    }
    Ok(())
}

/// `PUT /scenarios/{id}/funding`: load the graph, let the plan crate check and
/// apply the edit, write what it changed.
async fn set_funding(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(Submitted { body, fields }): Json<Submitted<SetFunding>>,
) -> ApiResult<Json<Scenario>> {
    super::owned_scenario(&state.db, id, &user.id).await?;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    set_funding_in(&mut tx, &user.id, id, &body).await?;
    tx.commit().await?;
    super::touch_scenario(&state.db, id).await?;

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

/// Apply `PUT /scenarios/{id}/funding`: the plan crate checks the policy
/// against the stored plan and says what it becomes, and that is written.
pub(crate) async fn set_funding_in(
    conn: &mut sqlx::SqliteConnection,
    user_id: &str,
    id: i64,
    body: &SetFunding,
) -> ApiResult<()> {
    let mut graph = crate::db::graph::load_connection(conn, id, user_id).await?;
    finplan_plan::edit::set_funding(&mut graph, body)?;
    write_funding(conn, id, &graph).await?;
    if body.align_sweeps == Some(true) && body.funding.is_some() {
        // The edit already set the rows in the graph; mirror them.
        for row in graph
            .withdrawal_sources
            .values()
            .filter(|row| row.mode == "Strategy")
        {
            sqlx::query(
                "UPDATE effect_withdrawal_sources SET strategy = ?2, bracket_ceiling = ?3
                 WHERE effect_id = ?1",
            )
            .bind(row.effect_id)
            .bind(&row.strategy)
            .bind(row.bracket_ceiling)
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

/// Store the graph's funding policy on scenario `id`: the two columns and the
/// excludes, replacing what was there.
pub(crate) async fn write_funding(
    conn: &mut sqlx::SqliteConnection,
    id: i64,
    graph: &finplan_plan::graph::ScenarioGraph,
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE scenarios SET funding_strategy = ?2, funding_bracket_ceiling = ?3 WHERE id = ?1",
    )
    .bind(id)
    .bind(&graph.scenario.funding_strategy)
    .bind(graph.scenario.funding_bracket_ceiling)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM scenario_funding_excludes WHERE scenario_id = ?1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    for account_id in &graph.funding_excludes {
        sqlx::query(
            "INSERT INTO scenario_funding_excludes (scenario_id, account_id) VALUES (?1, ?2)",
        )
        .bind(id)
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let deleted: Option<ScenarioStatus> =
        sqlx::query_scalar("DELETE FROM scenarios WHERE id = ?1 AND user_id = ?2 RETURNING status")
            .bind(id)
            .bind(&user.id)
            .fetch_optional(&state.db)
            .await?;

    let Some(status) = deleted else {
        return Err(ApiError::NotFound("scenario"));
    };
    // A draft deleted this way is no different from one cancelled: its running
    // agent stops and the originals held for it go now, not at the next sweep.
    if status == ScenarioStatus::Draft {
        super::draft_agent::abort(&state, id);
        crate::documents::images::discard_draft(&crate::documents::images::root(&state.config), id);
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
    let graph = crate::db::graph::load(&state.db, id, &user.id).await?;
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    crate::billing::check_plan_slot(&mut tx, &user.id, &state.config, 1).await?;
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

/// Lower the scenario without running it. Lets the UI surface configuration
/// errors before a user commits to a long Monte Carlo run.
async fn compile_check(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<CompileReport>> {
    let graph = crate::db::graph::load(&state.db, id, &user.id).await?;
    Ok(Json(finplan_plan::read::compile_report(&graph)?))
}

async fn owned_assumptions(
    conn: &mut sqlx::SqliteConnection,
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
            .fetch_one(&mut *conn)
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

impl ActivityFields for SetFunding {
    const FIELDS: &'static [&'static str] = &["funding", "align_sweeps"];
}

impl ActivityFields for DuplicateRequest {
    const FIELDS: &'static [&'static str] = &["name"];
}
