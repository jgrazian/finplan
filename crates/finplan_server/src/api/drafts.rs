//! AI-guided scenario drafts: the lifecycle around a plan the model is still
//! writing.
//!
//! A draft is an ordinary scenario row with `status = 'draft'`, so loading,
//! resolving and applying changes, suggestions and chat all work on it
//! unchanged (its suggestions carry no run: see `suggestions`). What makes it a
//! draft is only where it is hidden: from the scenario list, from plan-slot
//! counts, and from `editable_plans`, until Create & run turns it into a plan.
//!
//! Drafts are not resumable. A user has at most one; starting another discards
//! the old one, cancelling deletes it, and [`sweep_stale`] deletes any left
//! untouched for the configured TTL (a closed tab, a crash). Its id in these
//! routes is its scenario's id.
//!
//! A draft's documents (`api::documents`) live and die with it: deleting the
//! draft deletes them (a foreign-key cascade) and every path that deletes a
//! draft also removes its held originals (`documents::images`). Create & run
//! deletes them too unless the draft's `retain_documents` is set.
//!
//! Later steps extend [`DraftStatus`] (documents, questions, blocked notes) and
//! fill the draft through `apply_steps_sql`; this module owns only the
//! lifecycle and the quota.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::runs::{self, Run};
use super::scenarios::{SCENARIO_COLUMNS, Scenario};
use crate::auth::session::CurrentUser;
use crate::compile::{self, rows::ScenarioGraph};
use crate::db::Db;
use crate::documents;
use crate::error::{ApiError, ApiResult};
use crate::observability::{EventFields, Operation, Resource};
use crate::runner::telemetry::Submission;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/drafts", post(start))
        .route("/drafts/{id}", get(status).patch(update).delete(discard))
        .route("/drafts/{id}/create", post(create_and_run))
}

/// Where a draft stands. Only `ready` exists until the drafting agent lands;
/// it will add states for a model still writing and one waiting on answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DraftState {
    Ready,
}

/// What the draft holds so far, for the live "3 accounts, 6 events" line.
#[derive(Debug, Clone, Default, Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct DraftCounts {
    pub accounts: i64,
    pub assets: i64,
    pub events: i64,
    pub parameters: i64,
    /// Notes still open on the draft.
    pub open_suggestions: i64,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DraftStatus {
    /// The draft's scenario id.
    pub id: i64,
    pub state: DraftState,
    pub scenario: Scenario,
    /// When the sweeper deletes the draft unless it is touched first.
    pub expires_at: String,
    pub counts: DraftCounts,
    /// Whether the documents are kept with the plan once it is created,
    /// instead of deleted (2a's retention choice).
    pub retain_documents: bool,
    pub document_count: i64,
}

/// The body of `POST /drafts`, all of it optional.
#[derive(Debug, Default, Deserialize, TS)]
#[ts(export)]
pub struct StartDraft {
    /// Keep the documents with the plan created from this draft. Default: delete them.
    #[serde(default)]
    pub retain_documents: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct UpdateDraft {
    pub retain_documents: Option<bool>,
}

/// `POST /drafts` — start a draft. Checks the plan slot first, so no draft is
/// spent on a plan that could not be created; discards any draft the user
/// already has; spends one of the month's drafts; and inserts an empty
/// scenario whose start date is today (UTC), set here and never by the model.
async fn start(
    State(state): State<AppState>,
    user: CurrentUser,
    body: Option<Json<StartDraft>>,
) -> ApiResult<(StatusCode, Json<DraftStatus>)> {
    let retain_documents = body.is_some_and(|Json(b)| b.retain_documents);
    if state.review_ai.is_none() {
        return Err(ApiError::Conflict(
            "AI drafts are not available on this server".into(),
        ));
    }
    let pro = crate::billing::entitlements(&state.db, &user.id, &state.config)
        .await?
        .pro;
    let limits = state.config.draft.limits(pro);

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    crate::billing::check_plan_slot(&mut tx, &user.id, &state.config, 1).await?;
    crate::billing::reserve_ai_draft(&mut tx, &user.id, limits.drafts_per_month).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO scenarios (user_id, name, start_date, status, retain_documents)
         VALUES (?1, 'AI draft ' || lower(hex(randomblob(4))), date('now'), 'draft', ?2)
         RETURNING id",
    )
    .bind(&user.id)
    .bind(i64::from(retain_documents))
    .fetch_one(&mut *tx)
    .await?;
    // After the insert, so the new draft never takes the old one's id: a tab
    // still holding that id finds nothing, not somebody else's draft.
    let replaced: Vec<i64> = sqlx::query_scalar(
        "DELETE FROM scenarios WHERE user_id = ?1 AND status = 'draft' AND id <> ?2 RETURNING id",
    )
    .bind(&user.id)
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    // The old draft's documents went with its row; its held originals go now.
    // The new id's folder is cleared too, in case an id was reused.
    let files = documents::images::root(&state.config);
    for old in replaced.iter().chain([&id]) {
        documents::images::discard_draft(&files, *old);
    }

    state.telemetry.mutation(
        Resource::Scenario,
        Operation::Created,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(id),
            resource_id: Some(id),
            ..Default::default()
        },
    );
    // A second start racing this one may already have replaced the draft.
    let status = match load_status(&state, id, &user.id).await {
        Err(ApiError::NotFound(_)) => {
            return Err(ApiError::Conflict(
                "this draft was replaced by a newer one".into(),
            ));
        }
        other => other?,
    };
    Ok((StatusCode::CREATED, Json(status)))
}

/// `GET /drafts/{id}` — the draft's state, contents so far and expiry.
async fn status(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<DraftStatus>> {
    Ok(Json(load_status(&state, id, &user.id).await?))
}

async fn load_status(state: &AppState, id: i64, user_id: &str) -> ApiResult<DraftStatus> {
    let scenario: Scenario = sqlx::query_as(&format!(
        "SELECT {SCENARIO_COLUMNS} FROM scenarios
          WHERE id = ?1 AND user_id = ?2 AND status = 'draft'"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound("draft"))?;
    let counts: DraftCounts = sqlx::query_as(
        "SELECT (SELECT count(*) FROM accounts WHERE scenario_id = ?1) AS accounts,
                (SELECT count(*) FROM assets WHERE scenario_id = ?1) AS assets,
                (SELECT count(*) FROM events WHERE scenario_id = ?1) AS events,
                (SELECT count(*) FROM named_parameters WHERE scenario_id = ?1) AS parameters,
                (SELECT count(*) FROM suggestions
                  WHERE scenario_id = ?1 AND status = 'open') AS open_suggestions",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let expires_at: String = sqlx::query_scalar(
        "SELECT datetime(updated_at, '+' || ?2 || ' hours') FROM scenarios WHERE id = ?1",
    )
    .bind(id)
    .bind(i64::from(state.config.draft.ttl_hours))
    .fetch_one(&state.db)
    .await?;
    let (retain_documents, document_count): (i64, i64) = sqlx::query_as(
        "SELECT retain_documents, (SELECT count(*) FROM documents WHERE scenario_id = ?1)
           FROM scenarios WHERE id = ?1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(DraftStatus {
        id,
        state: DraftState::Ready,
        scenario,
        expires_at,
        counts,
        retain_documents: retain_documents != 0,
        document_count,
    })
}

/// `PATCH /drafts/{id}` — change the draft's retention choice: whether its
/// documents are kept with the plan when it is created. Applies to documents
/// already attached and to those attached later.
async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateDraft>,
) -> ApiResult<Json<DraftStatus>> {
    // A draft that is not the caller's, or is no longer a draft, is not found.
    load_status(&state, id, &user.id).await?;
    if let Some(retain) = body.retain_documents {
        let mut tx = state.db.begin().await?;
        sqlx::query(
            "UPDATE scenarios SET retain_documents = ?3, updated_at = datetime('now')
              WHERE id = ?1 AND user_id = ?2 AND status = 'draft'",
        )
        .bind(id)
        .bind(&user.id)
        .bind(i64::from(retain))
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE documents SET retain = ?2 WHERE scenario_id = ?1")
            .bind(id)
            .bind(i64::from(retain))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }
    Ok(Json(load_status(&state, id, &user.id).await?))
}

/// `DELETE /drafts/{id}` — cancel: delete the draft and everything under it
/// (rows, suggestions, its documents and their held originals) at once.
async fn discard(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let affected =
        sqlx::query("DELETE FROM scenarios WHERE id = ?1 AND user_id = ?2 AND status = 'draft'")
            .bind(id)
            .bind(&user.id)
            .execute(&state.db)
            .await?
            .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound("draft"));
    }
    documents::images::discard_draft(&documents::images::root(&state.config), id);
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

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DraftCreated {
    pub scenario: Scenario,
    /// The queued run. A review starts, as for any plan, once it has
    /// succeeded (`POST /scenarios/{id}/review`), because notes are written
    /// against a finished run.
    pub run: Run,
}

/// `POST /drafts/{id}/create` — Create & run: check the plan slot, make the
/// draft a plan (`status = 'active'`) and queue its run the way any run is
/// queued. A draft that does not compile is refused while it is still a draft;
/// if the run cannot be queued (capacity), the draft stays a draft so the
/// request can be retried.
async fn create_and_run(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<(StatusCode, Json<DraftCreated>)> {
    // A draft that cannot run says why now, before it stops being a draft.
    let graph = match ScenarioGraph::load(&state.db, id, &user.id).await {
        Ok(graph) if super::is_draft(&state.db, id).await? => graph,
        Ok(_) | Err(ApiError::NotFound(_)) => return Err(ApiError::NotFound("draft")),
        Err(err) => return Err(err),
    };
    compile::compile(&graph)?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    crate::billing::check_plan_slot(&mut tx, &user.id, &state.config, 1).await?;
    let promoted = sqlx::query(
        "UPDATE scenarios SET status = 'active', updated_at = datetime('now')
          WHERE id = ?1 AND user_id = ?2 AND status = 'draft'",
    )
    .bind(id)
    .bind(&user.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if promoted == 0 {
        return Err(ApiError::NotFound("draft"));
    }
    tx.commit().await?;

    let mut decision = Submission::new(&state.telemetry, crate::observability::JobKind::Run);
    let queued =
        runs::create_run(&state, &user, id, runs::CreateRun::default(), &mut decision).await;
    decision.result(&queued);
    let run = match queued {
        Ok((_, Json(run))) => run,
        Err(err) => {
            demote(&state.db, id, &user.id).await;
            return Err(err);
        }
    };
    // The plan is made: its documents are deleted unless kept, and the held
    // originals always are.
    documents::finish_draft(&state.db, &state.config, id).await?;
    let scenario: Scenario = sqlx::query_as(&format!(
        "SELECT {SCENARIO_COLUMNS} FROM scenarios WHERE id = ?1"
    ))
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(DraftCreated { scenario, run })))
}

/// Put a plan whose first run could not be queued back to a draft.
async fn demote(db: &Db, id: i64, user_id: &str) {
    if let Err(err) = sqlx::query(
        "UPDATE scenarios SET status = 'draft' WHERE id = ?1 AND user_id = ?2 AND status = 'active'",
    )
    .bind(id)
    .bind(user_id)
    .execute(db)
    .await
    {
        tracing::warn!(event = "draft.demote_failed", scenario_id = id, error = %err);
    }
}

/// Delete drafts untouched for `ttl_hours`. Returns how many went. A draft's
/// clock is its scenario's `updated_at`, which every edit to it moves, so a
/// draft still being written is not swept.
pub async fn sweep_stale(db: &Db, ttl_hours: u32) -> ApiResult<u64> {
    let swept = sqlx::query(
        "DELETE FROM scenarios
          WHERE status = 'draft' AND updated_at < datetime('now', '-' || ?1 || ' hours')",
    )
    .bind(i64::from(ttl_hours))
    .execute(db)
    .await?
    .rows_affected();
    if swept > 0 {
        tracing::info!(event = "draft.swept", count = swept);
    }
    Ok(swept)
}

/// [`sweep_stale`], then delete the held originals of every draft that no
/// longer exists (the ones just swept, and any a crash left behind).
pub async fn sweep(db: &Db, ttl_hours: u32, files: &std::path::Path) -> ApiResult<u64> {
    let swept = sweep_stale(db, ttl_hours).await?;
    documents::images::purge_orphans(db, files).await?;
    Ok(swept)
}
