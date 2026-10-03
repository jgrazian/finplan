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
//! The drafting agent (`draft_agent`) fills the draft: `POST /drafts/{id}/start`
//! begins its job, `POST /drafts/{id}/answers` answers its questions (with an
//! optional message), `POST /drafts/{id}/messages` sends a follow-up once it
//! is ready, and
//! [`DraftStatus`] is what the web polls meanwhile. This module owns the
//! lifecycle, the quota and the status.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::draft_agent::{self, DraftAnswers, DraftMessage, StartDrafting};
use super::runs::{self, Run};
use super::scenarios::SCENARIO_COLUMNS;
use super::suggestions::{self, ApplySuggestion, ApplyTo, SuggestionPath};
use crate::auth::session::CurrentUser;
use crate::db::Db;
use crate::documents;
use crate::error::{ApiError, ApiResult};
use crate::observability::{EventFields, Operation, Resource};
use crate::runner::telemetry::Submission;
use crate::state::AppState;
use crate::suggest::ai::draft::DraftQuestion;
use finplan_plan::compile;
use finplan_plan::specs::scenarios::Scenario;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/drafts", post(start))
        .route("/drafts/{id}", get(status).patch(update).delete(discard))
        .route("/drafts/{id}/start", post(start_drafting))
        .route("/drafts/{id}/answers", post(answer))
        .route("/drafts/{id}/messages", post(message))
        .route("/drafts/{id}/create", post(create_and_run))
}

/// Where a draft stands. A draft the agent was never started on is `ready`
/// too: nothing is being written and nothing is being waited for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DraftState {
    /// The model is working.
    Drafting,
    /// The model asked its questions and waits for `POST /drafts/{id}/answers`.
    AwaitingAnswers,
    /// Finished (or stopped at a limit: see `progress`), or never started.
    Ready,
    /// The job failed; `error` says why. Notes written before stay.
    Failed,
}

/// What the draft holds so far, for the live "3 accounts, 6 events" line and
/// 2c's "12 notes · 7 added · 3 to confirm".
#[derive(Debug, Clone, Default, Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct DraftCounts {
    pub accounts: i64,
    pub assets: i64,
    pub events: i64,
    pub parameters: i64,
    /// Notes still open on the draft.
    pub open_suggestions: i64,
    /// Every note of the draft that is still standing: added, open or
    /// confirmed (dismissed ones are gone from the count).
    pub notes: i64,
    /// Notes applied to the draft, by the agent or by the user.
    pub notes_added: i64,
    /// Open notes in the `to_confirm` column.
    pub notes_to_confirm: i64,
}

impl DraftCounts {
    pub async fn load(db: &Db, scenario_id: i64) -> ApiResult<Self> {
        Ok(sqlx::query_as(
            "SELECT (SELECT count(*) FROM accounts WHERE scenario_id = ?1) AS accounts,
                    (SELECT count(*) FROM assets WHERE scenario_id = ?1) AS assets,
                    (SELECT count(*) FROM events WHERE scenario_id = ?1) AS events,
                    (SELECT count(*) FROM named_parameters WHERE scenario_id = ?1) AS parameters,
                    (SELECT count(*) FROM suggestions
                      WHERE scenario_id = ?1 AND status = 'open') AS open_suggestions,
                    (SELECT count(*) FROM suggestions
                      WHERE scenario_id = ?1 AND status <> 'dismissed') AS notes,
                    (SELECT count(*) FROM suggestions
                      WHERE scenario_id = ?1 AND status = 'applied') AS notes_added,
                    (SELECT count(*) FROM suggestions
                      WHERE scenario_id = ?1 AND status = 'open'
                        AND board_column = 'to_confirm') AS notes_to_confirm",
        )
        .bind(scenario_id)
        .fetch_one(db)
        .await?)
    }
}

/// The agent's last whole-plan simulation of the draft as it stood: the
/// "est. 81%" figure. Either the rates or, when the draft cannot run yet, what
/// stops it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftEstimate {
    pub success_rate: Option<f64>,
    pub funding_success_rate: Option<f64>,
    /// Simulated iterations behind the rates.
    pub iterations: usize,
    /// Why the draft could not be simulated, when it could not.
    pub blocked: Option<String>,
}

/// An open note that waits on questions the person has not answered.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BlockedNote {
    pub id: i64,
    /// The note's own key, which a question's `blocks` names.
    pub key: Option<String>,
    pub title: String,
    /// Keys of the unanswered questions it waits on.
    pub waiting_on: Vec<String>,
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
    /// What the agent is doing, or how it ended ("Drafting… 3 accounts, 6
    /// events, 7 parameters so far"); null before it starts.
    pub progress: Option<String>,
    /// Why the job failed, in words for the person; null otherwise.
    pub error: Option<String>,
    /// Why the model stopped once it has: `finished`, `turn_limit`,
    /// `max_tokens`, `suspended`, ...
    pub stop: Option<String>,
    /// The questions the agent is waiting on (state `awaiting_answers`):
    /// at most three, each with its answer type and options.
    pub questions: Vec<DraftQuestion>,
    /// Questions already answered, with their answers.
    pub answered: Vec<DraftQuestion>,
    /// Open notes waiting on an unanswered question ("waiting on question 1").
    pub blocked_notes: Vec<BlockedNote>,
    /// The agent's last simulation of the draft, once it has run one.
    pub estimate: Option<DraftEstimate>,
    /// Follow-up messages the draft still takes once it is ready
    /// (`POST /drafts/{id}/messages`).
    pub follow_ups_left: i64,
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
    if state.review_ai_for(&user).is_none() {
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
    for old in &replaced {
        draft_agent::abort(&state, *old);
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
    let counts = DraftCounts::load(&state.db, id).await?;
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
    let job = draft_agent::load_job(&state.db, id).await?;
    let (state_of, progress, error, stop, questions, estimate) = match &job {
        None => (DraftState::Ready, None, None, None, Vec::new(), None),
        Some(job) => (
            match job.state.as_str() {
                "drafting" => DraftState::Drafting,
                "awaiting_answers" => DraftState::AwaitingAnswers,
                "failed" => DraftState::Failed,
                _ => DraftState::Ready,
            },
            job.progress.clone(),
            job.error.clone(),
            job.stop.clone(),
            draft_agent::questions_of(job)?,
            job.simulation_json
                .as_deref()
                .and_then(|json| serde_json::from_str::<DraftEstimate>(json).ok()),
        ),
    };
    let (answered, open): (Vec<DraftQuestion>, Vec<DraftQuestion>) =
        questions.into_iter().partition(|q| !q.is_open());
    let blocked_notes = blocked_notes(&state.db, id).await?;
    let follow_ups_left =
        (draft_agent::MAX_FOLLOW_UPS - job.as_ref().map_or(0, |job| job.follow_ups)).max(0);
    Ok(DraftStatus {
        id,
        state: state_of,
        scenario,
        expires_at,
        counts,
        retain_documents: retain_documents != 0,
        document_count,
        progress,
        error,
        stop,
        questions: open,
        answered,
        blocked_notes,
        estimate,
        follow_ups_left,
    })
}

/// The draft's open notes that still wait on questions.
async fn blocked_notes(db: &Db, id: i64) -> ApiResult<Vec<BlockedNote>> {
    let rows: Vec<(i64, Option<String>, String, String)> = sqlx::query_as(
        "SELECT id, note_key, title, blocked_by_json FROM suggestions
          WHERE scenario_id = ?1 AND status = 'open' AND blocked_by_json <> '[]'
          ORDER BY id",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, key, title, json)| BlockedNote {
            id,
            key,
            title,
            waiting_on: serde_json::from_str(&json).unwrap_or_default(),
        })
        .collect())
}

/// `POST /drafts/{id}/start` — hand the agent the person's description (and
/// whatever documents are attached) and start it in the background. Answers
/// with the status at once (`drafting`); poll `GET /drafts/{id}`.
async fn start_drafting(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<StartDrafting>,
) -> ApiResult<Json<DraftStatus>> {
    // A draft that is not the caller's, or is no longer a draft, is not found.
    load_status(&state, id, &user.id).await?;
    draft_agent::start(&state, &user, id, &body.description).await?;
    Ok(Json(load_status(&state, id, &user.id).await?))
}

/// `POST /drafts/{id}/answers` — answer the agent's open questions, by key.
/// Any subset may be sent; once none is left open the agent resumes
/// (`drafting`) and the notes that waited on the answers are unblocked.
async fn answer(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<DraftAnswers>,
) -> ApiResult<Json<DraftStatus>> {
    load_status(&state, id, &user.id).await?;
    draft_agent::answer(&state, &user, id, body.answers, body.message.as_deref()).await?;
    Ok(Json(load_status(&state, id, &user.id).await?))
}

/// `POST /drafts/{id}/messages` — a follow-up to a finished draft ("tell me
/// more"): the agent runs again over the draft as it stands and updates it.
/// Refused while it is writing or waiting on answers (send the message with
/// the answers then), and after the draft's last follow-up.
async fn message(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<DraftMessage>,
) -> ApiResult<Json<DraftStatus>> {
    load_status(&state, id, &user.id).await?;
    draft_agent::follow_up(&state, &user, id, &body.message).await?;
    Ok(Json(load_status(&state, id, &user.id).await?))
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
    draft_agent::abort(&state, id);
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

/// The body of `POST /drafts/{id}/create`, all of it optional.
#[derive(Debug, Default, Deserialize, TS)]
#[ts(export)]
pub struct CreateDraft {
    /// Add the draft's open `add` notes first, each by its recommended path,
    /// in the order they were written. Notes waiting on an unanswered
    /// question and check notes (To confirm) are left out. If any cannot be
    /// added the draft stays a draft (422, naming them), so nothing is
    /// dropped without the person knowing; false creates from what the
    /// draft already holds.
    #[serde(default)]
    #[ts(optional)]
    pub add_open: Option<bool>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DraftCreated {
    pub scenario: Scenario,
    /// Open notes `add_open` added on the way.
    pub added: i64,
    /// The queued run. A review starts, as for any plan, once it has
    /// succeeded (`POST /scenarios/{id}/review`), because notes are written
    /// against a finished run.
    pub run: Run,
}

/// `POST /drafts/{id}/create` — Create & run: with `add_open`, add the open
/// notes first ([`add_open_notes`]); then check the plan slot, make the
/// draft a plan (`status = 'active'`) and queue its run the way any run is
/// queued. A draft that does not compile is refused while it is still a draft;
/// if the run cannot be queued (capacity), the draft stays a draft so the
/// request can be retried.
async fn create_and_run(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    body: Option<Json<CreateDraft>>,
) -> ApiResult<(StatusCode, Json<DraftCreated>)> {
    let add_open = body.and_then(|Json(b)| b.add_open).unwrap_or(false);
    // A draft that cannot run says why now, before it stops being a draft.
    let graph = match crate::db::graph::load(&state.db, id, &user.id).await {
        Ok(graph) if super::is_draft(&state.db, id).await? => graph,
        Ok(_) | Err(ApiError::NotFound(_)) => return Err(ApiError::NotFound("draft")),
        Err(err) => return Err(err),
    };
    // A model still writing would keep adding to the plan; open notes and
    // unanswered questions are simply left out.
    if draft_agent::load_job(&state.db, id)
        .await?
        .is_some_and(|job| job.state == "drafting")
    {
        return Err(ApiError::Conflict(
            "the draft is still being written; wait for it to finish".into(),
        ));
    }
    let (graph, added) = if add_open {
        let added = add_open_notes(&state, &user, id).await?;
        (
            crate::db::graph::load(&state.db, id, &user.id).await?,
            added,
        )
    } else {
        (graph, 0)
    };
    compile::compile(&graph)?;

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    crate::billing::check_plan_slot(&mut tx, &user.id, &state.config, 1).await?;
    // Under the write lock, so a job started in the meantime is not missed.
    let drafting: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM draft_jobs WHERE scenario_id = ?1 AND state = 'drafting')",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if drafting {
        return Err(ApiError::Conflict(
            "the draft is still being written; wait for it to finish".into(),
        ));
    }
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
    // originals always are. So is the agent's stored conversation, which
    // quotes them.
    draft_agent::abort(&state, id);
    sqlx::query("DELETE FROM draft_jobs WHERE scenario_id = ?1")
        .bind(id)
        .execute(&state.db)
        .await?;
    documents::finish_draft(&state.db, &state.config, id).await?;
    let scenario: Scenario = sqlx::query_as(&format!(
        "SELECT {SCENARIO_COLUMNS} FROM scenarios WHERE id = ?1"
    ))
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(DraftCreated {
            scenario,
            added,
            run,
        }),
    ))
}

/// Add the draft's open, unblocked `add` notes, as Review's "Add to draft"
/// would: each by the path it has started, else its recommended one, else
/// its first, in the order written (a later note may build on an earlier
/// one). A note that cannot be added does not stop the rest; if any could
/// not, the draft is left a draft and the error names them.
async fn add_open_notes(state: &AppState, user: &CurrentUser, id: i64) -> ApiResult<i64> {
    let open: Vec<(i64, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, title, paths_json, applied_path FROM suggestions
          WHERE scenario_id = ?1 AND status = 'open' AND kind = 'add'
            AND blocked_by_json = '[]'
          ORDER BY id",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    let mut added = 0;
    let mut failed = Vec::new();
    for (note, title, paths_json, applied_path) in open {
        let paths: Vec<SuggestionPath> = serde_json::from_str(&paths_json)
            .map_err(|_| ApiError::internal("unreadable stored paths"))?;
        let Some(path) = applied_path.or_else(|| {
            paths
                .iter()
                .find(|p| p.recommended)
                .or(paths.first())
                .map(|p| p.key.clone())
        }) else {
            continue;
        };
        let applied = suggestions::apply(
            State(state.clone()),
            user.clone(),
            Path(note),
            Json(ApplySuggestion {
                path,
                through_step: None,
                to: ApplyTo::Plan,
                name: None,
            }),
        )
        .await;
        match applied {
            Ok(_) => added += 1,
            Err(failure) => failed.push(format!(
                "\u{201c}{title}\u{201d} ({})",
                draft_agent::failure_text(failure)
            )),
        }
    }
    if !failed.is_empty() {
        return Err(ApiError::unprocessable(format!(
            "{} could not be added to the draft, so it was not created: {}. \
             Fix or leave them out on Review, or create without them",
            if failed.len() == 1 {
                "1 note".to_owned()
            } else {
                format!("{} notes", failed.len())
            },
            failed.join("; ")
        )));
    }
    Ok(added)
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
