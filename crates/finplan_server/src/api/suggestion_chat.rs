//! "Chat about this": a thread of follow-up messages on one review note.
//!
//! `POST /suggestions/{id}/chat` appends the user's message and starts a turn:
//! the review model reads the note's run as a review pass does, the note, and
//! the thread, then answers in the background ([`suggest::ai::chat::answer`]).
//! Its answer joins the thread, and any note it submits (checked exactly as a
//! review's) is stored as a new suggestion pointing back at this one
//! (`parent_id`), so it shows in the review like any other. `GET` reads the
//! thread; `status` is `running` while a turn is out.
//!
//! One turn per thread at a time; a thread takes at most
//! [`MAX_USER_MESSAGES`] questions. Turns share the review model's slots with
//! review passes. A turn whose note is deleted meanwhile (a new review
//! replaces open rule notes) writes nothing: the thread went with the note.
//! A restart leaves no turn running: [`recover`] marks them failed.
//!
//! The turn itself (claiming the thread, running the model, storing the answer
//! and its notes, recovering after a restart) is shared with plan chat
//! (`api::plan_chat`) through [`Thread`], which names either kind of thread.
//!
//! Observability: a turn runs inside the job span of the POST that started it
//! (a `review_chat` or `plan_chat` job), so its logs, the model loop's and its
//! tool calls' carry that request's `request_id`, also stored on the thread.
//! It reports through the same `finplan_review_ai_*` families as review
//! passes (which total every review-model request), and adds
//! `finplan_review_chat_*`: turns by outcome, turn wall time, and the chat
//! share of tokens and cost, for both kinds of thread. Message text is never
//! logged.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tracing::{Instrument, instrument::WithSubscriber};
use ts_rs::TS;

use super::preview;
use super::review_ai::{self, AiReviews, PassMetrics, Tools, draft_suggestion, pass_outcome};
use super::runs::{self, ResultsQuery};
use super::suggestions::{
    self, Suggestion, SuggestionSource, SuggestionStatus, all_changes, insert,
};
use crate::auth::session::CurrentUser;
use crate::compile::rows::ScenarioGraph;
use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::observability::{
    AiCostSource, AiPassOutcome, AiSuggestionOutcome, AiTokenType, Component, ErrorClass,
    JobContext, JobKind, Origin, Outcome, QueueExit, SubmissionResult,
};
use crate::runner::telemetry::{Attempt, Submitted};
use crate::state::AppState;
use crate::suggest::ai::chat::{self, ChatInput, ChatRole as ModelRole, Subject};
use crate::suggest::ai::{AiError, AiOutcome, ReviewContext, stop_tag};
use crate::suggest::rules;

pub fn router() -> Router<AppState> {
    Router::new().route("/suggestions/{id}/chat", get(thread).post(post_message))
}

/// Longest message, in characters.
pub(super) const MAX_MESSAGE: usize = 2_000;
/// Most questions one note's thread takes.
const MAX_USER_MESSAGES: i64 = 20;

/// A thread: a note's ("Chat about this") or a plan's (`api::plan_chat`).
/// Names the tables it lives in and what its turns report as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Thread {
    Note(i64),
    Plan(i64),
}

/// Where a turn has got to, for its log line's `event`.
#[derive(Clone, Copy)]
enum Ev {
    Accepted,
    Stored,
    Discarded,
    StartFailed,
    StoreFailed,
    Failed,
    Panicked,
    StatusFailed,
}

impl Thread {
    /// The note's id, or the plan's.
    fn id(self) -> i64 {
        match self {
            Thread::Note(id) | Thread::Plan(id) => id,
        }
    }

    /// The thread table, the messages table, and the key column both share.
    fn tables(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Thread::Note(_) => ("suggestion_threads", "suggestion_messages", "suggestion_id"),
            Thread::Plan(_) => ("plan_chat_threads", "plan_chat_messages", "scenario_id"),
        }
    }

    fn kind(self) -> JobKind {
        match self {
            Thread::Note(_) => JobKind::ReviewChat,
            Thread::Plan(_) => JobKind::PlanChat,
        }
    }

    fn event(self, ev: Ev) -> &'static str {
        match (self, ev) {
            (Thread::Note(_), Ev::Accepted) => "review_chat.accepted",
            (Thread::Note(_), Ev::Stored) => "review_chat.stored",
            (Thread::Note(_), Ev::Discarded) => "review_chat.discarded",
            (Thread::Note(_), Ev::StartFailed) => "review_chat.start_failed",
            (Thread::Note(_), Ev::StoreFailed) => "review_chat.store_failed",
            (Thread::Note(_), Ev::Failed) => "review_chat.failed",
            (Thread::Note(_), Ev::Panicked) => "review_chat.panicked",
            (Thread::Note(_), Ev::StatusFailed) => "review_chat.status_failed",
            (Thread::Plan(_), Ev::Accepted) => "plan_chat.accepted",
            (Thread::Plan(_), Ev::Stored) => "plan_chat.stored",
            (Thread::Plan(_), Ev::Discarded) => "plan_chat.discarded",
            (Thread::Plan(_), Ev::StartFailed) => "plan_chat.start_failed",
            (Thread::Plan(_), Ev::StoreFailed) => "plan_chat.store_failed",
            (Thread::Plan(_), Ev::Failed) => "plan_chat.failed",
            (Thread::Plan(_), Ev::Panicked) => "plan_chat.panicked",
            (Thread::Plan(_), Ev::StatusFailed) => "plan_chat.status_failed",
        }
    }

    /// A note the model adds from a note's thread points back at that note.
    fn parent(self) -> Option<i64> {
        match self {
            Thread::Note(id) => Some(id),
            Thread::Plan(_) => None,
        }
    }
}

// ── wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChatRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ChatMessage {
    pub id: i64,
    pub role: ChatRole,
    pub text: String,
    pub created_at: String,
    /// The suggestions an assistant message added (their `parent_id` is this
    /// thread's note); empty otherwise.
    pub suggestion_ids: Vec<i64>,
}

/// Where the thread's current turn stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ThreadStatus {
    Idle,
    Running,
    Failed,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SuggestionThread {
    pub suggestion_id: i64,
    pub status: ThreadStatus,
    /// Why the last turn did not finish; null unless `failed`.
    pub error: Option<String>,
    /// Oldest first.
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ChatRequest {
    pub message: String,
}

// ── routes ──────────────────────────────────────────────────────────────────

/// `GET /suggestions/{id}/chat`: the thread, empty if nobody has asked yet.
async fn thread(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<SuggestionThread>> {
    suggestions::owned(&state.db, id, &user.id).await?;
    Ok(Json(load_thread(&state.db, id).await?))
}

/// `POST /suggestions/{id}/chat`: add the user's message and start a turn.
async fn post_message(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<ChatRequest>,
) -> ApiResult<Json<SuggestionThread>> {
    let note = suggestions::owned(&state.db, id, &user.id)
        .await?
        .into_suggestion()?;
    let Some(reviews) = state.review_ai.clone() else {
        return Err(ApiError::Conflict(
            "AI review is not enabled on this server, so there is no model to chat with".into(),
        ));
    };
    let message = body.message.trim().to_owned();
    if message.is_empty() || message.chars().count() > MAX_MESSAGE {
        return Err(ApiError::unprocessable(format!(
            "a message is 1 to {MAX_MESSAGE} characters"
        )));
    }

    // Everything the model reads, before the thread is claimed: a note whose
    // run is gone fails here, with nothing written.
    let (run_id, graph) =
        preview::base_snapshot(&state.db, note.scenario_id, &user.id, note.run_id).await?;
    // The model reads a run's results; a draft has none yet.
    let run_id = run_id.ok_or_else(|| {
        ApiError::Conflict("a note on a draft cannot be chatted about until it has a run".into())
    })?;
    let axum::Json(results) = runs::results(
        State(state.clone()),
        user.clone(),
        Path(run_id),
        axum::extract::Query(ResultsQuery { series: None }),
    )
    .await?;
    let board = suggestions::notes_of(&state.db, note.scenario_id).await?;
    let rule_drafts = rules::review(&graph, &results);
    // The model must repeat neither this note nor any other still standing.
    let context = ReviewContext::build(&graph, &results, &rule_drafts)
        .with_notes(
            board
                .iter()
                .filter(|n| n.status != SuggestionStatus::Applied)
                .map(|n| (n.kind, n.title.as_str(), all_changes(&n.paths))),
        )
        .with_dismissed_notes(
            suggestions::dismissed(&board)
                .into_iter()
                .map(suggestions::outline),
        );
    let note_text = render_note(&note, &board);
    let check_iterations = suggestions::check_iterations(&state.db, run_id).await?;

    let job = review_ai::new_job();
    // Captured here, inside the request, so the turn logs under its id.
    // `job_id` is the thread's note: a turn has no row id of its own (its
    // uuid is `chat_job`, logged alongside).
    let job_context = JobContext::new(
        JobKind::ReviewChat,
        Origin::Request,
        &user.id,
        note.scenario_id,
        id,
    );

    let thread = Thread::Note(id);
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let asked = claim(&mut tx, thread).await?;
    if asked >= MAX_USER_MESSAGES {
        return Err(ApiError::Conflict(format!(
            "a thread takes at most {MAX_USER_MESSAGES} messages; review again for a fresh one"
        )));
    }
    let history = begin_turn(&mut tx, thread, &message, &job, &job_context, None).await?;
    tx.commit().await?;

    start(
        &state,
        reviews,
        Turn {
            thread,
            job,
            job_context,
            submitted: Submitted::now(),
            scenario_id: note.scenario_id,
            run_id,
            user,
            graph,
            context,
            subject: note_text,
            history,
            check_iterations,
        },
        message.chars().count(),
    );

    Ok(Json(load_thread(&state.db, id).await?))
}

/// Take the thread for a new turn on the caller's write transaction: create
/// it if nobody has asked yet, and refuse while a turn is still out. Returns
/// how many questions it has already taken.
pub(super) async fn claim(tx: &mut sqlx::SqliteConnection, thread: Thread) -> ApiResult<i64> {
    let (threads, messages, key) = thread.tables();
    sqlx::query(&format!(
        "INSERT OR IGNORE INTO {threads} ({key}) VALUES (?1)"
    ))
    .bind(thread.id())
    .execute(&mut *tx)
    .await?;
    let status: String =
        sqlx::query_scalar(&format!("SELECT status FROM {threads} WHERE {key} = ?1"))
            .bind(thread.id())
            .fetch_one(&mut *tx)
            .await?;
    if status == "running" {
        return Err(ApiError::Conflict(
            "the model is still answering the last message".into(),
        ));
    }
    Ok(sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM {messages} WHERE {key} = ?1 AND role = 'user'"
    ))
    .bind(thread.id())
    .fetch_one(&mut *tx)
    .await?)
}

/// Add the user's message to a claimed thread and mark it running. Returns
/// the history the model reads: the last `window` messages (all of them when
/// `None`), oldest first, ending with this one.
pub(super) async fn begin_turn(
    tx: &mut sqlx::SqliteConnection,
    thread: Thread,
    message: &str,
    job: &str,
    job_context: &JobContext,
    window: Option<i64>,
) -> ApiResult<Vec<(ModelRole, String)>> {
    let (threads, messages, key) = thread.tables();
    sqlx::query(&format!(
        "INSERT INTO {messages} ({key}, role, text) VALUES (?1, 'user', ?2)"
    ))
    .bind(thread.id())
    .bind(message)
    .execute(&mut *tx)
    .await?;
    sqlx::query(&format!(
        "UPDATE {threads}
            SET status = 'running', error = NULL, job = ?2, request_id = ?3,
                started_at = datetime('now'), finished_at = NULL, stop = NULL
          WHERE {key} = ?1"
    ))
    .bind(thread.id())
    .bind(job)
    .bind(&job_context.request_id)
    .execute(&mut *tx)
    .await?;
    let mut history: Vec<(String, String)> = sqlx::query_as(&format!(
        "SELECT role, text FROM {messages} WHERE {key} = ?1 ORDER BY id DESC LIMIT ?2"
    ))
    .bind(thread.id())
    .bind(window.unwrap_or(-1))
    .fetch_all(&mut *tx)
    .await?;
    history.reverse();
    // The model's side of the conversation starts with the user.
    while history.first().is_some_and(|(role, _)| role == "assistant") {
        history.remove(0);
    }
    Ok(history
        .into_iter()
        .map(|(role, text)| {
            let role = if role == "assistant" {
                ModelRole::Assistant
            } else {
                ModelRole::User
            };
            (role, text)
        })
        .collect())
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: i64,
    role: String,
    text: String,
    suggestion_ids: String,
    created_at: String,
}

/// A thread's current turn status, its failure message, and its messages.
pub(super) async fn load_messages(
    db: &Db,
    thread: Thread,
) -> ApiResult<(ThreadStatus, Option<String>, Vec<ChatMessage>)> {
    let (threads, messages, key) = thread.tables();
    let head: Option<(String, Option<String>)> = sqlx::query_as(&format!(
        "SELECT status, error FROM {threads} WHERE {key} = ?1"
    ))
    .bind(thread.id())
    .fetch_optional(db)
    .await?;
    let (status, error) = head.unwrap_or_else(|| ("idle".into(), None));
    let status = match status.as_str() {
        "running" => ThreadStatus::Running,
        "failed" => ThreadStatus::Failed,
        _ => ThreadStatus::Idle,
    };
    let rows: Vec<MessageRow> = sqlx::query_as(&format!(
        "SELECT id, role, text, suggestion_ids, created_at FROM {messages}
          WHERE {key} = ?1 ORDER BY id"
    ))
    .bind(thread.id())
    .fetch_all(db)
    .await?;
    let messages = rows
        .into_iter()
        .map(|row| ChatMessage {
            id: row.id,
            role: if row.role == "assistant" {
                ChatRole::Assistant
            } else {
                ChatRole::User
            },
            text: row.text,
            created_at: row.created_at,
            suggestion_ids: serde_json::from_str(&row.suggestion_ids).unwrap_or_default(),
        })
        .collect();
    let error = (status == ThreadStatus::Failed).then_some(error).flatten();
    Ok((status, error, messages))
}

async fn load_thread(db: &Db, id: i64) -> ApiResult<SuggestionThread> {
    let (status, error, messages) = load_messages(db, Thread::Note(id)).await?;
    Ok(SuggestionThread {
        suggestion_id: id,
        status,
        error,
        messages,
    })
}

/// Mark turns a previous process left running, on either kind of thread, as
/// failed: nothing is going to finish them.
pub async fn recover(db: &Db) -> Result<u64, sqlx::Error> {
    let mut recovered = 0;
    for threads in ["suggestion_threads", "plan_chat_threads"] {
        recovered += sqlx::query(&format!(
            "UPDATE {threads}
                SET status = 'failed', error = 'interrupted by a server restart',
                    finished_at = datetime('now')
              WHERE status = 'running'"
        ))
        .execute(db)
        .await?
        .rows_affected();
    }
    if recovered > 0 {
        tracing::info!(event = "review_chat.recovered", turns = recovered);
    }
    Ok(recovered)
}

// ── the note, for the model ─────────────────────────────────────────────────

fn percent(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

/// The note under discussion, whole, then the rest of the board by title.
fn render_note(note: &Suggestion, board: &[Suggestion]) -> String {
    let mut out = String::new();
    let author = match note.source {
        SuggestionSource::Rules => "FinPlan's rules",
        SuggestionSource::Ai => "the review model",
    };
    let _ = writeln!(
        out,
        "The note being discussed (#{}, {}, {}, status {}; written by {author}):",
        note.id,
        tag(&note.kind),
        tag(&note.section),
        tag(&note.status)
    );
    let _ = writeln!(out, "Title: {}", note.title);
    if let Some(summary) = &note.summary {
        let _ = writeln!(out, "Summary: {summary}");
    }
    let _ = writeln!(out, "Reasoning: {}", note.reasoning);
    if !note.evidence.is_empty() {
        let _ = writeln!(
            out,
            "Evidence: {}",
            serde_json::to_string(&note.evidence).unwrap_or_default()
        );
    }
    if note.paths.is_empty() {
        let _ = writeln!(out, "Courses of action: none; the note only flags this.");
    }
    for path in &note.paths {
        let _ = write!(out, "Path \"{}\": {}", path.key, path.label);
        if path.recommended {
            out.push_str(" (recommended)");
        }
        if note.applied_path.as_deref() == Some(path.key.as_str()) {
            out.push_str(" (being followed)");
        }
        out.push('\n');
        if let Some(reasoning) = &path.reasoning {
            let _ = writeln!(out, "  Why: {reasoning}");
        }
        if let Some(check) = &path.check {
            let _ = write!(
                out,
                "  Simulated ({} iterations{}): success {} -> {}",
                check.iterations,
                if check.paired { "" } else { ", not paired" },
                percent(check.base.success_rate),
                percent(check.edited.success_rate)
            );
            if let (Some(base), Some(edited)) = (
                check.base.funding_success_rate,
                check.edited.funding_success_rate,
            ) {
                let _ = write!(out, ", funding {} -> {}", percent(base), percent(edited));
            }
            out.push('\n');
        } else if let Some(estimate) = &path.estimate
            && let Some(rate) = estimate.funding_success_rate.or(estimate.success_rate)
        {
            let _ = writeln!(out, "  Estimated (not simulated): ~{}", percent(rate));
        }
        for step in &path.steps {
            let _ = writeln!(
                out,
                "  Step \"{}\": {}{}",
                step.key,
                step.title,
                if step.applied { " (applied)" } else { "" }
            );
            for line in &step.diff {
                let _ = writeln!(
                    out,
                    "    {}: {} -> {}",
                    line.label,
                    line.from.as_deref().unwrap_or("(none)"),
                    line.to.as_deref().unwrap_or("(removed)")
                );
            }
        }
    }
    let others: Vec<&Suggestion> = board
        .iter()
        .filter(|n| n.id != note.id && n.status == SuggestionStatus::Open)
        .collect();
    if !others.is_empty() {
        out.push_str("Other open notes on the board:\n");
        for other in others {
            let _ = writeln!(
                out,
                "- #{} {} · {}",
                other.id,
                tag(&other.kind),
                other.title
            );
        }
    }
    out
}

/// A unit enum's wire tag.
pub(super) fn tag<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

// ── the turn ────────────────────────────────────────────────────────────────

/// One turn, as the POST hands it over.
pub(super) struct Turn {
    pub thread: Thread,
    /// The thread's `job`, already stored as its current turn.
    pub job: String,
    /// The starting request's context: its `request_id` tags every log line.
    pub job_context: JobContext,
    pub submitted: Submitted,
    pub scenario_id: i64,
    /// The run the board reviews: notes are written against it.
    pub run_id: i64,
    pub user: CurrentUser,
    /// The run's snapshot: what the model's changes resolve against.
    pub graph: ScenarioGraph,
    pub context: ReviewContext,
    /// What the thread is about, rendered ([`Subject`]).
    pub subject: String,
    pub history: Vec<(ModelRole, String)>,
    pub check_iterations: usize,
}

impl Turn {
    fn subject(&self) -> Subject<'_> {
        match self.thread {
            Thread::Note(_) => Subject::Note(&self.subject),
            Thread::Plan(_) => Subject::Plan(&self.subject),
        }
    }
}

/// Run an accepted turn in the background. `message_chars` sizes the log.
pub(super) fn start(state: &AppState, reviews: AiReviews, turn: Turn, message_chars: usize) {
    let thread = turn.thread;
    let job = turn.job.clone();
    state
        .telemetry
        .submission(thread.kind(), SubmissionResult::Accepted);
    let span = turn.job_context.span();
    span.in_scope(|| {
        tracing::info!(
            event = thread.event(Ev::Accepted),
            thread_id = thread.id(),
            run_id = turn.run_id,
            chat_job = %job,
            messages = turn.history.len(),
            message_chars,
        );
    });
    let span = span.in_scope(|| tracing::info_span!("review_chat", chat_job = %job));
    let task = tokio::spawn(
        run(state.clone(), reviews, turn)
            .instrument(span.clone())
            .with_current_subscriber(),
    );
    // A turn that panicked would otherwise read as running until a restart.
    let db = state.db.clone();
    tokio::spawn(
        async move {
            if let Err(error) = task.await
                && error.is_panic()
            {
                tracing::error!(event = thread.event(Ev::Panicked), thread_id = thread.id());
                fail(&db, thread, &job, "the model stopped unexpectedly").await;
            }
        }
        .instrument(span)
        .with_current_subscriber(),
    );
}

async fn run(state: AppState, reviews: AiReviews, turn: Turn) {
    let thread = turn.thread;
    let telemetry = state.telemetry.clone();
    let Ok(_permit) = reviews.permits().acquire_owned().await else {
        return;
    };
    telemetry.queue_wait(thread.kind(), QueueExit::Started, turn.submitted.elapsed());
    let _running = telemetry.job_started(thread.kind());
    let mut attempt = Attempt::new(&telemetry, &turn.job_context, turn.submitted.clone());
    let started = Instant::now();
    let model = reviews.model().to_owned();
    let observer = PassMetrics::new(&telemetry, &model);
    let finish = |outcome: AiPassOutcome| {
        telemetry.review_chat_turn(&model, outcome, started.elapsed().as_secs_f64());
    };

    let tools = match Tools::load(
        &state,
        &turn.user,
        turn.scenario_id,
        turn.run_id,
        turn.check_iterations,
        turn.graph.clone(),
    )
    .await
    {
        Ok(tools) => tools,
        Err(_) => {
            telemetry.count_error(Component::ReviewAi, ErrorClass::Database);
            tracing::error!(
                event = thread.event(Ev::StartFailed),
                error_class = "database"
            );
            fail(&state.db, thread, &turn.job, "the chat could not start").await;
            attempt.failure(ErrorClass::Database);
            attempt.finish(Outcome::Failed);
            finish(AiPassOutcome::Failed);
            return;
        }
    };

    let input = ChatInput {
        context: &turn.context,
        subject: turn.subject(),
        history: &turn.history,
    };
    match chat::answer(reviews.client(), &input, &tools, &observer).await {
        Ok(outcome) => {
            record_spend(&telemetry, &model, &outcome);
            let accepted = outcome.drafts.len();
            match store(&state.db, &turn, &outcome).await {
                Ok(Some(stored)) => {
                    let discarded = accepted - stored.len();
                    telemetry
                        .review_ai_suggestions(AiSuggestionOutcome::Stored, stored.len() as u64);
                    telemetry
                        .review_ai_suggestions(AiSuggestionOutcome::Discarded, discarded as u64);
                    tracing::info!(
                        event = thread.event(Ev::Stored),
                        thread_id = thread.id(),
                        run_id = turn.run_id,
                        stop = stop_tag(&outcome.stop),
                        accepted,
                        stored = stored.len(),
                        discarded,
                        turns = outcome.usage.turns,
                        previews = outcome.usage.previews,
                        input_tokens = outcome.usage.input_tokens,
                        output_tokens = outcome.usage.output_tokens,
                        cache_read_input_tokens = outcome.usage.cache_read_input_tokens,
                        cost_usd = outcome.usage.cost_usd,
                        cost_source = outcome.usage.cost_source(),
                        wall_ms = started.elapsed().as_millis() as u64,
                    );
                    attempt.finish(Outcome::Succeeded);
                    finish(pass_outcome(&outcome.stop));
                }
                Ok(None) => {
                    telemetry
                        .review_ai_suggestions(AiSuggestionOutcome::Discarded, accepted as u64);
                    tracing::info!(
                        event = thread.event(Ev::Discarded),
                        thread_id = thread.id(),
                        reason = "thread_gone",
                        accepted,
                        cost_usd = outcome.usage.cost_usd,
                    );
                    attempt.finish(Outcome::Canceled);
                    finish(AiPassOutcome::Superseded);
                }
                Err(_) => {
                    telemetry.count_error(Component::ReviewAi, ErrorClass::Database);
                    tracing::error!(
                        event = thread.event(Ev::StoreFailed),
                        thread_id = thread.id(),
                        error_class = "database",
                    );
                    fail(
                        &state.db,
                        thread,
                        &turn.job,
                        "the answer could not be saved",
                    )
                    .await;
                    attempt.failure(ErrorClass::Database);
                    attempt.finish(Outcome::Failed);
                    finish(AiPassOutcome::Failed);
                }
            }
        }
        Err(error) => {
            let class = match error {
                AiError::Api(_) => ErrorClass::Upstream,
                AiError::Malformed(_) => ErrorClass::Internal,
            };
            telemetry.count_error(Component::ReviewAi, class);
            // The model loop has already scrubbed the key out of the text.
            tracing::warn!(
                event = thread.event(Ev::Failed),
                thread_id = thread.id(),
                error_class = class.as_str(),
                error = %error
            );
            let message = match error {
                AiError::Api(_) => "the model could not be reached",
                AiError::Malformed(_) => "the model's reply could not be read",
            };
            fail(&state.db, thread, &turn.job, message).await;
            attempt.failure(class);
            attempt.finish(Outcome::Failed);
            finish(AiPassOutcome::Failed);
        }
    }
}

/// The chat share of the review model's spend (the `review_ai_*` families
/// already counted it, per request, through [`PassMetrics`]).
fn record_spend(telemetry: &crate::observability::Telemetry, model: &str, outcome: &AiOutcome) {
    let usage = &outcome.usage;
    for (kind, tokens) in [
        (AiTokenType::Input, usage.input_tokens),
        (AiTokenType::Output, usage.output_tokens),
        (AiTokenType::CacheRead, usage.cache_read_input_tokens),
        (AiTokenType::CacheWrite, usage.cache_creation_input_tokens),
    ] {
        telemetry.review_chat_tokens(model, kind, tokens);
    }
    telemetry.review_chat_cost(model, AiCostSource::Reported, usage.cost_reported_usd);
    telemetry.review_chat_cost(model, AiCostSource::Estimated, usage.cost_estimated_usd);
}

/// Record a failed turn, if it is still the thread's current one.
async fn fail(db: &Db, thread: Thread, job: &str, message: &str) {
    let (threads, _, key) = thread.tables();
    let result = sqlx::query(&format!(
        "UPDATE {threads}
            SET status = 'failed', error = ?3, finished_at = datetime('now')
          WHERE {key} = ?1 AND job = ?2 AND status = 'running'"
    ))
    .bind(thread.id())
    .bind(job)
    .bind(message)
    .execute(db)
    .await;
    if result.is_err() {
        tracing::error!(
            event = thread.event(Ev::StatusFailed),
            thread_id = thread.id()
        );
    }
}

/// Store the answer and the notes it added, and mark the thread idle —
/// unless the thread is gone (its note was deleted, or the plan's thread
/// cleared) or no longer this turn's, in which case nothing is written and
/// `None` comes back.
async fn store(db: &Db, turn: &Turn, outcome: &AiOutcome) -> ApiResult<Option<Vec<i64>>> {
    let thread = turn.thread;
    let (threads, messages, key) = thread.tables();
    let mut tx = db.begin().await?;
    // First, so the transaction holds the write lock before it reads.
    let current = sqlx::query(&format!(
        "UPDATE {threads}
            SET status = 'idle', error = NULL, finished_at = datetime('now'), stop = ?3,
                turns = ?4, input_tokens = ?5, output_tokens = ?6, cost_usd = ?7
          WHERE {key} = ?1 AND job = ?2 AND status = 'running'"
    ))
    .bind(thread.id())
    .bind(&turn.job)
    .bind(stop_tag(&outcome.stop))
    .bind(i64::from(outcome.usage.turns))
    .bind(outcome.usage.input_tokens as i64)
    .bind(outcome.usage.output_tokens as i64)
    .bind(outcome.usage.cost_usd)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if current == 0 {
        return Ok(None);
    }

    // As a review pass: dismissed or confirmed notes stay silent, an open one
    // is not duplicated, and one applied from this run is not raised again.
    let standing: HashSet<String> = sqlx::query_scalar(
        "SELECT fingerprint FROM suggestions
          WHERE scenario_id = ?1
            AND (status IN ('open', 'dismissed', 'confirmed')
                 OR (status = 'applied' AND run_id = ?2))",
    )
    .bind(turn.scenario_id)
    .bind(turn.run_id)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();

    let mut seen = HashSet::new();
    let mut stored = Vec::new();
    for draft in &outcome.drafts {
        // Not a review pass's: a later pass does not replace it.
        let new = draft_suggestion(turn.run_id, draft, None, thread.parent());
        if standing.contains(&new.fingerprint) || !seen.insert(new.fingerprint.clone()) {
            continue;
        }
        stored.push(insert(&mut tx, turn.scenario_id, &new).await?);
    }
    sqlx::query(&format!(
        "INSERT INTO {messages} ({key}, role, text, suggestion_ids)
         VALUES (?1, 'assistant', ?2, ?3)"
    ))
    .bind(thread.id())
    .bind(chat::answer_text(outcome))
    .bind(serde_json::to_string(&stored).map_err(|e| ApiError::internal(e.to_string()))?)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(stored))
}
