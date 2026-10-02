//! Plan chat: one thread per plan on its Review tab, about the plan as a
//! whole. The user asks a question or for a change ("retire at 62", "my
//! 401(k) is now $310,000"); the review model answers in the background and
//! may propose changes, which are stored as ordinary notes on the plan's
//! review. The model never edits the plan: every change waits on the board
//! for the user to apply, adjust or dismiss.
//!
//! - `GET /scenarios/{id}/chat`: the thread, empty if nobody has asked yet;
//! - `POST /scenarios/{id}/chat`: add a message and start a turn;
//! - `DELETE /scenarios/{id}/chat`: start over (not while a turn is out).
//!
//! A turn reads what "Chat about this" reads, for the run the board reviews,
//! with the board itself as its subject. Notes are written against that run,
//! so they appear on the board beside the review's own. The plan needs a
//! review first: there is no board to put notes on without one.
//!
//! Each message spends one of the month's plan chat messages
//! (`billing::reserve_ai_plan_chat`), an allowance of its own until model
//! usage is unified across features. The model reads at most
//! [`HISTORY_WINDOW`] messages of the thread. The turn machinery is shared
//! with note threads (`suggestion_chat::Thread`).

use std::fmt::Write as _;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use ts_rs::TS;

use super::preview;
use super::review_ai;
use super::runs::{self, ResultsQuery};
use super::suggestion_chat::{
    self, ChatMessage, ChatRequest, MAX_MESSAGE, ThreadStatus, Turn, tag,
};
use super::suggestions::{self, Suggestion, SuggestionStatus, all_changes};
use crate::auth::session::CurrentUser;
use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::observability::{JobContext, JobKind, Origin};
use crate::runner::telemetry::Submitted;
use crate::state::AppState;
use crate::suggest::ai::ReviewContext;
use crate::suggest::rules;

/// Most messages of the thread the model reads, newest last. Older ones stay
/// on screen; the model works from the plan and the board as they are now.
const HISTORY_WINDOW: i64 = 24;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/scenarios/{scenario_id}/chat",
        get(thread).post(post_message).delete(clear),
    )
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PlanThread {
    pub scenario_id: i64,
    pub status: ThreadStatus,
    /// Why the last turn did not finish; null unless `failed`.
    pub error: Option<String>,
    /// Oldest first.
    pub messages: Vec<ChatMessage>,
}

fn thread_of(scenario_id: i64) -> suggestion_chat::Thread {
    suggestion_chat::Thread::Plan(scenario_id)
}

async fn load(db: &Db, scenario_id: i64) -> ApiResult<PlanThread> {
    let (status, error, messages) =
        suggestion_chat::load_messages(db, thread_of(scenario_id)).await?;
    Ok(PlanThread {
        scenario_id,
        status,
        error,
        messages,
    })
}

/// `GET /scenarios/{id}/chat`.
async fn thread(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<PlanThread>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    Ok(Json(load(&state.db, scenario_id).await?))
}

/// `DELETE /scenarios/{id}/chat`: forget the thread. The notes it added stay
/// on the board.
async fn clear(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<StatusCode> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let removed =
        sqlx::query("DELETE FROM plan_chat_threads WHERE scenario_id = ?1 AND status <> 'running'")
            .bind(scenario_id)
            .execute(&state.db)
            .await?
            .rows_affected();
    if removed == 0 {
        let running: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM plan_chat_threads
                            WHERE scenario_id = ?1 AND status = 'running')",
        )
        .bind(scenario_id)
        .fetch_one(&state.db)
        .await?;
        if running {
            return Err(ApiError::Conflict(
                "the model is still answering the last message".into(),
            ));
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /scenarios/{id}/chat`: add the user's message and start a turn.
async fn post_message(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ChatRequest>,
) -> ApiResult<Json<PlanThread>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    if super::is_draft(&state.db, scenario_id).await? {
        return Err(ApiError::Conflict(
            "a draft is described on New scenario; create it to chat about it here".into(),
        ));
    }
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

    // Notes go on the board, which shows the reviewed run's: that is the run
    // the model reads and writes against.
    let reviewed: Option<i64> =
        sqlx::query_scalar("SELECT run_id FROM suggestion_reviews WHERE scenario_id = ?1")
            .bind(scenario_id)
            .fetch_optional(&state.db)
            .await?;
    let reviewed = reviewed.ok_or_else(|| {
        ApiError::Conflict(
            "review this plan first: the chat's changes are notes on its review".into(),
        )
    })?;
    // Everything the model reads, before the thread is claimed or a message
    // spent: a review whose run is gone fails here, with nothing written.
    let (run_id, graph) =
        preview::base_snapshot(&state.db, scenario_id, &user.id, Some(reviewed)).await?;
    let run_id = run_id.unwrap_or(reviewed);
    let axum::Json(results) = runs::results(
        State(state.clone()),
        user.clone(),
        Path(run_id),
        axum::extract::Query(ResultsQuery { series: None }),
    )
    .await?;
    let board = suggestions::notes_of(&state.db, scenario_id).await?;
    let rule_drafts = rules::review(&graph, &results);
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
    let changed: bool = sqlx::query_scalar(
        "SELECT s.updated_at > r.created_at FROM scenarios s, runs r
          WHERE s.id = ?1 AND r.id = ?2",
    )
    .bind(scenario_id)
    .bind(run_id)
    .fetch_one(&state.db)
    .await?;
    let subject = render_board(run_id, changed, &board);
    let check_iterations = suggestions::check_iterations(&state.db, run_id).await?;
    let pro = crate::billing::entitlements(&state.db, &user.id, &state.config)
        .await?
        .pro;
    let limit = state.config.plan_chat.per_month(pro);

    let job = review_ai::new_job();
    // Captured here, inside the request, so the turn logs under its id.
    let job_context = JobContext::new(
        JobKind::PlanChat,
        Origin::Request,
        &user.id,
        scenario_id,
        scenario_id,
    );
    let thread = thread_of(scenario_id);
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    suggestion_chat::claim(&mut tx, thread).await?;
    // Spent only once the message is sure to be taken.
    crate::billing::reserve_ai_plan_chat(&mut tx, &user.id, limit).await?;
    let history = suggestion_chat::begin_turn(
        &mut tx,
        thread,
        &message,
        &job,
        &job_context,
        Some(HISTORY_WINDOW),
    )
    .await?;
    tx.commit().await?;

    suggestion_chat::start(
        &state,
        reviews,
        Turn {
            thread,
            job,
            job_context,
            submitted: Submitted::now(),
            scenario_id,
            run_id,
            user,
            graph,
            context,
            subject,
            history,
            check_iterations,
        },
        message.chars().count(),
    );

    Ok(Json(load(&state.db, scenario_id).await?))
}

/// The plan's board, for the model: the review it is about, whether the plan
/// has changed since, and each note standing on it.
fn render_board(run_id: i64, changed: bool, board: &[Suggestion]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "The conversation is about the whole plan. Its review is of run {run_id}; \
         notes you add are written against that run's inputs."
    );
    if changed {
        out.push_str(
            "The plan has been edited since that run, so the figures above may be out of \
             date; say so when it matters, and suggest running it again.\n",
        );
    }
    let on_board: Vec<&Suggestion> = board
        .iter()
        .filter(|n| {
            n.run_id == Some(run_id)
                && matches!(n.status, SuggestionStatus::Open | SuggestionStatus::Applied)
        })
        .collect();
    if on_board.is_empty() {
        out.push_str("The review has no notes.\n");
        return out;
    }
    out.push_str("Notes on the review:\n");
    for note in on_board {
        let _ = write!(
            out,
            "- #{} {} · {} · {}",
            note.id,
            tag(&note.kind),
            tag(&note.status),
            note.title
        );
        let paths: Vec<&str> = note.paths.iter().map(|p| p.label.as_str()).collect();
        if !paths.is_empty() {
            let _ = write!(out, " (options: {})", paths.join("; "));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_board_says_so_and_a_stale_plan_is_flagged() {
        let text = render_board(7, true, &[]);
        assert!(text.contains("run 7"));
        assert!(text.contains("edited since that run"));
        assert!(text.contains("no notes"));
        assert!(!render_board(7, false, &[]).contains("edited since"));
    }
}
