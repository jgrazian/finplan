//! The drafting agent as a background job on a draft (`api::drafts`).
//!
//! `POST /drafts/{id}/start` stores the person's description and starts
//! [`suggest::ai::draft::run`] in the background, as a review pass runs. The
//! job is one row (`draft_jobs`) whose `state` the web polls through
//! `GET /drafts/{id}`:
//!
//! - `drafting`: a segment of the conversation is running;
//! - `awaiting_answers`: the model called `ask_user` and the job is suspended,
//!   its conversation stored (documents by reference); `POST /drafts/{id}/answers`
//!   records answers and, once every open question has one, resumes;
//! - `ready`: the model finished, or stopped at a limit (`stop` says which);
//! - `failed`: with a public `error`. Notes already written stay in the draft.
//!
//! The model's notes are written as they are accepted ([`DraftTools`] is the
//! host the loop writes them through), so the draft fills while it works, and
//! a note the model marks `auto_add` is applied to the draft at once through
//! the same route a user's "Add to draft" takes. Its tools run through the
//! same code the routes use, as in `review_ai`: draft simulations through
//! `preview::simulate_draft` (admitted as compute like any other), changes
//! through `suggest::resolve_steps`.
//!
//! A server restart leaves no segment running: [`recover`] marks them failed
//! (a suspended job survives, as it holds nothing in memory). Deleting the
//! draft deletes the job (a foreign-key cascade) and aborts a running segment;
//! one that survives anyway finds its job gone and stops at its next turn.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::Mutex;

use axum::Json;
use axum::extract::{Path, State};
use finplan_core::model::{TaxBracket, TaxConfig};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Instrument, instrument::WithSubscriber};
use ts_rs::TS;

use super::drafts::{DraftCounts, DraftEstimate};
use super::preview::{self, DraftBlocked};
use super::review_ai::{AiReviews, PassEnd, PassMetrics, draft_suggestion};
use super::suggestions::{
    self, ApplySuggestion, ApplyTo, DraftMeta, Failure, NewSuggestion, all_changes, fingerprint,
};
use crate::auth::session::CurrentUser;
use crate::compile::rows::ScenarioGraph;
use crate::db::Db;
use crate::documents::{self, store};
use crate::error::{ApiError, ApiResult};
use crate::observability::{AiPassOutcome, Component, ErrorClass, JobKind};
use crate::state::AppState;
use crate::suggest::ai::draft::{
    self, AddedNote, ContextParts, DocumentRead, DocumentTool, DraftHost, DraftInput, DraftOutcome,
    DraftQuestion, DraftStop, LibraryInflation, LibraryProfile, LibraryTax, NewNote, NoteLine,
    NoteSummary, Resume, Transcript, render_context, validate_answer,
};
use crate::suggest::ai::tools::{PathRank, ToolHost, unavailable};
use crate::suggest::ai::{AiError, BoxFuture};
use crate::suggest::{self, Change, ChangeProblem, Created, DiffLine};

/// Most characters of a description.
const MAX_DESCRIPTION: usize = 4_000;
/// Longest progress line shown while a draft is written and once it is ready.
const MAX_PROGRESS: usize = 300;
/// Follow-up messages a finished draft takes. Each runs the agent again, so
/// the month's draft allowance would mean little without a cap.
pub(super) const MAX_FOLLOW_UPS: i64 = 3;

/// The body of `POST /drafts/{id}/start`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct StartDrafting {
    /// What the person says about themselves: age, income, spending, goals,
    /// big purchases. May be empty when documents are attached.
    #[serde(default)]
    pub description: String,
}

/// The body of `POST /drafts/{id}/answers`: answers by question key. A choice
/// answers with an option's `value`, money with a number (or "$1,200"), a date
/// with `YYYY-MM-DD`, text with a string.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct DraftAnswers {
    #[ts(type = "Record<string, string | number>")]
    pub answers: BTreeMap<String, Value>,
    /// Anything else the person wrote with their answers ("Answer, or tell me
    /// more"). Only with answers that leave no question open, since that is
    /// when the agent resumes and reads it.
    #[serde(default)]
    #[ts(optional)]
    pub message: Option<String>,
}

/// The body of `POST /drafts/{id}/messages`: a follow-up to a finished draft.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct DraftMessage {
    pub message: String,
}

/// A job row, as the routes and the runner read it.
#[derive(Debug, sqlx::FromRow)]
pub(super) struct JobRow {
    pub job: String,
    pub state: String,
    pub description: String,
    pub progress: Option<String>,
    pub error: Option<String>,
    pub stop: Option<String>,
    pub questions_json: String,
    pub transcript_json: Option<String>,
    pub simulation_json: Option<String>,
    /// Follow-up messages taken since the job started.
    pub follow_ups: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
}

const JOB_COLUMNS: &str = "job, state, description, progress, error, stop, questions_json, \
                           transcript_json, simulation_json, follow_ups, input_tokens, \
                           output_tokens, cost_usd";

pub(super) async fn load_job(db: &Db, scenario_id: i64) -> ApiResult<Option<JobRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {JOB_COLUMNS} FROM draft_jobs WHERE scenario_id = ?1"
    ))
    .bind(scenario_id)
    .fetch_optional(db)
    .await?)
}

pub(super) fn questions_of(row: &JobRow) -> ApiResult<Vec<DraftQuestion>> {
    serde_json::from_str(&row.questions_json)
        .map_err(|_| ApiError::internal("unreadable stored questions"))
}

/// Mark segments a previous process left running as failed: nothing is going
/// to finish them. A job waiting on answers holds nothing in memory and is left.
pub async fn recover(db: &Db) -> Result<u64, sqlx::Error> {
    let recovered = sqlx::query(
        "UPDATE draft_jobs
            SET state = 'failed', error = 'interrupted by a server restart',
                transcript_json = NULL, finished_at = datetime('now'),
                updated_at = datetime('now')
          WHERE state = 'drafting'",
    )
    .execute(db)
    .await?
    .rows_affected();
    if recovered > 0 {
        tracing::info!(event = "draft_ai.recovered", jobs = recovered);
    }
    Ok(recovered)
}

// ── starting and resuming ───────────────────────────────────────────────────

/// One segment of a job for the runner: the first, one resumed after
/// answers, or one a follow-up message started.
struct Segment {
    job: String,
    scenario_id: i64,
    user: CurrentUser,
    resumed: bool,
    /// Notes the answers just unblocked.
    unblocked: Vec<String>,
    /// What the person wrote with their answers, or as the follow-up.
    message: Option<String>,
    /// A follow-up to a finished draft: a fresh conversation over the draft.
    follow_up: bool,
}

/// A message's text, trimmed and within the description's limit.
fn message_text(message: &str) -> ApiResult<String> {
    let message = message.trim();
    if message.chars().count() > MAX_DESCRIPTION {
        return Err(ApiError::unprocessable(format!(
            "a message is at most {MAX_DESCRIPTION} characters"
        )));
    }
    Ok(message.to_owned())
}

/// Begin drafting: store the description, mark the job `drafting` and run its
/// first segment in the background.
pub(super) async fn start(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    description: &str,
) -> ApiResult<()> {
    let Some(reviews) = state.review_ai_for(user) else {
        return Err(ApiError::Conflict(
            "AI drafts are not available on this server".into(),
        ));
    };
    let description = description.trim();
    if description.chars().count() > MAX_DESCRIPTION {
        return Err(ApiError::unprocessable(format!(
            "the description is at most {MAX_DESCRIPTION} characters"
        )));
    }
    let documents: i64 =
        sqlx::query_scalar("SELECT count(*) FROM documents WHERE scenario_id = ?1")
            .bind(scenario_id)
            .fetch_one(&state.db)
            .await?;
    if description.is_empty() && documents == 0 {
        return Err(ApiError::unprocessable(
            "describe yourself or attach a document to draft from",
        ));
    }

    let job = uuid::Uuid::new_v4().to_string();
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let existing: Option<String> =
        sqlx::query_scalar("SELECT state FROM draft_jobs WHERE scenario_id = ?1")
            .bind(scenario_id)
            .fetch_optional(&mut *tx)
            .await?;
    if existing.is_some_and(|s| s != "failed") {
        return Err(ApiError::Conflict(
            "this draft has already been started".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO draft_jobs (scenario_id, job, state, description, progress)
         VALUES (?1, ?2, 'drafting', ?3, 'Reading what you sent')
         ON CONFLICT(scenario_id) DO UPDATE SET
             job = excluded.job, state = 'drafting', description = excluded.description,
             progress = excluded.progress, error = NULL, stop = NULL,
             questions_json = '[]', transcript_json = NULL, simulation_json = NULL,
             follow_ups = 0, turns = 0, previews = 0, input_tokens = 0, output_tokens = 0, cost_usd = 0,
             started_at = datetime('now'), finished_at = NULL, updated_at = datetime('now')",
    )
    .bind(scenario_id)
    .bind(&job)
    .bind(description)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    launch(
        state,
        reviews,
        Segment {
            job,
            scenario_id,
            user: user.clone(),
            resumed: false,
            unblocked: Vec::new(),
            message: None,
            follow_up: false,
        },
    );
    Ok(())
}

/// Record answers to the open questions; once none is left open, resume the
/// job. Every answer is checked against its question and all problems reported
/// together; nothing is stored unless all are sound.
pub(super) async fn answer(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    answers: BTreeMap<String, Value>,
    message: Option<&str>,
) -> ApiResult<()> {
    let Some(reviews) = state.review_ai_for(user) else {
        return Err(ApiError::Conflict(
            "AI drafts are not available on this server".into(),
        ));
    };
    if answers.is_empty() {
        return Err(ApiError::unprocessable("send at least one answer"));
    }
    let message = message
        .map(message_text)
        .transpose()?
        .filter(|m| !m.is_empty());
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let row: JobRow = sqlx::query_as(&format!(
        "SELECT {JOB_COLUMNS} FROM draft_jobs WHERE scenario_id = ?1"
    ))
    .bind(scenario_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::Conflict("this draft has no questions".into()))?;
    if row.state != "awaiting_answers" {
        return Err(ApiError::Conflict(
            "this draft is not waiting for answers".into(),
        ));
    }
    let mut questions = questions_of(&row)?;
    let mut problems = Vec::new();
    let mut accepted = Vec::new();
    for (key, given) in &answers {
        match questions.iter().find(|q| q.key == *key) {
            None => problems.push(format!("there is no question `{key}`")),
            Some(q) if !q.is_open() => problems.push(format!("`{key}` is already answered")),
            Some(q) => match validate_answer(q, given) {
                Ok(value) => accepted.push((key.clone(), value)),
                Err(message) => problems.push(message),
            },
        }
    }
    if !problems.is_empty() {
        return Err(ApiError::unprocessable(problems.join("; ")));
    }
    for (key, value) in accepted {
        if let Some(q) = questions.iter_mut().find(|q| q.key == key) {
            q.answer = Some(value);
        }
    }

    // The notes that waited on these answers no longer do.
    let answered: Vec<&str> = answers.keys().map(String::as_str).collect();
    let waiting: Vec<(i64, Option<String>, String)> = sqlx::query_as(
        "SELECT id, note_key, blocked_by_json FROM suggestions
          WHERE scenario_id = ?1 AND blocked_by_json <> '[]'",
    )
    .bind(scenario_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut unblocked = Vec::new();
    for (id, key, json) in waiting {
        let blocked: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
        let remaining: Vec<&String> = blocked
            .iter()
            .filter(|b| !answered.contains(&b.as_str()))
            .collect();
        if remaining.len() == blocked.len() {
            continue;
        }
        sqlx::query("UPDATE suggestions SET blocked_by_json = ?2 WHERE id = ?1")
            .bind(id)
            .bind(serde_json::to_string(&remaining).unwrap_or_else(|_| "[]".into()))
            .execute(&mut *tx)
            .await?;
        if remaining.is_empty() {
            unblocked.push(key.unwrap_or_else(|| format!("#{id}")));
        }
    }

    let resume = questions.iter().all(|q| !q.is_open());
    // The agent reads a message only when it resumes; one sent with a question
    // still open would sit unread.
    if message.is_some() && !resume {
        return Err(ApiError::unprocessable(
            "answer every question before adding a message",
        ));
    }
    let job = uuid::Uuid::new_v4().to_string();
    // The message joins the description, which the context shows and
    // description evidence is checked against.
    sqlx::query(
        "UPDATE draft_jobs
            SET questions_json = ?2,
                state = CASE WHEN ?3 THEN 'drafting' ELSE state END,
                job = CASE WHEN ?3 THEN ?4 ELSE job END,
                progress = CASE WHEN ?3 THEN 'Continuing with your answers' ELSE progress END,
                description = CASE WHEN ?5 IS NULL THEN description
                                   WHEN description = '' THEN ?5
                                   ELSE description || char(10) || char(10) || ?5 END,
                updated_at = datetime('now')
          WHERE scenario_id = ?1",
    )
    .bind(scenario_id)
    .bind(serde_json::to_string(&questions).map_err(|e| ApiError::internal(e.to_string()))?)
    .bind(resume)
    .bind(&job)
    .bind(message.as_deref())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    super::touch_scenario(&state.db, scenario_id).await?;

    if resume {
        launch(
            state,
            reviews,
            Segment {
                job,
                scenario_id,
                user: user.clone(),
                resumed: true,
                unblocked,
                message,
                follow_up: false,
            },
        );
    }
    Ok(())
}

/// Take a follow-up message to a finished draft and run the agent again over
/// the draft as it stands: the message joins the description and opens a
/// fresh conversation (a finished job keeps none). At most
/// [`MAX_FOLLOW_UPS`] per draft.
pub(super) async fn follow_up(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    message: &str,
) -> ApiResult<()> {
    let Some(reviews) = state.review_ai_for(user) else {
        return Err(ApiError::Conflict(
            "AI drafts are not available on this server".into(),
        ));
    };
    let message = message_text(message)?;
    if message.is_empty() {
        return Err(ApiError::unprocessable("write a message to send"));
    }
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let row: JobRow = sqlx::query_as(&format!(
        "SELECT {JOB_COLUMNS} FROM draft_jobs WHERE scenario_id = ?1"
    ))
    .bind(scenario_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::Conflict("this draft has not been started".into()))?;
    match row.state.as_str() {
        "ready" => {}
        "drafting" => {
            return Err(ApiError::Conflict(
                "the draft is still being written; wait for it to finish".into(),
            ));
        }
        "awaiting_answers" => {
            return Err(ApiError::Conflict(
                "the draft is waiting for answers; send the message with them".into(),
            ));
        }
        _ => {
            return Err(ApiError::Conflict(
                "this draft stopped; start it again".into(),
            ));
        }
    }
    if row.follow_ups >= MAX_FOLLOW_UPS {
        return Err(ApiError::Conflict(format!(
            "a draft takes at most {MAX_FOLLOW_UPS} follow-up messages; review it and change the rest there"
        )));
    }
    let job = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "UPDATE draft_jobs
            SET job = ?2, state = 'drafting', progress = 'Reading your message',
                description = CASE WHEN description = '' THEN ?3
                                   ELSE description || char(10) || char(10) || ?3 END,
                follow_ups = follow_ups + 1, error = NULL, stop = NULL,
                transcript_json = NULL, finished_at = NULL, updated_at = datetime('now')
          WHERE scenario_id = ?1",
    )
    .bind(scenario_id)
    .bind(&job)
    .bind(&message)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    super::touch_scenario(&state.db, scenario_id).await?;

    launch(
        state,
        reviews,
        Segment {
            job,
            scenario_id,
            user: user.clone(),
            resumed: true,
            unblocked: Vec::new(),
            message: Some(message),
            follow_up: true,
        },
    );
    Ok(())
}

/// Stop a draft's running segment, if any (the draft is being deleted or
/// created).
pub(super) fn abort(state: &AppState, scenario_id: i64) {
    if let Some(reviews) = &state.review_ai {
        reviews.abort_draft(scenario_id);
    }
}

/// Run `segment` in the background, replacing (aborting) any other for the draft.
fn launch(state: &AppState, reviews: &AiReviews, segment: Segment) {
    let scenario_id = segment.scenario_id;
    let job = segment.job.clone();
    let span = tracing::info_span!(
        "draft_ai",
        ai_job = %job,
        scenario_id,
        user_id = %segment.user.id,
    );
    let task = tokio::spawn(
        run_segment(state.clone(), reviews.clone(), segment)
            .instrument(span.clone())
            .with_current_subscriber(),
    );
    reviews.track_draft(scenario_id, &job, task.abort_handle());

    // One that panicked would otherwise read as drafting until the next restart.
    let reviews = reviews.clone();
    let db = state.db.clone();
    tokio::spawn(
        async move {
            if let Err(error) = task.await
                && error.is_panic()
            {
                tracing::error!(event = "draft_ai.panicked", scenario_id);
                fail(
                    &db,
                    scenario_id,
                    &job,
                    "the drafting model stopped unexpectedly",
                )
                .await;
            }
            reviews.forget_draft(scenario_id, &job);
        }
        .instrument(span)
        .with_current_subscriber(),
    );
}

/// Record a failed job, if it is still the current one. Its transcript goes:
/// nothing resumes a failed job.
async fn fail(db: &Db, scenario_id: i64, job: &str, message: &str) {
    let result = sqlx::query(
        "UPDATE draft_jobs
            SET state = 'failed', error = ?3, transcript_json = NULL,
                finished_at = datetime('now'), updated_at = datetime('now')
          WHERE scenario_id = ?1 AND job = ?2 AND state = 'drafting'",
    )
    .bind(scenario_id)
    .bind(job)
    .bind(message)
    .execute(db)
    .await;
    if result.is_err() {
        tracing::error!(event = "draft_ai.status_failed", scenario_id);
    }
}

async fn run_segment(state: AppState, reviews: AiReviews, segment: Segment) {
    let telemetry = state.telemetry.clone();
    let Ok(_permit) = reviews.permits().acquire_owned().await else {
        return;
    };
    let _running = telemetry.job_started(JobKind::ReviewAi);
    let mut ended = PassEnd::new(&telemetry, reviews.model());
    let observer = PassMetrics::new(&telemetry, reviews.model());
    let Segment {
        job,
        scenario_id,
        user,
        resumed,
        unblocked,
        message,
        follow_up,
    } = segment;

    let prepared = prepare(&state, &user, scenario_id, &job, follow_up).await;
    let (host, input, transcript) = match prepared {
        Ok(Some(prepared)) => prepared,
        // The draft was deleted before the segment began.
        Ok(None) => {
            ended.set(AiPassOutcome::Superseded);
            return;
        }
        Err(error) => {
            telemetry.count_error(Component::ReviewAi, ErrorClass::Database);
            tracing::error!(event = "draft_ai.start_failed", error = %error);
            fail(&state.db, scenario_id, &job, "the draft could not start").await;
            ended.set(AiPassOutcome::Failed);
            return;
        }
    };
    let resume = resumed.then(|| Resume {
        state: format!("The draft now holds: {}.", host.holds_line()),
        unblocked,
        message,
        follow_up,
    });

    let outcome = draft::run(
        reviews.draft_client(),
        &host,
        &observer,
        input,
        transcript,
        resume,
    )
    .await;
    match outcome {
        Ok(outcome) => {
            let pass = match &outcome.stop {
                DraftStop::Finished | DraftStop::Suspended => AiPassOutcome::Finished,
                DraftStop::TurnLimit => AiPassOutcome::TurnLimit,
                DraftStop::MaxTokens => AiPassOutcome::MaxTokens,
                DraftStop::Refused { .. } => AiPassOutcome::Refused,
                DraftStop::Interrupted { .. } => AiPassOutcome::Interrupted,
                DraftStop::Unexpected { .. } => AiPassOutcome::Unexpected,
                DraftStop::Cancelled => AiPassOutcome::Superseded,
            };
            match store_outcome(&state.db, scenario_id, &job, &outcome).await {
                Ok(true) => ended.set(pass),
                Ok(false) => ended.set(AiPassOutcome::Superseded),
                Err(_) => {
                    telemetry.count_error(Component::ReviewAi, ErrorClass::Database);
                    tracing::error!(event = "draft_ai.store_failed", scenario_id);
                    fail(&state.db, scenario_id, &job, "the draft could not be saved").await;
                    ended.set(AiPassOutcome::Failed);
                }
            }
        }
        Err(error) => {
            let class = match error {
                AiError::Api(_) => ErrorClass::Upstream,
                AiError::Malformed(_) => ErrorClass::Internal,
            };
            telemetry.count_error(Component::ReviewAi, class);
            tracing::warn!(
                event = "draft_ai.failed",
                scenario_id,
                error_class = class.as_str(),
                error = %error
            );
            let message = match error {
                AiError::Api(_) => "the drafting model could not be reached",
                AiError::Malformed(_) => "the drafting model's reply could not be read",
            };
            fail(&state.db, scenario_id, &job, message).await;
            ended.set(AiPassOutcome::Failed);
        }
    }
}

/// What a segment starts from: the host bound to the draft, the conversation's
/// opening (context, documents, questions) and its stored transcript. `None`
/// when the draft no longer exists.
async fn prepare(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    job: &str,
    follow_up: bool,
) -> ApiResult<Option<(DraftTools, DraftInput, Transcript)>> {
    let Some(row) = load_job(&state.db, scenario_id).await? else {
        return Ok(None);
    };
    if row.job != job {
        return Ok(None);
    }
    let questions = questions_of(&row)?;
    let mut transcript: Transcript = match &row.transcript_json {
        Some(json) => serde_json::from_str(json)
            .map_err(|_| ApiError::internal("unreadable stored conversation"))?,
        None => Transcript::default(),
    };
    // A follow-up starts a new conversation with a new turn budget, but what
    // the job has spent carries on, so the stored totals stay the job's.
    if follow_up {
        transcript.usage.input_tokens = row.input_tokens.max(0) as u64;
        transcript.usage.output_tokens = row.output_tokens.max(0) as u64;
        transcript.usage.cost_usd = row.cost_usd;
    }
    let host = DraftTools::load(state, user, scenario_id, job).await?;

    let manifest = store::manifest(&state.db, scenario_id).await?;
    let documents = store::load_all(&state.db, scenario_id).await?;
    let notes = host.note_lines().await?;
    let (profiles, taxes, inflation) = host.library(state).await?;
    let presets: Vec<(String, String, i32, usize)> = crate::compile::HISTORY_PRESETS
        .iter()
        .filter_map(|id| {
            let history = crate::compile::historical_returns(id).ok()?;
            Some((
                (*id).to_owned(),
                history.name.to_string(),
                i32::from(history.start_year),
                history.returns.len(),
            ))
        })
        .collect();
    let context = {
        let graph = host.graph();
        render_context(&ContextParts {
            today: &graph.scenario.start_date,
            description: &row.description,
            manifest: &manifest,
            graph: &graph,
            profiles: &profiles,
            presets: &presets,
            taxes: &taxes,
            inflation: &inflation,
            notes: &notes,
            questions: &questions,
        })
    };
    let input = DraftInput {
        context,
        description: row.description.clone(),
        documents: documents
            .into_iter()
            .map(|d| {
                let text = (d.status == store::DocumentStatus::Parsed).then_some(d.text);
                (d.id, text)
            })
            .collect(),
        questions,
    };
    Ok(Some((host, input, transcript)))
}

/// Write a segment's result, unless the job is no longer the draft's current
/// one (`false`). A suspended job keeps its conversation; a finished one does
/// not.
async fn store_outcome(
    db: &Db,
    scenario_id: i64,
    job: &str,
    outcome: &DraftOutcome,
) -> ApiResult<bool> {
    let (state, error) = match &outcome.stop {
        DraftStop::Suspended => ("awaiting_answers", None),
        DraftStop::Finished | DraftStop::TurnLimit | DraftStop::MaxTokens => ("ready", None),
        DraftStop::Cancelled => return Ok(false),
        DraftStop::Refused { .. } => (
            "failed",
            Some("the drafting model declined to draft this plan"),
        ),
        DraftStop::Interrupted { .. } => {
            ("failed", Some("the drafting model stopped partway through"))
        }
        DraftStop::Unexpected { .. } => ("failed", Some("the drafting model stopped unexpectedly")),
    };
    let mut progress = match &outcome.stop {
        DraftStop::Suspended => "Waiting for your answers".to_owned(),
        DraftStop::TurnLimit => {
            "Stopped at the drafting limit; the draft may be incomplete. ".to_owned()
        }
        DraftStop::MaxTokens => {
            "The model ran out of room; the draft may be incomplete. ".to_owned()
        }
        _ => String::new(),
    };
    if state == "ready"
        && let Some(summary) = &outcome.summary
    {
        progress.push_str(summary);
    }
    let progress: String = progress.trim().chars().take(MAX_PROGRESS).collect();
    let usage = &outcome.transcript.usage;
    let transcript = (state == "awaiting_answers")
        .then(|| serde_json::to_string(&outcome.transcript))
        .transpose()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let questions =
        serde_json::to_string(&outcome.questions).map_err(|e| ApiError::internal(e.to_string()))?;
    let stored = sqlx::query(
        "UPDATE draft_jobs
            SET state = ?3, stop = ?4, error = ?5,
                progress = CASE WHEN ?6 = '' THEN progress ELSE ?6 END,
                questions_json = ?7, transcript_json = ?8,
                turns = ?9, previews = ?10, input_tokens = ?11, output_tokens = ?12,
                cost_usd = ?13,
                finished_at = CASE WHEN ?3 = 'awaiting_answers' THEN NULL
                                   ELSE datetime('now') END,
                updated_at = datetime('now')
          WHERE scenario_id = ?1 AND job = ?2 AND state = 'drafting'",
    )
    .bind(scenario_id)
    .bind(job)
    .bind(state)
    .bind(draft::stop_tag(&outcome.stop))
    .bind(error)
    .bind(&progress)
    .bind(questions)
    .bind(transcript)
    .bind(i64::from(usage.turns))
    .bind(i64::from(usage.previews))
    .bind(usage.input_tokens as i64)
    .bind(usage.output_tokens as i64)
    .bind(usage.cost_usd)
    .execute(db)
    .await?
    .rows_affected();
    Ok(stored > 0)
}

// ── the host ────────────────────────────────────────────────────────────────

/// The drafting loop's server side: the draft's plan (reloaded after every
/// write), its documents, the user's library and where notes go.
pub(super) struct DraftTools {
    state: AppState,
    user: CurrentUser,
    scenario_id: i64,
    job: String,
    /// The draft as it stands, with the user's whole library loaded, so the
    /// model's steps resolve against what a route would.
    graph: Mutex<ScenarioGraph>,
    /// Every return profile's name and description, for diffs and
    /// `find_return_profile`.
    library: Mutex<Vec<LibraryProfile>>,
}

impl DraftTools {
    pub(super) async fn load(
        state: &AppState,
        user: &CurrentUser,
        scenario_id: i64,
        job: &str,
    ) -> ApiResult<Self> {
        let tools = Self {
            state: state.clone(),
            user: user.clone(),
            scenario_id,
            job: job.to_owned(),
            graph: Mutex::new(ScenarioGraph::load(&state.db, scenario_id, &user.id).await?),
            library: Mutex::new(Vec::new()),
        };
        tools.refresh().await?;
        Ok(tools)
    }

    /// Reload the draft and the library.
    async fn refresh(&self) -> ApiResult<()> {
        let db = &self.state.db;
        let uid = &self.user.id;
        let mut graph = ScenarioGraph::load(db, self.scenario_id, uid).await?;
        let rows: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, name, description, asset_class FROM return_profiles
              WHERE user_id = ?1 ORDER BY sort_order, id",
        )
        .bind(uid)
        .fetch_all(db)
        .await?;
        preview::load_profiles(db, uid, &mut graph, rows.iter().map(|r| r.0).collect()).await?;
        let tax: Vec<i64> = sqlx::query_scalar("SELECT id FROM tax_configs WHERE user_id = ?1")
            .bind(uid)
            .fetch_all(db)
            .await?;
        let inflation: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM inflation_profiles WHERE user_id = ?1")
                .bind(uid)
                .fetch_all(db)
                .await?;
        preview::load_assumptions(db, uid, &mut graph, tax, inflation).await?;
        *self.graph.lock().unwrap_or_else(|e| e.into_inner()) = graph;
        *self.library.lock().unwrap_or_else(|e| e.into_inner()) = rows
            .into_iter()
            .map(|(id, name, description, class)| LibraryProfile {
                id,
                name,
                description,
                asset_class: class
                    .as_deref()
                    .and_then(crate::api::profiles::AssetClass::parse),
            })
            .collect();
        Ok(())
    }

    fn graph(&self) -> std::sync::MutexGuard<'_, ScenarioGraph> {
        self.graph.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn names(&self) -> Vec<(i64, String)> {
        self.library
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect()
    }

    /// "3 accounts, 6 events, 7 parameters, 4 notes (2 added)".
    fn holds_line(&self) -> String {
        let graph = self.graph();
        format!(
            "{} accounts, {} events, {} parameters",
            graph.accounts.len(),
            graph.events.len(),
            graph.parameters.len()
        )
    }

    /// The user's tax configurations and inflation profiles, and the return
    /// profiles, for the context.
    async fn library(
        &self,
        state: &AppState,
    ) -> ApiResult<(Vec<LibraryProfile>, Vec<LibraryTax>, Vec<LibraryInflation>)> {
        let profiles = self
            .library
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        #[derive(sqlx::FromRow)]
        struct TaxRow {
            id: i64,
            name: String,
            description: Option<String>,
            state_rate: f64,
            capital_gains_rate: f64,
            early_withdrawal_penalty_rate: f64,
            standard_deduction: f64,
            age_65_extra_deduction: f64,
            brackets: i64,
        }
        let taxes: Vec<TaxRow> = sqlx::query_as(
            "SELECT t.id, t.name, t.description, t.state_rate, t.capital_gains_rate,
                    t.early_withdrawal_penalty_rate, t.standard_deduction,
                    t.age_65_extra_deduction,
                    (SELECT count(*) FROM tax_brackets b WHERE b.tax_config_id = t.id) AS brackets
               FROM tax_configs t WHERE t.user_id = ?1 ORDER BY t.id",
        )
        .bind(&self.user.id)
        .fetch_all(&state.db)
        .await?;
        let inflation: Vec<(i64, String, Option<String>)> = sqlx::query_as(
            "SELECT id, name, description FROM inflation_profiles
              WHERE user_id = ?1 ORDER BY sort_order, id",
        )
        .bind(&self.user.id)
        .fetch_all(&state.db)
        .await?;
        Ok((
            profiles,
            taxes
                .into_iter()
                .map(|t| LibraryTax {
                    id: t.id,
                    name: t.name,
                    description: t.description,
                    state_rate: t.state_rate,
                    capital_gains_rate: t.capital_gains_rate,
                    early_withdrawal_penalty_rate: t.early_withdrawal_penalty_rate,
                    standard_deduction: t.standard_deduction,
                    age_65_extra_deduction: t.age_65_extra_deduction,
                    brackets: t.brackets as usize,
                })
                .collect(),
            inflation
                .into_iter()
                .map(|(id, name, description)| LibraryInflation {
                    id,
                    name,
                    description,
                })
                .collect(),
        ))
    }

    /// The draft's notes, one line each, for a resumed conversation's context.
    async fn note_lines(&self) -> ApiResult<Vec<NoteLine>> {
        Ok(suggestions::notes_of(&self.state.db, self.scenario_id)
            .await?
            .into_iter()
            .filter(|n| {
                !matches!(
                    n.status,
                    suggestions::SuggestionStatus::Dismissed
                        | suggestions::SuggestionStatus::Confirmed
                )
            })
            .map(|n| NoteLine {
                key: n.note_key.clone(),
                kind: suggestions::tag(&n.kind),
                title: n.title.clone(),
                state: if n.status == suggestions::SuggestionStatus::Applied {
                    "added".into()
                } else if n.blocked_by.is_empty() {
                    "open".into()
                } else {
                    format!("waiting on {}", n.blocked_by.join(", "))
                },
            })
            .collect())
    }

    async fn set_progress(&self, line: &str) {
        let line: String = line.chars().take(MAX_PROGRESS).collect();
        let result = sqlx::query(
            "UPDATE draft_jobs SET progress = ?3, updated_at = datetime('now')
              WHERE scenario_id = ?1 AND job = ?2 AND state = 'drafting'",
        )
        .bind(self.scenario_id)
        .bind(&self.job)
        .bind(line)
        .execute(&self.state.db)
        .await;
        if result.is_err() {
            tracing::warn!(
                event = "draft_ai.progress_failed",
                scenario_id = self.scenario_id
            );
        }
    }

    /// Store the draft's latest simulation, for the polling client.
    async fn keep_estimate(&self, estimate: &DraftEstimate) {
        let Ok(json) = serde_json::to_string(estimate) else {
            return;
        };
        let _ = sqlx::query(
            "UPDATE draft_jobs SET simulation_json = ?3
              WHERE scenario_id = ?1 AND job = ?2",
        )
        .bind(self.scenario_id)
        .bind(&self.job)
        .bind(json)
        .execute(&self.state.db)
        .await;
    }
}

/// A route error as the model may read it: what the model did wrong stays,
/// the server's own faults do not.
fn public(error: ApiError) -> String {
    match error {
        ApiError::Database(_) | ApiError::Internal(_) => {
            tracing::warn!(event = "draft_ai.tool_failed", error = %error);
            "the server could not do that; try something else or stop".to_string()
        }
        other => other.to_string(),
    }
}

/// A failed apply, in words.
pub(super) fn failure_text(failure: Failure) -> String {
    match failure {
        Failure::Api(error) => public(error),
        Failure::Problems {
            message, by_step, ..
        } => {
            let first = by_step
                .iter()
                .flat_map(|s| s.problems.iter())
                .next()
                .map(|p| serde_json::to_string(p).unwrap_or_default())
                .unwrap_or_default();
            format!("{message}: {first}")
        }
    }
}

impl ToolHost for DraftTools {
    fn preview<'a>(&'a self, _changes: Vec<Change>) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async { Err(unavailable("preview_changes")) })
    }

    fn preflight(&self) -> Result<Value, String> {
        serde_json::to_value(super::onboarding::review(&self.graph())).map_err(|e| e.to_string())
    }

    fn inspect_path<'a>(
        &'a self,
        _rank: PathRank,
        _years: Option<(i64, i64)>,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Err(unavailable("inspect_path")) })
    }

    fn goal_seek<'a>(
        &'a self,
        request: crate::suggest::ai::tools::goal_seek::GoalSeekRequest,
    ) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async move {
            // The draft as it stands, copied so no lock is held while it runs.
            let graph = self.graph().clone();
            super::analysis::ai_goal_seek(&self.state, &self.user, &graph, request)
                .await
                .map_err(public)
        })
    }

    fn plan_tax_config(&self) -> Option<TaxConfig> {
        let graph = self.graph();
        let config = graph.tax_config.as_ref()?;
        Some(TaxConfig {
            federal_brackets: graph
                .tax_brackets
                .iter()
                .map(|b| TaxBracket {
                    threshold: b.threshold,
                    rate: b.rate,
                })
                .collect(),
            state_rate: config.state_rate,
            capital_gains_rate: config.capital_gains_rate,
            early_withdrawal_penalty_rate: config.early_withdrawal_penalty_rate,
            standard_deduction: config.standard_deduction,
            age_65_extra_deduction: config.age_65_extra_deduction,
        })
    }

    fn resolve_steps(
        &self,
        steps: &[Vec<Change>],
    ) -> Result<Vec<Vec<DiffLine>>, (usize, Vec<ChangeProblem>)> {
        let graph = self.graph();
        match suggest::resolve_steps(&graph, steps, &Created::new()) {
            Ok(Ok(stepped)) => Ok(stepped
                .steps
                .iter()
                .map(|step| step.diff(self.names()))
                .collect()),
            Ok(Err(failed)) => Err((failed.step, failed.problems)),
            // A server fault, not the model's: reported without detail.
            Err(error) => {
                tracing::warn!(event = "draft_ai.resolve_failed", error = %error);
                Err((
                    0,
                    vec![ChangeProblem::UnsupportedOp {
                        change: 0,
                        reason: "the changes could not be checked; try again or stop".into(),
                    }],
                ))
            }
        }
    }
}

impl DraftHost for DraftTools {
    fn alive(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM draft_jobs WHERE scenario_id = ?1 AND job = ?2)",
            )
            .bind(self.scenario_id)
            .bind(&self.job)
            .fetch_one(&self.state.db)
            .await
            // A database that cannot answer is not a reason to abandon the job.
            .unwrap_or(true)
        })
    }

    fn progress(&self, line: String) -> BoxFuture<'_, ()> {
        Box::pin(async move { self.set_progress(&line).await })
    }

    fn read_document(
        &self,
        id: i64,
        pages: Option<RangeInclusive<u32>>,
    ) -> BoxFuture<'_, Result<DocumentRead, String>> {
        Box::pin(async move {
            let doc = store::load(&self.state.db, self.scenario_id, id)
                .await
                .map_err(|e| match e {
                    ApiError::NotFound(_) => format!("there is no document #{id} in this draft"),
                    other => public(other),
                })?;
            if doc.status == store::DocumentStatus::Failed {
                return Err(format!(
                    "document #{id} could not be read{}",
                    doc.note.map(|n| format!(": {n}")).unwrap_or_default()
                ));
            }
            if doc.status.holds_original() {
                let held = documents::pending_file(
                    &self.state.db,
                    &self.state.config,
                    self.scenario_id,
                    id,
                )
                .await
                .map_err(public)?;
                return match held {
                    Some(file) => Ok(DocumentRead::Held {
                        filename: file.filename,
                        mime: file.mime,
                        bytes: file.bytes,
                    }),
                    None => Err(format!(
                        "the original of document #{id} is gone and it has no text"
                    )),
                };
            }
            let total = doc.pages.max(1) as usize;
            Ok(DocumentRead::Text {
                filename: doc.filename,
                kind: doc.kind.as_str().to_owned(),
                pages_total: total,
                pages: store::split_pages(&doc.text, pages),
            })
        })
    }

    fn store_extraction(
        &self,
        id: i64,
        text: String,
    ) -> BoxFuture<'_, Result<Vec<String>, String>> {
        Box::pin(async move {
            const MAX_EXTRACTION: usize = 60_000;
            if text.trim().is_empty() || text.len() > MAX_EXTRACTION {
                return Err(format!(
                    "extraction is the document's text, 1 to {MAX_EXTRACTION} characters"
                ));
            }
            store::load(&self.state.db, self.scenario_id, id)
                .await
                .map_err(|e| match e {
                    ApiError::NotFound(_) => format!("there is no document #{id} in this draft"),
                    other => public(other),
                })?;
            documents::store_extraction(
                &self.state.db,
                &self.state.config,
                self.scenario_id,
                id,
                &text,
            )
            .await
            .map_err(|e| match e {
                ApiError::Conflict(message) => message,
                other => public(other),
            })?;
            let doc = store::load(&self.state.db, self.scenario_id, id)
                .await
                .map_err(public)?;
            Ok(store::split_pages(&doc.text, None)
                .into_iter()
                .map(|p| p.text)
                .collect())
        })
    }

    fn document_tool(
        &self,
        tool: DocumentTool,
        id: i64,
        months: Option<u32>,
    ) -> BoxFuture<'_, Result<Value, String>> {
        Box::pin(async move {
            let doc = store::load(&self.state.db, self.scenario_id, id)
                .await
                .map_err(|e| match e {
                    ApiError::NotFound(_) => format!("there is no document #{id} in this draft"),
                    other => public(other),
                })?;
            let value = match tool {
                DocumentTool::SummarizeTransactions => {
                    serde_json::to_value(documents::tools::summarize_transactions(&doc, months))
                }
                DocumentTool::MatchAccount => {
                    serde_json::to_value(documents::tools::match_account(&doc, &self.graph()))
                }
                DocumentTool::Reconcile => {
                    serde_json::to_value(documents::tools::reconcile(&doc, &self.graph()))
                }
            };
            value.map_err(|e| e.to_string())
        })
    }

    fn return_profiles(&self) -> Vec<LibraryProfile> {
        self.library
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn simulate(&self, steps: Vec<Vec<Change>>) -> BoxFuture<'_, Result<Value, String>> {
        Box::pin(async move {
            let result =
                preview::simulate_draft(&self.state, &self.user, self.scenario_id, &steps, None)
                    .await
                    .map_err(public)?;
            let counts = DraftCounts::load(&self.state.db, self.scenario_id)
                .await
                .map_err(public)?;
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM suggestions
                  WHERE scenario_id = ?1 AND status = 'open' AND blocked_by_json <> '[]'",
            )
            .bind(self.scenario_id)
            .fetch_one(&self.state.db)
            .await
            .map_err(|e| public(e.into()))?;
            let mut estimate = DraftEstimate {
                success_rate: None,
                funding_success_rate: None,
                iterations: result.iterations,
                blocked: None,
            };
            match (&result.stats, &result.blocked) {
                (Some(stats), _) => {
                    estimate.success_rate = Some(stats.success_rate);
                    estimate.funding_success_rate = stats.funding_success_rate;
                }
                (None, Some(DraftBlocked::Compile { message })) => {
                    estimate.blocked = Some(message.clone());
                }
                (None, Some(DraftBlocked::Steps { .. })) => {
                    estimate.blocked = Some("a step could not be applied".into());
                }
                (None, None) => {}
            }
            if steps.is_empty() {
                self.keep_estimate(&estimate).await;
            }
            let mut out = serde_json::to_value(&result).map_err(|e| e.to_string())?;
            out["draft"] = serde_json::to_value(&counts).map_err(|e| e.to_string())?;
            out["notes_waiting_on_answers"] = json!(waiting);
            Ok(out)
        })
    }

    fn notes(&self) -> BoxFuture<'_, Vec<NoteSummary>> {
        Box::pin(async move {
            suggestions::notes_of(&self.state.db, self.scenario_id)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|n| NoteSummary {
                    key: n.note_key.clone(),
                    kind: n.kind,
                    open: n.status == suggestions::SuggestionStatus::Open
                        && n.applied_path.is_none(),
                    changes: n.paths.iter().flat_map(|p| p.all_changes()).collect(),
                    title: n.title,
                })
                .collect()
        })
    }

    fn block_notes(
        &self,
        question: String,
        notes: Vec<String>,
    ) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            for key in notes {
                let current: Option<String> = sqlx::query_scalar(
                    "SELECT blocked_by_json FROM suggestions
                      WHERE scenario_id = ?1 AND note_key = ?2 AND status = 'open'",
                )
                .bind(self.scenario_id)
                .bind(&key)
                .fetch_optional(&self.state.db)
                .await
                .map_err(|e| public(e.into()))?;
                let Some(current) = current else { continue };
                let mut blocked: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
                if !blocked.contains(&question) {
                    blocked.push(question.clone());
                }
                sqlx::query(
                    "UPDATE suggestions SET blocked_by_json = ?3
                      WHERE scenario_id = ?1 AND note_key = ?2 AND status = 'open'",
                )
                .bind(self.scenario_id)
                .bind(&key)
                .bind(serde_json::to_string(&blocked).unwrap_or_else(|_| "[]".into()))
                .execute(&self.state.db)
                .await
                .map_err(|e| public(e.into()))?;
            }
            Ok(())
        })
    }

    fn add_note(&self, note: NewNote) -> BoxFuture<'_, Result<AddedNote, String>> {
        Box::pin(async move {
            let db = &self.state.db;
            let mut new: NewSuggestion = draft_suggestion(0, &note.draft, None, None);
            new.run_id = None;
            new.fingerprint = fingerprint(
                None,
                note.draft.kind,
                &all_changes(&new.paths),
                &note.draft.title,
            );
            new.draft = DraftMeta {
                note_key: note.key.clone(),
                blocked_by: note.blocked_by.clone(),
                column: Some(note.column),
                auto_added: false,
            };
            let path_key = suggestions::SuggestionPath::default_of(&new.paths)
                .map(|p| p.key.clone())
                .unwrap_or_default();

            let mut tx = db.begin().await.map_err(|e| public(e.into()))?;
            if let Some(key) = &note.replaces {
                sqlx::query(
                    "DELETE FROM suggestions
                      WHERE scenario_id = ?1 AND note_key = ?2 AND status = 'open'
                        AND applied_path IS NULL",
                )
                .bind(self.scenario_id)
                .bind(key)
                .execute(&mut *tx)
                .await
                .map_err(|e| public(e.into()))?;
            }
            let id = suggestions::insert(&mut tx, self.scenario_id, &new)
                .await
                .map_err(public)?;
            tx.commit().await.map_err(|e| public(e.into()))?;

            let mut added = false;
            let mut not_added = None;
            let mut created = Vec::new();
            if note.auto_add && note.blocked_by.is_empty() {
                let applied = suggestions::apply(
                    State(self.state.clone()),
                    self.user.clone(),
                    Path(id),
                    Json(ApplySuggestion {
                        path: path_key,
                        through_step: None,
                        to: ApplyTo::Plan,
                        name: None,
                    }),
                )
                .await;
                match applied {
                    Ok(_) => {
                        added = true;
                        sqlx::query("UPDATE suggestions SET auto_added = 1 WHERE id = ?1")
                            .bind(id)
                            .execute(db)
                            .await
                            .map_err(|e| public(e.into()))?;
                        let json: String = sqlx::query_scalar(
                            "SELECT created_json FROM suggestions WHERE id = ?1",
                        )
                        .bind(id)
                        .fetch_one(db)
                        .await
                        .map_err(|e| public(e.into()))?;
                        let refs: Created = serde_json::from_str(&json).unwrap_or_default();
                        created = refs
                            .into_iter()
                            .map(|(key, r)| json!({"key": key, "kind": r.kind, "id": r.id}))
                            .collect();
                    }
                    Err(failure) => {
                        let message = failure_text(failure);
                        tracing::info!(
                            event = "draft_ai.auto_add_failed",
                            scenario_id = self.scenario_id
                        );
                        not_added = Some(message);
                    }
                }
            }
            super::touch_scenario(db, self.scenario_id)
                .await
                .map_err(public)?;
            self.refresh().await.map_err(public)?;

            let counts = DraftCounts::load(db, self.scenario_id)
                .await
                .map_err(public)?;
            self.set_progress(&format!(
                "Drafting… {} accounts, {} events, {} parameters so far",
                counts.accounts, counts.events, counts.parameters
            ))
            .await;
            Ok(AddedNote {
                id,
                added,
                not_added,
                created,
                counts: serde_json::to_value(&counts).unwrap_or(Value::Null),
            })
        })
    }
}
