//! What-if: an ordered stack of override layers over a saved plan.
//!
//! The stack itself is a client document stored whole per scenario (see
//! `what_if_stacks`), the same bargain the sweep layout makes. Running it is an
//! analysis job (`POST /scenarios/{id}/analyses` with `kind: "what-if"`), and
//! applying it writes the layers into the plan for real: parameter values are
//! set, and shocks and one-offs become fires-once age events.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::analysis::jobs::{InlineFailure, run_inline};
use crate::analysis::params::parameters;
use crate::analysis::results::{AnalysisOutcome, WhatIfOutcome};
use crate::api::analysis::CreateAnalysis;
use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::observability::{JobKind as MetricKind, Origin};
use crate::runner::telemetry::Submission;
use crate::state::AppState;
use finplan_core::analysis::SweepProgress;
use finplan_plan::compile;
use finplan_plan::specs::events::EventBody;
use finplan_plan::specs::scenarios::Scenario;
pub use finplan_plan::what_if::{
    ApplyWhatIf, MAX_LAYERS, QuickWhatIf, WhatIfEntry, WhatIfLayer, WhatIfStack,
};
use finplan_plan::what_if::{Resolved, quick_iterations, resolve, value_columns};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/scenarios/{scenario_id}/what-if",
            get(load_stack).put(save_stack),
        )
        .route("/scenarios/{scenario_id}/what-if/apply", post(apply))
        .route("/scenarios/{scenario_id}/what-if/quick", post(quick))
}

/// Cancels the engine when the request goes away.
///
/// A client that has moved on aborts its fetch, axum drops this handler's
/// future, and the drop flags the worker to stop rather than finishing an
/// answer nobody will read.
struct CancelOnDrop(SweepProgress);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// `POST /scenarios/{id}/what-if/quick`: the quick pass of a what-if, run
/// inline and returned directly — one round trip instead of start, poll and
/// fetch. Same checks and lowering as the job route; the refinement still
/// goes through `POST /scenarios/{id}/analyses`.
async fn quick(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<QuickWhatIf>,
) -> ApiResult<Json<WhatIfOutcome>> {
    let mut decision = Submission::new(&state.telemetry, MetricKind::WhatIf);
    let result = run_quick(&state, &user, scenario_id, body, &mut decision).await;
    decision.result(&result);
    result
}

async fn run_quick(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    body: QuickWhatIf,
    decision: &mut Submission,
) -> ApiResult<Json<WhatIfOutcome>> {
    let cap = crate::billing::iteration_cap(
        &crate::billing::entitlements(&state.db, &user.id, &state.config).await?,
        &state.config,
    );
    let iterations = quick_iterations(body.iterations, cap);
    let prepared = super::analysis::prepare(
        state,
        user,
        scenario_id,
        CreateAnalysis::WhatIf {
            layers: body.layers,
            iterations: Some(iterations),
        },
    )
    .await?;
    let permit = crate::billing::admit_compute_observed(
        &user.id,
        prepared.tier,
        &state.telemetry,
        Origin::Request,
    )?;
    decision.accepted();

    let progress = SweepProgress::new(prepared.spec.budget());
    let guard = CancelOnDrop(progress.clone());
    let outcome = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        run_inline(&prepared.base, &prepared.spec, &progress)
    })
    .await;
    drop(guard);
    match outcome {
        Ok(Ok(AnalysisOutcome::WhatIf(outcome))) => Ok(Json(outcome)),
        Ok(Err(InlineFailure::Cancelled)) => Err(ApiError::Conflict("what-if canceled".into())),
        Ok(Ok(_)) | Ok(Err(InlineFailure::Failed)) | Err(_) => {
            Err(ApiError::Internal("what-if failed".into()))
        }
    }
}

// ── the stored stack ────────────────────────────────────────────────────────

async fn load_stack(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<WhatIfStack>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let row: Option<String> = sqlx::query_scalar(
        "SELECT stack FROM what_if_stacks WHERE scenario_id = ?1 AND user_id = ?2",
    )
    .bind(scenario_id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;
    // A stack written by an older client that no longer parses is treated as
    // no stack: the screen starts empty, which is where it would start anyway.
    Ok(Json(
        row.and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
    ))
}

async fn save_stack(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<WhatIfStack>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    body.validate()?;
    let json =
        serde_json::to_string(&body).map_err(|_| ApiError::internal("unserializable stack"))?;
    sqlx::query(
        "INSERT INTO what_if_stacks (scenario_id, user_id, stack, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(scenario_id) DO UPDATE
            SET user_id = excluded.user_id,
                stack = excluded.stack,
                updated_at = excluded.updated_at",
    )
    .bind(scenario_id)
    .bind(&user.id)
    .bind(json)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── apply ───────────────────────────────────────────────────────────────────

/// Write the layers into the plan: this scenario, or a fresh copy of it.
async fn apply(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ApplyWhatIf>,
) -> ApiResult<Json<Scenario>> {
    let graph = crate::db::graph::load(&state.db, scenario_id, &user.id).await?;
    let compiled = compile::compile(&graph)?;
    let available = parameters(&compiled);
    let resolved = resolve(&graph, &available, &body.layers)?;
    let new_name = body
        .new_scenario_name
        .as_deref()
        .map(str::trim)
        .map(str::to_string);

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let (target, maps) = match &new_name {
        Some(name) => {
            crate::billing::check_plan_slot(&mut tx, &user.id, &state.config, 1).await?;
            let cloned = crate::domain::clone_into_mapped(&mut tx, &graph, name).await?;
            (cloned.id, Some(cloned))
        }
        None => (scenario_id, None),
    };
    let account =
        |id: i64| -> finplan_plan::PlanResult<i64> {
            match &maps {
                Some(maps) => maps.accounts.get(&id).copied().ok_or_else(|| {
                    finplan_plan::PlanError::internal("account missing from the copy")
                }),
                None => Ok(id),
            }
        };
    let parameter = |id: i64| -> ApiResult<i64> {
        match &maps {
            Some(maps) => maps
                .parameters
                .get(&id)
                .copied()
                .ok_or_else(|| ApiError::internal("parameter missing from the copy")),
            None => Ok(id),
        }
    };

    let mut names: Vec<String> = graph.events.iter().map(|e| e.name.clone()).collect();
    for layer in &resolved {
        match layer {
            // Applied in order, so a later layer on the same parameter wins.
            Resolved::Parameter { param, value } => {
                let (kind, number, date, years, months) = value_columns(value);
                sqlx::query(
                    "UPDATE named_parameters
                        SET kind=?1, number_value=?2, date_value=?3, age_years=?4, age_months=?5
                      WHERE id=?6 AND scenario_id=?7",
                )
                .bind(kind)
                .bind(number)
                .bind(date)
                .bind(years)
                .bind(months)
                .bind(parameter(param.parameter_id)?)
                .bind(target)
                .execute(&mut *tx)
                .await?;
            }
            other => {
                if let Some(body) = other.event_body(&mut names, account)? {
                    insert_event(&mut tx, target, &body).await?;
                }
            }
        }
    }

    if new_name.is_none() {
        // The plan now says what the stack said; keeping the stack would
        // apply it twice.
        sqlx::query("DELETE FROM what_if_stacks WHERE scenario_id = ?1")
            .bind(target)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    super::touch_scenario(&state.db, target).await?;

    let row: Scenario = sqlx::query_as(&format!(
        "SELECT {} FROM scenarios WHERE id = ?1",
        super::scenarios::SCENARIO_COLUMNS
    ))
    .bind(target)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(row))
}

/// Append a fires-once age event to the end of the scenario's list.
async fn insert_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scenario_id: i64,
    body: &EventBody,
) -> ApiResult<()> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO events (scenario_id, name, description, fires_once, enabled, sort_order)
         VALUES (?1,?2,?3,1,1,
                 (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM events WHERE scenario_id = ?1))
         RETURNING id",
    )
    .bind(scenario_id)
    .bind(&body.name)
    .bind(&body.description)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| on_unique_violation(e, "an event with that name already exists"))?;
    super::events::write_tree(tx, scenario_id, id, body).await
}
