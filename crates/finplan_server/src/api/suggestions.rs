//! Suggestions: review notes that carry plan changes.
//!
//! A suggestion is a note (kind, section, title, reasoning, evidence) plus up
//! to [`MAX_PATHS`] paths — courses of action — each a sequence of up to
//! [`MAX_STEPS`] steps, and each step a batch of [`Change`]s written against
//! one run's input snapshot. The user follows one path, whole or step by
//! step in order; a read note has none.
//!
//! Steps are sequential: step k's changes (`expect` included) read the plan
//! as steps 1..k-1 left it, and an entity a step creates (`new_event`,
//! `new_asset`, `new_account`, referenced as `{"$new": key}`) stays in scope
//! for the rest of its path — across requests, too, through the ids the
//! applied steps created (`suggestions.created_json`).
//! Rules write them in bulk (`POST /scenarios/{id}/review`); a model or any
//! other client writes one at a time (`POST /scenarios/{id}/suggestions`).
//! Either way the server renders the diff, and a change batch is checked
//! against the snapshot before it is stored.
//!
//! Applying re-resolves the changes against the *live* plan: `expect` catches
//! an edit made since the run, and the batch is applied in memory and compiled
//! before anything is written. `to: "plan"` then writes through the same SQL
//! halves the edit routes use, in one transaction; `to: "copy"` clones the
//! edited in-memory plan into a new scenario, so no id remapping is needed.
//!
//! A suggestion's fingerprint names what it is about rather than its exact
//! numbers, so dismissing a note (or confirming "it's correct") keeps later
//! reviews from raising it again.

use std::collections::HashSet;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use super::preview::{self, Preview, PreviewStats};
use super::review_ai;
use super::runs::{self, ResultsQuery};
use super::suggestion_paths::{self as paths, PathShape, StepShape, WalkStep};
use crate::auth::session::CurrentUser;
use crate::compile::rows::ScenarioGraph;
use crate::db::Db;
use crate::error::{ApiError, ApiResult, ErrorDetail};
use crate::observability::{JobContext, JobKind, Origin, SubmissionResult};
use crate::runner::telemetry::{Submission, Submitted};
use crate::state::AppState;
use crate::suggest::ai::NoteOutline;
use crate::suggest::ai::ReviewContext;
use crate::suggest::rules::{self, Evidence, Kind, Section};
use crate::suggest::{self, Change, ChangeProblem, Created, DiffLine};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/scenarios/{scenario_id}/review",
            get(latest_review).post(review),
        )
        .route(
            "/scenarios/{scenario_id}/suggestions",
            get(list).post(create),
        )
        .route("/suggestions/{id}/preview", post(preview_suggestion))
        .route("/suggestions/{id}/apply", post(apply))
        .route("/suggestions/{id}/dismiss", post(dismiss))
        .route("/suggestions/{id}/reopen", post(reopen))
}

/// Most suggestions of one review that get a simulated check. Each is a
/// preview of the base run, so this bounds the work a review request does.
const MAX_CHECKS_PER_REVIEW: usize = 4;
/// Iterations for a review's checks, or the base run's count when smaller.
const REVIEW_CHECK_ITERATIONS: i64 = 2_000;

const MAX_TITLE: usize = 160;
const MAX_REASONING: usize = 4_000;
const MAX_SUMMARY: usize = 240;
const MAX_CHANGES: usize = 20;
/// Most paths (courses of action) one suggestion offers.
pub(crate) const MAX_PATHS: usize = 4;
/// Most steps one path takes.
pub(crate) const MAX_STEPS: usize = 4;
const MAX_EVIDENCE: usize = 24;
const MAX_EVIDENCE_NAME: usize = 80;

/// The `funding_diagnostics` fields a `diagnostic` evidence entry may name.
const DIAGNOSTIC_FIELDS: &[&str] = &[
    "iterations",
    "failed",
    "cash_shortfall",
    "event_failure",
    "iteration_limit",
    "failed_solvent",
    "first_shortfall_years",
    "median_first_shortfall_year",
    "shortfall_accounts",
    "event_failures",
    "liquid_depleted_years",
    "median_max_deficit",
    "median_shortfall_years",
    "worst_seed",
];

// ── wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SuggestionStatus {
    Open,
    Applied,
    Dismissed,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SuggestionSource {
    Rules,
    Ai,
}

/// What the author expects the changes to do, before anything is simulated.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionEstimate {
    #[serde(default)]
    pub success_rate: Option<f64>,
    #[serde(default)]
    pub funding_success_rate: Option<f64>,
}

/// The changes simulated against the suggestion's run: the last preview.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionCheck {
    pub iterations: usize,
    pub paired: bool,
    pub base: PreviewStats,
    pub edited: PreviewStats,
}

/// One step of a path: a self-contained batch of changes the user could stop
/// after.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionStep {
    /// Unique within its path: 1-32 of `a-z`, `0-9`, `-`.
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    pub changes: Vec<Change>,
    /// `changes` as the server renders them, read after the path's earlier
    /// steps.
    #[serde(default)]
    pub diff: Vec<DiffLine>,
    #[serde(default)]
    pub applied: bool,
    /// When it was applied (UTC, `YYYY-MM-DD HH:MM:SS` like `resolved_at`);
    /// steps applied in one request share it.
    #[serde(default)]
    pub applied_at: Option<String>,
}

/// One course of action: ordered steps, and what all of them together are
/// expected (`estimate`) or simulated (`check`) to do.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionPath {
    /// Unique within the suggestion: 1-32 of `a-z`, `0-9`, `-`.
    pub key: String,
    pub label: String,
    /// Why this path rather than the others; null when the note's own
    /// reasoning covers it.
    #[serde(default)]
    pub reasoning: Option<String>,
    /// At most one path is; with none, clients treat the first as default.
    #[serde(default)]
    pub recommended: bool,
    /// 1 to [`MAX_STEPS`], in the order they apply.
    pub steps: Vec<SuggestionStep>,
    #[serde(default)]
    pub estimate: Option<SuggestionEstimate>,
    /// The last preview of every step together. A check from an older shape
    /// of the stats is dropped when read, not fatal: previewing again
    /// rewrites it.
    #[serde(default, deserialize_with = "lenient")]
    pub check: Option<SuggestionCheck>,
}

fn lenient<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: for<'a> Deserialize<'a>,
{
    let value = Option::<serde_json::Value>::deserialize(de)?;
    Ok(value.and_then(|v| serde_json::from_value(v).ok()))
}

impl SuggestionPath {
    /// Every change of every step, in order: the path as one batch.
    pub(crate) fn all_changes(&self) -> Vec<Change> {
        self.steps
            .iter()
            .flat_map(|s| s.changes.iter().cloned())
            .collect()
    }

    /// The path a client shows first: the recommended one, else the first.
    pub(crate) fn default_of(paths: &[SuggestionPath]) -> Option<&SuggestionPath> {
        paths
            .iter()
            .find(|p| p.recommended)
            .or_else(|| paths.first())
    }
}

/// Where a note of a draft groups on the Review board (design 2c): the
/// accounts and holdings, the events and parameters, or what the user still has
/// to confirm. A hint from the drafting agent; null on every other note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DraftColumn {
    Portfolio,
    Plan,
    ToConfirm,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Suggestion {
    pub id: i64,
    pub scenario_id: i64,
    /// The run whose inputs and results the suggestion was written against;
    /// null on a note written for a draft, which has no run yet.
    pub run_id: Option<i64>,
    pub source: SuggestionSource,
    /// The rule that wrote it; null for model-written suggestions.
    pub rule: Option<String>,
    pub kind: Kind,
    pub section: Section,
    pub title: String,
    /// The note's lead: one sentence on what it found and why it matters,
    /// shown under the title. Null on notes stored before authors wrote one.
    pub summary: Option<String>,
    /// The working behind it — figures, assumptions, what to check — which
    /// the Review tab keeps behind a disclosure.
    pub reasoning: String,
    pub evidence: Vec<Evidence>,
    /// The courses of action, in the author's order; empty on a read note.
    pub paths: Vec<SuggestionPath>,
    /// The path being followed, once any of its steps is applied; the other
    /// paths are then closed. `status` turns `applied` when every step of it
    /// is.
    pub applied_path: Option<String>,
    pub status: SuggestionStatus,
    pub created_at: String,
    pub resolved_at: Option<String>,
    /// The note whose "Chat about this" thread the model wrote this one from
    /// (see `suggestion_chat`); null otherwise, or once that note is gone.
    pub parent_id: Option<i64>,
    /// A draft note's own key, which a question's `blocks` names; null
    /// otherwise.
    pub note_key: Option<String>,
    /// The keys of the drafting agent's questions this note still waits on.
    /// While any is unanswered the note cannot be applied. Empty once they are
    /// answered, and on every other note.
    pub blocked_by: Vec<String>,
    /// Where the note groups on a draft's Review board.
    pub column: Option<DraftColumn>,
    /// The drafting agent applied this note itself, as a plain fact read from a
    /// document or answered by the user: it is `applied` and shows as "Added".
    pub auto_added: bool,
}

/// A scenario's latest review: the run it read and every note of the plan's,
/// whichever review wrote it — open (carried over from earlier reviews), or
/// acted on: applied, or set aside (dismissed or confirmed, so they can be
/// taken back). Drafts' notes, which have no run, are not the plan's.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Review {
    pub run_id: i64,
    pub reviewed_at: String,
    pub suggestions: Vec<Suggestion>,
    /// The model-written pass over the same run, which lands after the rule
    /// notes; null when the server has no review model configured.
    pub ai: Option<ReviewAi>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ReviewAiStatus {
    Running,
    Done,
    Failed,
}

/// Where the review's model-written pass stands.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReviewAi {
    pub status: ReviewAiStatus,
    /// Why the model stopped (`finished`, `turn_limit`, `suggestion_limit`,
    /// `max_tokens`, `refused`, `interrupted`, `unexpected`) once it has.
    pub stop: Option<String>,
    /// A short public explanation when the pass failed or stopped early.
    pub error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// What the running pass has done so far; empty unless `running`.
    pub activity: Vec<super::ai_activity::AiStep>,
}

#[derive(Debug, Default, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ReviewRequest {
    /// The run to review; defaults to the scenario's latest succeeded run.
    #[serde(default)]
    pub run_id: Option<i64>,
}

/// A suggestion as a client (usually a model) writes it.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionDraft {
    /// The run it was written against; defaults to the latest succeeded run.
    #[serde(default)]
    pub run_id: Option<i64>,
    pub kind: Kind,
    pub section: Section,
    pub title: String,
    /// One sentence shown under the title; optional for older clients.
    #[serde(default)]
    pub summary: Option<String>,
    pub reasoning: String,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub paths: Vec<SuggestionPathDraft>,
}

/// One path as a client writes it; the server renders each step's diff.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionPathDraft {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub recommended: bool,
    /// What every step together is expected to do.
    #[serde(default)]
    pub estimate: Option<SuggestionEstimate>,
    pub steps: Vec<SuggestionStepDraft>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionStepDraft {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyTo {
    Plan,
    Copy,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ApplySuggestion {
    /// The key of the path to follow.
    pub path: String,
    /// Apply the path's not-yet-applied steps up to and including this one,
    /// in order, in one transaction; null applies every remaining step.
    /// `to: "copy"` takes the whole path, so it must be null there.
    #[serde(default)]
    pub through_step: Option<String>,
    #[ts(type = "\"plan\" | \"copy\"")]
    pub to: ApplyTo,
    /// The copy's name (`to: "copy"` only); defaults to one made from the
    /// scenario's name and the suggestion's title.
    #[serde(default)]
    pub name: Option<String>,
}

/// Body of `POST /suggestions/{id}/preview`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct PreviewSuggestion {
    /// The key of the path to simulate.
    pub path: String,
    /// Simulate the path's steps up to and including this one; null
    /// simulates every step, and only that result is stored as the path's
    /// check.
    #[serde(default)]
    pub through_step: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct AppliedSuggestion {
    /// Where the changes landed: the suggestion's scenario, or the new copy.
    pub scenario_id: i64,
    pub suggestion: Suggestion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Dismissed,
    Confirmed,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct DismissSuggestion {
    /// `confirmed` is "it's correct": the user checked and the input stands.
    #[serde(rename = "as")]
    #[ts(rename = "as", type = "\"dismissed\" | \"confirmed\"")]
    pub resolution: Resolution,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<String>,
}

/// The error body of a change batch that cannot be stored or applied: the
/// usual `error`, every problem (`problems`), and the same problems grouped
/// by the path and step whose changes they concern (`by_step`). A problem's
/// `change` indexes that step's `changes`.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct ChangeProblemsBody {
    pub error: ErrorDetail,
    pub problems: Vec<ChangeProblem>,
    pub by_step: Vec<StepProblems>,
}

/// The problems with one step's changes.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct StepProblems {
    pub path: String,
    pub step: String,
    pub problems: Vec<ChangeProblem>,
}

// ── errors ──────────────────────────────────────────────────────────────────

/// An [`ApiError`], or a batch refused with the problems that explain why.
#[derive(Debug)]
pub enum Failure {
    Api(ApiError),
    Problems {
        status: StatusCode,
        code: &'static str,
        message: String,
        by_step: Vec<StepProblems>,
    },
}

impl Failure {
    /// One step's problems: a conflict when any is stale (the plan moved
    /// under it), unprocessable otherwise.
    fn stale_or_invalid(
        path: &str,
        step: &str,
        problems: Vec<ChangeProblem>,
        stale_message: &str,
    ) -> Self {
        let group = StepProblems {
            path: path.to_string(),
            step: step.to_string(),
            problems,
        };
        if group
            .problems
            .iter()
            .any(|p| matches!(p, ChangeProblem::Stale { .. }))
        {
            Failure::Problems {
                status: StatusCode::CONFLICT,
                code: "conflict",
                message: stale_message.to_string(),
                by_step: vec![group],
            }
        } else {
            Failure::invalid(vec![group])
        }
    }

    fn invalid(by_step: Vec<StepProblems>) -> Self {
        Failure::Problems {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "unprocessable",
            message: "the changes cannot be applied to this plan".into(),
            by_step,
        }
    }
}

impl From<ApiError> for Failure {
    fn from(err: ApiError) -> Self {
        Failure::Api(err)
    }
}

impl From<sqlx::Error> for Failure {
    fn from(err: sqlx::Error) -> Self {
        Failure::Api(err.into())
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match self {
            Failure::Api(err) => err.into_response(),
            Failure::Problems {
                status,
                code,
                message,
                by_step,
            } => (
                status,
                Json(ChangeProblemsBody {
                    error: ErrorDetail { code, message },
                    problems: by_step
                        .iter()
                        .flat_map(|o| o.problems.iter().cloned())
                        .collect(),
                    by_step,
                }),
            )
                .into_response(),
        }
    }
}

type Out<T> = Result<T, Failure>;

// ── storage ─────────────────────────────────────────────────────────────────

const COLUMNS: &str = "s.id, s.scenario_id, s.run_id, s.source, s.rule, s.kind, s.section, \
                       s.title, s.summary, s.reasoning, s.evidence_json, s.paths_json, \
                       s.applied_path, s.created_json, s.status, s.created_at, \
                       s.resolved_at, s.parent_id, s.note_key, s.blocked_by_json, \
                       s.board_column, s.auto_added";

/// Fixes first, reads last; ties in the order written.
const ORDER: &str = "CASE s.kind WHEN 'add' THEN 0 WHEN 'fix' THEN 1 WHEN 'check' THEN 2 \
                     WHEN 'stress' THEN 3 ELSE 4 END, s.id";

#[derive(sqlx::FromRow)]
pub(super) struct SuggestionRow {
    id: i64,
    scenario_id: i64,
    run_id: Option<i64>,
    source: String,
    rule: Option<String>,
    kind: String,
    section: String,
    title: String,
    summary: Option<String>,
    reasoning: String,
    evidence_json: String,
    paths_json: String,
    applied_path: Option<String>,
    /// Entities the applied steps created (`suggestion_paths::Created`).
    created_json: String,
    status: String,
    created_at: String,
    resolved_at: Option<String>,
    parent_id: Option<i64>,
    note_key: Option<String>,
    blocked_by_json: String,
    board_column: Option<String>,
    auto_added: i64,
}

/// A unit enum's wire tag, as stored in its TEXT column.
pub(super) fn tag<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn untag<T: for<'de> Deserialize<'de>>(text: &str) -> ApiResult<T> {
    serde_json::from_value(serde_json::Value::String(text.to_string()))
        .map_err(|_| ApiError::internal(format!("unreadable stored value '{text}'")))
}

fn from_json<T: for<'de> Deserialize<'de>>(text: &str) -> ApiResult<T> {
    serde_json::from_str(text).map_err(|_| ApiError::internal("unreadable stored suggestion"))
}

fn to_json<T: Serialize>(value: &T) -> ApiResult<String> {
    serde_json::to_string(value).map_err(|e| ApiError::internal(e.to_string()))
}

impl SuggestionRow {
    pub(super) fn into_suggestion(self) -> ApiResult<Suggestion> {
        Ok(Suggestion {
            id: self.id,
            scenario_id: self.scenario_id,
            run_id: self.run_id,
            source: untag(&self.source)?,
            rule: self.rule,
            kind: untag(&self.kind)?,
            section: untag(&self.section)?,
            title: self.title,
            summary: self.summary,
            reasoning: self.reasoning,
            evidence: from_json(&self.evidence_json)?,
            paths: from_json(&self.paths_json)?,
            applied_path: self.applied_path,
            status: untag(&self.status)?,
            created_at: self.created_at,
            resolved_at: self.resolved_at,
            parent_id: self.parent_id,
            note_key: self.note_key,
            blocked_by: from_json(&self.blocked_by_json)?,
            column: self.board_column.as_deref().map(untag).transpose()?,
            auto_added: self.auto_added != 0,
        })
    }
}

/// A suggestion ready to insert.
pub(super) struct NewSuggestion {
    pub run_id: Option<i64>,
    pub source: SuggestionSource,
    pub rule: Option<String>,
    pub kind: Kind,
    pub section: Section,
    pub title: String,
    pub summary: Option<String>,
    pub reasoning: String,
    pub evidence: Vec<Evidence>,
    pub paths: Vec<SuggestionPath>,
    pub fingerprint: String,
    /// The model-written review pass that wrote it (`suggestions.review_job`).
    pub review_job: Option<String>,
    /// The note a chat thread wrote it from (`suggestions.parent_id`).
    pub parent_id: Option<i64>,
    /// What only the drafting agent's notes carry.
    pub draft: DraftMeta,
}

/// The columns of a note that belong to the drafting agent; every other
/// writer leaves them at their defaults.
#[derive(Debug, Clone, Default)]
pub(super) struct DraftMeta {
    pub note_key: Option<String>,
    pub blocked_by: Vec<String>,
    pub column: Option<DraftColumn>,
    pub auto_added: bool,
}

pub(super) async fn insert(
    conn: &mut sqlx::SqliteConnection,
    scenario_id: i64,
    new: &NewSuggestion,
) -> ApiResult<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO suggestions
            (scenario_id, run_id, source, rule, kind, section, title, reasoning,
             evidence_json, paths_json, fingerprint, review_job, parent_id,
             note_key, blocked_by_json, board_column, auto_added, summary)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18) RETURNING id",
    )
    .bind(scenario_id)
    .bind(new.run_id)
    .bind(tag(&new.source))
    .bind(&new.rule)
    .bind(tag(&new.kind))
    .bind(tag(&new.section))
    .bind(&new.title)
    .bind(&new.reasoning)
    .bind(to_json(&new.evidence)?)
    .bind(to_json(&new.paths)?)
    .bind(&new.fingerprint)
    .bind(&new.review_job)
    .bind(new.parent_id)
    .bind(&new.draft.note_key)
    .bind(to_json(&new.draft.blocked_by)?)
    .bind(new.draft.column.as_ref().map(tag))
    .bind(i64::from(new.draft.auto_added))
    .bind(&new.summary)
    .fetch_one(&mut *conn)
    .await?;
    Ok(id)
}

pub(super) async fn fetch(db: &Db, id: i64) -> ApiResult<Suggestion> {
    let row: SuggestionRow = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM suggestions s WHERE s.id = ?1"
    ))
    .bind(id)
    .fetch_one(db)
    .await?;
    row.into_suggestion()
}

/// Every note of a scenario, whatever its status, in board order.
pub(super) async fn notes_of(db: &Db, scenario_id: i64) -> ApiResult<Vec<Suggestion>> {
    let rows: Vec<SuggestionRow> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM suggestions s WHERE s.scenario_id = ?1 ORDER BY {ORDER}"
    ))
    .bind(scenario_id)
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(SuggestionRow::into_suggestion)
        .collect()
}

/// Most dismissed notes listed for the model, newest first.
const MAX_DISMISSED_LISTED: usize = 30;

/// The notes the user set aside (dismissed, or confirmed on an older
/// client), newest first and at most `MAX_DISMISSED_LISTED`: what the model
/// is told the user does not want raised again.
pub(super) fn dismissed(board: &[Suggestion]) -> Vec<&Suggestion> {
    let mut out: Vec<&Suggestion> = board
        .iter()
        .filter(|n| {
            matches!(
                n.status,
                SuggestionStatus::Dismissed | SuggestionStatus::Confirmed
            )
        })
        .collect();
    out.sort_by(|a, b| b.resolved_at.cmp(&a.resolved_at).then(b.id.cmp(&a.id)));
    out.truncate(MAX_DISMISSED_LISTED);
    out
}

/// Iterations for previews made on a run's review notes: the review's check
/// budget, or the run's own count when smaller.
pub(super) async fn check_iterations(db: &Db, run_id: i64) -> ApiResult<usize> {
    let iterations: i64 = sqlx::query_scalar("SELECT iterations FROM runs WHERE id = ?1")
        .bind(run_id)
        .fetch_one(db)
        .await?;
    Ok(iterations.clamp(1, REVIEW_CHECK_ITERATIONS) as usize)
}

/// The suggestion, if it belongs to one of the caller's scenarios; another
/// user's reads as not found, so ids are not probeable.
pub(super) async fn owned(db: &Db, id: i64, user_id: &str) -> ApiResult<SuggestionRow> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM suggestions s JOIN scenarios c ON c.id = s.scenario_id
          WHERE s.id = ?1 AND c.user_id = ?2"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound("suggestion"))
}

/// What a suggestion is about, independent of the numbers it quotes: the
/// author (rule, or "ai"), the kind, and either the fields its changes touch
/// (every step of every path together) or — for a note without changes — its
/// title with every figure blanked. "USAA empties in 97 of 99 failures" and
/// "… 12 of 14 …" are one note.
pub(crate) fn fingerprint<'a>(
    rule: Option<&str>,
    kind: Kind,
    changes: impl IntoIterator<Item = &'a Change>,
    title: &str,
) -> String {
    let mut touched: Vec<String> = changes
        .into_iter()
        .map(|c| {
            format!(
                "{}{}{}",
                target_key(&c.target),
                c.created_signature(),
                c.path
            )
        })
        .collect();
    touched.sort();
    touched.dedup();

    let mut hash = Sha256::new();
    hash.update(rule.unwrap_or("ai"));
    hash.update([0]);
    hash.update(tag(&kind));
    hash.update([0]);
    if touched.is_empty() {
        hash.update(blank_figures(title));
    } else {
        for key in touched {
            hash.update(key);
            hash.update([0]);
        }
    }
    format!("{:x}", hash.finalize())
}

/// A change's target as the fingerprint reads it. An entity the batch creates
/// (`new_event`, `new_asset`, …) is named by its kind alone: its key is the
/// author's own label, not something the note is about.
fn target_key(target: &suggest::ChangeTarget) -> String {
    match serde_json::to_value(target) {
        Ok(serde_json::Value::Object(map)) if map.len() == 1 => {
            let (kind, id) = map.into_iter().next().expect("one entry");
            if kind.starts_with("new_") {
                kind
            } else {
                format!("{{\"{kind}\":{id}}}")
            }
        }
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

/// Every change of every step of every path, for the fingerprint.
pub(crate) fn all_changes(paths: &[SuggestionPath]) -> Vec<Change> {
    paths.iter().flat_map(SuggestionPath::all_changes).collect()
}

/// Each run of digits (with the separators inside a figure) becomes `#`.
fn blank_figures(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            while chars
                .peek()
                .is_some_and(|n| n.is_ascii_digit() || *n == ',' || *n == '.')
            {
                chars.next();
            }
            out.push('#');
        } else {
            out.push(c);
        }
    }
    out
}

// ── review ──────────────────────────────────────────────────────────────────

/// `POST /scenarios/{id}/review`: run the rules over a run and store their
/// notes. Open notes from earlier reviews stay: a rule note the rules raise
/// again is refreshed in place, one they no longer raise goes with its
/// condition, and every other note is carried over, its paths walked against
/// the plan as it now stands (one that no longer fits drops out). With
/// a review model configured, a model-written pass over the same run starts in
/// the background (see `review_ai`) and `Review.ai` reports it as running.
async fn review(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<ReviewRequest>,
) -> Out<Json<Review>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let (run_id, graph) =
        preview::base_snapshot(&state.db, scenario_id, &user.id, body.run_id).await?;
    let run_id = run_id.ok_or_else(|| {
        ApiError::Conflict("a draft has no run to review; create and run the plan first".into())
    })?;
    let Json(results) = runs::results(
        State(state.clone()),
        user.clone(),
        Path(run_id),
        Query(ResultsQuery { series: None }),
    )
    .await?;
    let drafts = rules::review(&graph, &results);

    // The board's open notes. Rule notes are matched to this review's by
    // fingerprint below; the rest (model-written, from a chat, a client's)
    // are carried over if their paths still fit the plan.
    let board = notes_of(&state.db, scenario_id).await?;
    let open: Vec<Suggestion> = board
        .iter()
        .filter(|n| n.status == SuggestionStatus::Open && n.run_id.is_some())
        .cloned()
        .collect();
    let open_rules: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, fingerprint FROM suggestions
          WHERE scenario_id = ?1 AND source = 'rules' AND status = 'open'
            AND applied_path IS NULL",
    )
    .bind(scenario_id)
    .fetch_all(&state.db)
    .await?;
    let mut carried = Vec::new();
    let mut dropped = Vec::new();
    for note in open
        .iter()
        .filter(|n| n.source != SuggestionSource::Rules && n.applied_path.is_none())
    {
        match rewalk(&state.db, &user.id, &graph, note).await? {
            Some(note) => carried.push(note),
            None => dropped.push(note.id),
        }
    }

    // The model reads the rule notes and the open ones too, so it adds to
    // them rather than repeating them, and the ones the user dismissed, so
    // it does not raise them again.
    // A hosted guest gets the rule notes only: no model pass (spec 17).
    let context = state.review_ai_for(&user).is_some().then(|| {
        ReviewContext::build(&graph, &results, &drafts)
            .with_open_notes(
                carried
                    .iter()
                    .chain(open.iter().filter(|n| n.applied_path.is_some()))
                    .map(outline),
            )
            .with_dismissed_notes(dismissed(&board).into_iter().map(outline))
    });

    // Dismissed or confirmed notes stay silent; one already applied from
    // this same run is not raised again against it, and neither is one whose
    // path is part-way applied (it stays on the board as it is).
    let silenced: HashSet<String> = sqlx::query_scalar(
        "SELECT fingerprint FROM suggestions
          WHERE scenario_id = ?1
            AND (status IN ('dismissed', 'confirmed')
                 OR (status = 'applied' AND run_id = ?2)
                 OR (status = 'open' AND applied_path IS NOT NULL))",
    )
    .bind(scenario_id)
    .bind(run_id)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    let run_iterations: i64 = sqlx::query_scalar("SELECT iterations FROM runs WHERE id = ?1")
        .bind(run_id)
        .fetch_one(&state.db)
        .await?;
    let check_iterations = run_iterations.clamp(1, REVIEW_CHECK_ITERATIONS) as usize;

    let mut seen = HashSet::new();
    let mut prepared = Vec::new();
    for draft in drafts {
        let fingerprint = fingerprint(
            Some(draft.rule),
            draft.kind,
            draft.paths.iter().flat_map(|p| p.changes()),
            &draft.title,
        );
        if silenced.contains(&fingerprint) || !seen.insert(fingerprint.clone()) {
            continue;
        }
        let had_paths = !draft.paths.is_empty();
        let mut paths = Vec::with_capacity(draft.paths.len());
        for path in draft.paths {
            let steps: Vec<WalkStep<'_>> = path
                .steps
                .iter()
                .map(|s| WalkStep {
                    key: s.key,
                    changes: &s.changes,
                })
                .collect();
            match paths::walk(&state.db, &user.id, graph.clone(), &steps, &Created::new()).await? {
                Ok(walked) => paths.push(SuggestionPath {
                    key: path.key.to_string(),
                    label: path.label,
                    reasoning: path.reasoning,
                    recommended: path.recommended,
                    steps: path
                        .steps
                        .into_iter()
                        .zip(walked.diffs)
                        .map(|(step, diff)| SuggestionStep {
                            key: step.key.to_string(),
                            title: step.title,
                            reasoning: step.reasoning,
                            changes: step.changes,
                            diff,
                            applied: false,
                            applied_at: None,
                        })
                        .collect(),
                    estimate: None,
                    check: None,
                }),
                Err(failure) => {
                    // Rules only write changes that resolve against the graph
                    // they read; one that does not is a rule bug, not the
                    // user's problem. Leave the path out.
                    tracing::warn!(
                        event = "review.rule_change_unresolved",
                        rule = draft.rule,
                        path = path.key,
                        step = %failure.step,
                        problems = failure.problems.len()
                    );
                }
            }
        }
        if had_paths && paths.is_empty() {
            continue;
        }
        prepared.push(NewSuggestion {
            run_id: Some(run_id),
            source: SuggestionSource::Rules,
            rule: Some(draft.rule.to_string()),
            kind: draft.kind,
            section: draft.section,
            title: draft.title,
            summary: Some(draft.summary),
            reasoning: draft.reasoning,
            evidence: draft.evidence,
            paths,
            fingerprint,
            review_job: None,
            parent_id: None,
            draft: DraftMeta::default(),
        });
    }

    // Combined checks (every step of a path), within the review's budget:
    // each note's recommended or first path, the rules' notes in board order
    // and then the carried ones, whose old checks were of an older run.
    let mut budget = MAX_CHECKS_PER_REVIEW;
    for paths in prepared
        .iter_mut()
        .map(|n| &mut n.paths)
        .chain(carried.iter_mut().map(|n| &mut n.paths))
        .filter(|p| !p.is_empty())
    {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let Some(key) = SuggestionPath::default_of(paths).map(|p| p.key.clone()) else {
            continue;
        };
        let path = paths
            .iter_mut()
            .find(|p| p.key == key)
            .expect("the default is one of the paths");
        path.check = check(
            &state,
            &user,
            scenario_id,
            run_id,
            &path.all_changes(),
            check_iterations,
        )
        .await?;
    }

    let job = context.as_ref().map(|_| review_ai::new_job());
    // Captured here, inside the request, so the background pass logs under
    // this request's id. `job_id` is the reviewed run: a pass has no row id of
    // its own (its uuid is `ai_job`, logged alongside).
    let job_context = job.as_ref().map(|_| {
        JobContext::new(
            JobKind::ReviewAi,
            Origin::Request,
            &user.id,
            scenario_id,
            run_id,
        )
    });
    let request_id = job_context.as_ref().and_then(|c| c.request_id.clone());

    let mut tx = state.db.begin().await?;
    // A rule note raised again keeps its row (and its chat), with this run's
    // figures; one not raised again is gone with its condition.
    let mut refreshed = HashSet::new();
    for new in &prepared {
        let again = open_rules
            .iter()
            .find(|(id, fp)| *fp == new.fingerprint && !refreshed.contains(id));
        match again {
            Some((id, _)) => {
                refresh(&mut tx, *id, new).await?;
                refreshed.insert(*id);
            }
            None => {
                insert(&mut tx, scenario_id, new).await?;
            }
        }
    }
    for id in open_rules
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !refreshed.contains(id))
        .chain(dropped)
    {
        sqlx::query("DELETE FROM suggestions WHERE id = ?1 AND status = 'open'")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    for note in &carried {
        sqlx::query("UPDATE suggestions SET run_id = ?2, paths_json = ?3 WHERE id = ?1")
            .bind(note.id)
            .bind(run_id)
            .bind(to_json(&note.paths)?)
            .execute(&mut *tx)
            .await?;
    }
    // A new job supersedes any pass still running for this scenario: that
    // pass is no longer the review's job, so it cannot write its notes.
    sqlx::query(
        "INSERT INTO suggestion_reviews
            (scenario_id, run_id, reviewed_at, ai_status, ai_job, ai_started_at, ai_request_id)
         VALUES (?1, ?2, datetime('now'), ?3, ?4, CASE WHEN ?4 IS NULL THEN NULL
                                                        ELSE datetime('now') END, ?5)
         ON CONFLICT(scenario_id) DO UPDATE SET run_id = excluded.run_id,
                                                reviewed_at = excluded.reviewed_at,
                                                ai_status = excluded.ai_status,
                                                ai_job = excluded.ai_job,
                                                ai_started_at = excluded.ai_started_at,
                                                ai_request_id = excluded.ai_request_id,
                                                ai_finished_at = NULL,
                                                ai_error = NULL,
                                                ai_stop = NULL,
                                                ai_turns = NULL,
                                                ai_input_tokens = NULL,
                                                ai_output_tokens = NULL,
                                                ai_cost_usd = NULL",
    )
    .bind(scenario_id)
    .bind(run_id)
    .bind(if job.is_some() { "running" } else { "off" })
    .bind(&job)
    .bind(&request_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    if let (Some(reviews), Some(context), Some(job), Some(job_context)) =
        (&state.review_ai, context, job, job_context)
    {
        state
            .telemetry
            .submission(JobKind::ReviewAi, SubmissionResult::Accepted);
        reviews.start(
            &state,
            review_ai::Pass {
                job,
                job_context,
                submitted: Submitted::now(),
                scenario_id,
                run_id,
                user: user.clone(),
                graph,
                context,
                check_iterations,
            },
        );
    }

    let review = load_review(&state, scenario_id)
        .await?
        .ok_or_else(|| ApiError::internal("review vanished after it was stored"))?;
    Ok(Json(review))
}

/// A stored note as the model is shown it.
pub(super) fn outline(note: &Suggestion) -> NoteOutline<'_> {
    (
        note.kind,
        note.section,
        note.title.as_str(),
        all_changes(&note.paths),
    )
}

/// A rule note raised again, given this review's run, wording and paths.
async fn refresh(conn: &mut sqlx::SqliteConnection, id: i64, new: &NewSuggestion) -> ApiResult<()> {
    sqlx::query(
        "UPDATE suggestions
            SET run_id = ?2, title = ?3, summary = ?4, reasoning = ?5, evidence_json = ?6,
                paths_json = ?7, fingerprint = ?8
          WHERE id = ?1",
    )
    .bind(id)
    .bind(new.run_id)
    .bind(&new.title)
    .bind(&new.summary)
    .bind(&new.reasoning)
    .bind(to_json(&new.evidence)?)
    .bind(to_json(&new.paths)?)
    .bind(&new.fingerprint)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// An open note carried into a new review: each path walked against the
/// plan as it now stands, with fresh diffs and no check (that was of an
/// older run). A path that no longer fits — the plan moved under it — drops
/// out; `None` when a note that had paths has none left.
async fn rewalk(
    db: &Db,
    user_id: &str,
    graph: &ScenarioGraph,
    note: &Suggestion,
) -> ApiResult<Option<Suggestion>> {
    let mut paths = Vec::with_capacity(note.paths.len());
    for path in &note.paths {
        let steps: Vec<WalkStep<'_>> = path
            .steps
            .iter()
            .map(|s| WalkStep {
                key: &s.key,
                changes: &s.changes,
            })
            .collect();
        if let Ok(walked) = paths::walk(db, user_id, graph.clone(), &steps, &Created::new()).await?
        {
            let mut path = path.clone();
            for (step, diff) in path.steps.iter_mut().zip(walked.diffs) {
                step.diff = diff;
            }
            path.check = None;
            paths.push(path);
        }
    }
    if !note.paths.is_empty() && paths.is_empty() {
        return Ok(None);
    }
    Ok(Some(Suggestion {
        paths,
        ..note.clone()
    }))
}

/// Preview `changes` against the run for a review note. A preview that is
/// refused (capacity, a batch problem) leaves the note unchecked rather than
/// failing the review; only the server's own failures propagate.
async fn check(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    run_id: i64,
    changes: &[Change],
    iterations: usize,
) -> ApiResult<Option<SuggestionCheck>> {
    match preview::run_preview(
        state,
        user,
        scenario_id,
        Some(run_id),
        changes,
        Some(iterations),
        None,
    )
    .await
    {
        Ok(preview) => Ok(check_of(&preview)),
        Err(err @ (ApiError::Database(_) | ApiError::Internal(_))) => Err(err),
        Err(err) => {
            tracing::info!(event = "review.check_skipped", reason = %err);
            Ok(None)
        }
    }
}

pub(super) fn check_of(preview: &Preview) -> Option<SuggestionCheck> {
    if !preview.problems.is_empty() {
        return None;
    }
    Some(SuggestionCheck {
        iterations: preview.iterations,
        paired: preview.paired,
        base: preview.base.clone()?,
        edited: preview.edited.clone()?,
    })
}

#[derive(sqlx::FromRow)]
struct ReviewRow {
    run_id: i64,
    reviewed_at: String,
    ai_status: String,
    ai_stop: Option<String>,
    ai_error: Option<String>,
    ai_started_at: Option<String>,
    ai_finished_at: Option<String>,
}

impl ReviewRow {
    fn ai(&self) -> Option<ReviewAi> {
        let status = match self.ai_status.as_str() {
            "running" => ReviewAiStatus::Running,
            "done" => ReviewAiStatus::Done,
            "failed" => ReviewAiStatus::Failed,
            _ => return None,
        };
        Some(ReviewAi {
            status,
            stop: self.ai_stop.clone(),
            error: self.ai_error.clone(),
            started_at: self.ai_started_at.clone().unwrap_or_default(),
            finished_at: self.ai_finished_at.clone(),
            activity: Vec::new(),
        })
    }
}

async fn load_review(state: &AppState, scenario_id: i64) -> ApiResult<Option<Review>> {
    let db = &state.db;
    let latest: Option<ReviewRow> = sqlx::query_as(
        "SELECT run_id, reviewed_at, ai_status, ai_stop, ai_error, ai_started_at, ai_finished_at
           FROM suggestion_reviews WHERE scenario_id = ?1",
    )
    .bind(scenario_id)
    .fetch_optional(db)
    .await?;
    let Some(latest) = latest else {
        return Ok(None);
    };
    let run_id = latest.run_id;
    let rows: Vec<SuggestionRow> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM suggestions s
          WHERE s.scenario_id = ?1
            AND s.run_id IS NOT NULL
            AND s.status IN ('open', 'applied', 'dismissed', 'confirmed')
          ORDER BY {ORDER}"
    ))
    .bind(scenario_id)
    .fetch_all(db)
    .await?;
    let suggestions = rows
        .into_iter()
        .map(SuggestionRow::into_suggestion)
        .collect::<ApiResult<_>>()?;
    let mut ai = latest.ai();
    if let (Some(ai), Some(reviews)) = (&mut ai, &state.review_ai)
        && ai.status == ReviewAiStatus::Running
    {
        ai.activity = reviews.review_steps(scenario_id);
    }
    Ok(Some(Review {
        run_id,
        ai,
        reviewed_at: latest.reviewed_at,
        suggestions,
    }))
}

/// `GET /scenarios/{id}/review`: the latest review, or `null` if none yet.
async fn latest_review(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Option<Review>>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    Ok(Json(load_review(&state, scenario_id).await?))
}

// ── list and create ─────────────────────────────────────────────────────────

/// `GET /scenarios/{id}/suggestions?status=`: every suggestion of the
/// scenario, or those with one status.
async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Vec<Suggestion>>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let status = query
        .status
        .as_deref()
        .map(|s| {
            untag::<SuggestionStatus>(s)
                .map_err(|_| ApiError::bad_request(format!("unknown status '{s}'")))
        })
        .transpose()?;
    let rows: Vec<SuggestionRow> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM suggestions s
          WHERE s.scenario_id = ?1 AND (?2 IS NULL OR s.status = ?2)
          ORDER BY {ORDER}"
    ))
    .bind(scenario_id)
    .bind(status.as_ref().map(tag))
    .fetch_all(&state.db)
    .await?;
    let suggestions = rows
        .into_iter()
        .map(SuggestionRow::into_suggestion)
        .collect::<ApiResult<_>>()?;
    Ok(Json(suggestions))
}

/// `POST /scenarios/{id}/suggestions`: store one client-written suggestion.
/// Each path's steps must resolve in order against the run's snapshot and
/// leave a plan that compiles; its evidence must point at things the run has.
async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(draft): Json<SuggestionDraft>,
) -> Out<(StatusCode, Json<Suggestion>)> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let title = draft.title.trim().to_string();
    let reasoning = draft.reasoning.trim().to_string();
    validate_text(&title, &reasoning)?;
    let summary = trimmed(draft.summary);
    validate_summary(summary.as_deref())?;
    if draft.evidence.len() > MAX_EVIDENCE {
        return Err(ApiError::unprocessable(format!(
            "a suggestion cites at most {MAX_EVIDENCE} pieces of evidence"
        ))
        .into());
    }
    let shapes: Vec<PathShape<'_>> = draft.paths.iter().map(path_shape).collect();
    if let Some(problem) = paths::shape_problems(draft.kind, &shapes, MAX_CHANGES)
        .into_iter()
        .next()
    {
        return Err(ApiError::unprocessable(problem).into());
    }
    for path in &draft.paths {
        if let Some(estimate) = &path.estimate {
            validate_estimate(estimate)?;
        }
    }

    let (run_id, graph) =
        preview::base_snapshot(&state.db, scenario_id, &user.id, draft.run_id).await?;
    validate_evidence(&graph, &draft.evidence)?;

    // Every path is walked, and every path's problems reported at once.
    let mut by_step = Vec::new();
    let mut paths = Vec::with_capacity(draft.paths.len());
    for path in draft.paths {
        let steps: Vec<WalkStep<'_>> = path
            .steps
            .iter()
            .map(|s| WalkStep {
                key: &s.key,
                changes: &s.changes,
            })
            .collect();
        let walked =
            match paths::walk(&state.db, &user.id, graph.clone(), &steps, &Created::new()).await? {
                Ok(walked) => walked,
                Err(failure) => {
                    by_step.push(StepProblems {
                        path: path.key,
                        step: failure.step,
                        problems: failure.problems,
                    });
                    continue;
                }
            };
        paths.push(SuggestionPath {
            label: path.label.trim().to_string(),
            reasoning: trimmed(path.reasoning),
            recommended: path.recommended,
            steps: path
                .steps
                .into_iter()
                .zip(walked.diffs)
                .map(|(step, diff)| SuggestionStep {
                    key: step.key,
                    title: step.title.trim().to_string(),
                    reasoning: trimmed(step.reasoning),
                    changes: step.changes,
                    diff,
                    applied: false,
                    applied_at: None,
                })
                .collect(),
            key: path.key,
            estimate: path.estimate,
            check: None,
        });
    }
    if !by_step.is_empty() {
        return Err(Failure::invalid(by_step));
    }

    let fingerprint = fingerprint(None, draft.kind, &all_changes(&paths), &title);
    let standing: Option<String> = sqlx::query_scalar(
        "SELECT status FROM suggestions
          WHERE scenario_id = ?1 AND fingerprint = ?2
            AND status IN ('open', 'dismissed', 'confirmed')
          LIMIT 1",
    )
    .bind(scenario_id)
    .bind(&fingerprint)
    .fetch_optional(&state.db)
    .await?;
    if let Some(status) = standing {
        return Err(ApiError::Conflict(match status.as_str() {
            "open" => "an open suggestion already proposes this".into(),
            _ => format!("a suggestion like this was {status}; it is not raised again"),
        })
        .into());
    }

    let new = NewSuggestion {
        run_id,
        source: SuggestionSource::Ai,
        rule: None,
        kind: draft.kind,
        section: draft.section,
        title,
        summary,
        reasoning,
        evidence: draft.evidence,
        paths,
        fingerprint,
        review_job: None,
        parent_id: None,
        draft: DraftMeta::default(),
    };
    let mut conn = state.db.acquire().await?;
    let id = insert(&mut conn, scenario_id, &new).await?;
    drop(conn);
    Ok((StatusCode::CREATED, Json(fetch(&state.db, id).await?)))
}

fn path_shape(path: &SuggestionPathDraft) -> PathShape<'_> {
    PathShape {
        key: &path.key,
        label: &path.label,
        reasoning: path.reasoning.as_deref(),
        recommended: path.recommended,
        steps: path
            .steps
            .iter()
            .map(|s| StepShape {
                key: &s.key,
                title: &s.title,
                reasoning: s.reasoning.as_deref(),
                changes: &s.changes,
            })
            .collect(),
    }
}

fn trimmed(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

fn validate_text(title: &str, reasoning: &str) -> ApiResult<()> {
    if title.is_empty() {
        return Err(ApiError::unprocessable("a suggestion needs a title"));
    }
    if title.chars().count() > MAX_TITLE {
        return Err(ApiError::unprocessable(format!(
            "a title is at most {MAX_TITLE} characters"
        )));
    }
    if reasoning.is_empty() {
        return Err(ApiError::unprocessable("a suggestion needs its reasoning"));
    }
    if reasoning.chars().count() > MAX_REASONING {
        return Err(ApiError::unprocessable(format!(
            "reasoning is at most {MAX_REASONING} characters"
        )));
    }
    Ok(())
}

fn validate_summary(summary: Option<&str>) -> ApiResult<()> {
    match summary {
        Some(s) if s.chars().count() > MAX_SUMMARY => Err(ApiError::unprocessable(format!(
            "a summary is at most {MAX_SUMMARY} characters"
        ))),
        Some(s) if s.contains('\n') => Err(ApiError::unprocessable("a summary is one line")),
        _ => Ok(()),
    }
}

fn validate_estimate(estimate: &SuggestionEstimate) -> ApiResult<()> {
    for rate in [estimate.success_rate, estimate.funding_success_rate]
        .into_iter()
        .flatten()
    {
        if !(0.0..=1.0).contains(&rate) {
            return Err(ApiError::unprocessable(
                "an estimated rate is a fraction between 0 and 1",
            ));
        }
    }
    Ok(())
}

/// Every evidence entry must point at something the run has: an account or
/// event of its snapshot, a year inside its horizon, a diagnostics field.
fn validate_evidence(graph: &ScenarioGraph, evidence: &[Evidence]) -> ApiResult<()> {
    let first_year: i64 = graph
        .scenario
        .start_date
        .get(0..4)
        .and_then(|y| y.parse().ok())
        .ok_or_else(|| ApiError::internal("run snapshot has no start year"))?;
    let last_year = first_year + graph.scenario.duration_years;
    let accounts: HashSet<i64> = graph.accounts.iter().map(|a| a.id).collect();
    let events: HashSet<i64> = graph.events.iter().map(|e| e.id).collect();

    let year_ok = |year: i64| (first_year..=last_year).contains(&year);
    let bad = |index: usize, why: String| {
        Err(ApiError::unprocessable(format!("evidence {index}: {why}")))
    };
    for (index, entry) in evidence.iter().enumerate() {
        match entry {
            Evidence::Ledger {
                year,
                event_id,
                account_id,
            } => {
                if !year_ok(*year) {
                    return bad(index, format!("{year} is outside the run"));
                }
                if let Some(id) = event_id
                    && !events.contains(id)
                {
                    return bad(index, format!("event {id} is not in the run"));
                }
                if let Some(id) = account_id
                    && !accounts.contains(id)
                {
                    return bad(index, format!("account {id} is not in the run"));
                }
            }
            Evidence::AccountSeries {
                account_id,
                date,
                value,
            } => {
                if !accounts.contains(account_id) {
                    return bad(index, format!("account {account_id} is not in the run"));
                }
                let year = date.get(0..4).and_then(|y| y.parse::<i64>().ok());
                if !year.is_some_and(year_ok) {
                    return bad(index, format!("'{date}' is not a date inside the run"));
                }
                if !value.is_finite() {
                    return bad(index, "the value is not a number".into());
                }
            }
            Evidence::Stat { name, value } => {
                if name.trim().is_empty() || name.chars().count() > MAX_EVIDENCE_NAME {
                    return bad(
                        index,
                        format!("a stat name is 1 to {MAX_EVIDENCE_NAME} characters"),
                    );
                }
                if !value.is_finite() {
                    return bad(index, "the value is not a number".into());
                }
            }
            Evidence::Diagnostic { field, value } => {
                if !DIAGNOSTIC_FIELDS.contains(&field.as_str()) {
                    return bad(
                        index,
                        format!("'{field}' is not a funding_diagnostics field"),
                    );
                }
                if !value.is_finite() {
                    return bad(index, "the value is not a number".into());
                }
            }
            // Only the AI loops write these, and they check them against what
            // they came from (a document, an answer, a tool call).
            Evidence::Document { .. }
            | Evidence::Answer { .. }
            | Evidence::Description { .. }
            | Evidence::Computed { .. } => {
                return bad(index, "this kind of evidence is not accepted here".into());
            }
        }
    }
    Ok(())
}

// ── preview, apply, dismiss ─────────────────────────────────────────────────

/// `POST /suggestions/{id}/preview`: simulate a path's steps, from its first
/// through `through_step` (all of them when null), as one batch against the
/// suggestion's run — the latest succeeded one, if that run is gone. The
/// result of the whole path is kept as its check.
async fn preview_suggestion(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<PreviewSuggestion>,
) -> ApiResult<Json<Preview>> {
    let row = owned(&state.db, id, &user.id).await?;
    let paths: Vec<SuggestionPath> = from_json(&row.paths_json)?;
    let path = &paths[path_index(&paths, &body.path)?];
    let end = match &body.through_step {
        None => path.steps.len(),
        Some(key) => step_index(path, key)? + 1,
    };
    let changes: Vec<Change> = path.steps[..end]
        .iter()
        .flat_map(|s| s.changes.iter().cloned())
        .collect();
    if changes.is_empty() {
        return Err(ApiError::unprocessable(
            "this path has no changes to preview",
        ));
    }
    let mut decision = Submission::new(&state.telemetry, JobKind::Preview);
    let mut result = preview::run_preview(
        &state,
        &user,
        row.scenario_id,
        row.run_id,
        &changes,
        None,
        Some(&mut decision),
    )
    .await;
    if row.run_id.is_some() && matches!(result, Err(ApiError::NotFound("run"))) {
        result = preview::run_preview(
            &state,
            &user,
            row.scenario_id,
            None,
            &changes,
            None,
            Some(&mut decision),
        )
        .await;
    }
    decision.result(&result);
    let preview = result?;
    if end == path.steps.len()
        && let Some(check) = check_of(&preview)
    {
        store_check(&state.db, id, &body.path, check).await?;
    }
    Ok(Json(preview))
}

/// The path with this key, or not found.
fn path_index(paths: &[SuggestionPath], key: &str) -> ApiResult<usize> {
    paths
        .iter()
        .position(|p| p.key == key)
        .ok_or(ApiError::NotFound("path"))
}

/// The step of `path` with this key, or not found.
fn step_index(path: &SuggestionPath, key: &str) -> ApiResult<usize> {
    path.steps
        .iter()
        .position(|s| s.key == key)
        .ok_or(ApiError::NotFound("step"))
}

/// Keep `check` as one path's latest combined preview. Read-modify-write in
/// one transaction, so two previews of different paths both land.
async fn store_check(db: &Db, id: i64, key: &str, check: SuggestionCheck) -> ApiResult<()> {
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let json: String = sqlx::query_scalar("SELECT paths_json FROM suggestions WHERE id = ?1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let mut paths: Vec<SuggestionPath> = from_json(&json)?;
    if let Some(path) = paths.iter_mut().find(|p| p.key == key) {
        path.check = Some(check);
        sqlx::query("UPDATE suggestions SET paths_json = ?2 WHERE id = ?1")
            .bind(id)
            .bind(to_json(&paths)?)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// `POST /suggestions/{id}/apply`: follow a path — its remaining steps, or
/// those through `through_step` — writing them into the plan in order, in one
/// transaction; or, whole and before any step is applied, into a new copy of
/// the plan. Once a path is started the others are closed. Nothing is run;
/// the client starts the run.
pub(super) async fn apply(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<ApplySuggestion>,
) -> Out<Json<AppliedSuggestion>> {
    let row = owned(&state.db, id, &user.id).await?;
    if row.status != "open" {
        return Err(
            ApiError::Conflict(format!("this suggestion is already {}", row.status)).into(),
        );
    }
    let waiting: Vec<String> = from_json(&row.blocked_by_json)?;
    if !waiting.is_empty() {
        return Err(ApiError::Conflict(format!(
            "this note waits on your answer to: {}",
            waiting.join(", ")
        ))
        .into());
    }
    let mut paths: Vec<SuggestionPath> = from_json(&row.paths_json)?;
    let p = path_index(&paths, &body.path)?;
    if let Some(started) = &row.applied_path
        && *started != body.path
    {
        return Err(ApiError::Conflict(format!(
            "path \"{started}\" is already being followed; the others are closed"
        ))
        .into());
    }
    let path = &paths[p];
    let end = match &body.through_step {
        None => path.steps.len(),
        Some(key) => {
            let i = step_index(path, key)?;
            if path.steps[i].applied {
                return Err(
                    ApiError::Conflict(format!("step \"{key}\" is already applied")).into(),
                );
            }
            i + 1
        }
    };
    // Steps apply in order, so the unapplied ones form a suffix.
    let todo: Vec<usize> = (0..end).filter(|&i| !path.steps[i].applied).collect();
    if todo.is_empty() {
        return Err(ApiError::Conflict("every step of this path is already applied".into()).into());
    }
    let batches: Vec<Vec<Change>> = todo
        .iter()
        .map(|&i| path.steps[i].changes.clone())
        .collect();
    let step_key = |failed: usize| path.steps[todo[failed]].key.clone();
    let seeded: Created = from_json(&row.created_json)?;
    let scenario_id = row.scenario_id;
    const STALE: &str = "the plan has changed since this suggestion was written";

    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let (target, created) = match body.to {
        ApplyTo::Plan => {
            // Each step is resolved against the plan as the ones before it
            // left it in the database, checked in memory, then written.
            match suggest::apply_steps_sql(&mut tx, scenario_id, &user.id, &batches, &seeded)
                .await?
            {
                Ok(created) => (scenario_id, created),
                Err(failed) => {
                    return Err(Failure::stale_or_invalid(
                        &body.path,
                        &step_key(failed.step),
                        failed.problems,
                        STALE,
                    ));
                }
            }
        }
        ApplyTo::Copy => {
            if body.through_step.is_some() || row.applied_path.is_some() {
                return Err(ApiError::unprocessable(
                    "a path goes to a copy whole, before any of its steps is applied",
                )
                .into());
            }
            // A copy shares the user's library rows as they are; a path that
            // adds to the library has nothing yet to point the copy at.
            if batches.iter().flatten().any(|c| {
                matches!(
                    c.target,
                    suggest::ChangeTarget::NewReturnProfile(_)
                        | suggest::ChangeTarget::NewTaxConfig(_)
                )
            }) {
                return Err(ApiError::unprocessable(
                    "a path that creates return profiles or tax configs can only be applied to the plan",
                )
                .into());
            }
            // Read the live plan inside the write lock, walk the path over
            // it in memory, and clone the result.
            let live = ScenarioGraph::load_connection(&mut tx, scenario_id, &user.id).await?;
            let stepped = match suggest::resolve_steps(&live, &batches, &seeded)? {
                Ok(stepped) => stepped,
                Err(failed) => {
                    return Err(Failure::stale_or_invalid(
                        &body.path,
                        &step_key(failed.step),
                        failed.problems,
                        STALE,
                    ));
                }
            };
            if let Err(err) = crate::compile::compile(&stepped.graph) {
                let last = batches.last().expect("at least one step");
                let problem = suggest::plan_problem(
                    err,
                    last.len().saturating_sub(1),
                    last.last()
                        .map_or(suggest::ChangeTarget::NewEvent(String::new()), |c| {
                            c.target.clone()
                        }),
                )?;
                return Err(Failure::invalid(vec![StepProblems {
                    path: body.path.clone(),
                    step: step_key(batches.len() - 1),
                    problems: vec![problem],
                }]));
            }
            crate::billing::check_plan_slot(&mut tx, &user.id, &state.config, 1).await?;
            let name = match body.name.as_deref().map(str::trim) {
                Some(name) if !name.is_empty() => name.to_string(),
                _ => copy_name(&mut tx, &user.id, &live.scenario.name, &row.title).await?,
            };
            let copy = crate::domain::clone_into_mapped(&mut tx, &stepped.graph, &name)
                .await?
                .id;
            // Its ids belong to the copy, not to this plan.
            (copy, seeded)
        }
    };

    // One timestamp for every step of this request, and for `resolved_at`
    // when they finish the path.
    let now: String = sqlx::query_scalar("SELECT datetime('now')")
        .fetch_one(&mut *tx)
        .await?;
    for &i in &todo {
        paths[p].steps[i].applied = true;
        paths[p].steps[i].applied_at = Some(now.clone());
    }
    let complete = paths[p].steps.iter().all(|s| s.applied);
    sqlx::query(
        "UPDATE suggestions
            SET paths_json = ?2, applied_path = ?3, created_json = ?4,
                status = CASE WHEN ?5 THEN 'applied' ELSE status END,
                resolved_at = CASE WHEN ?5 THEN ?6 ELSE resolved_at END
          WHERE id = ?1",
    )
    .bind(id)
    .bind(to_json(&paths)?)
    .bind(&body.path)
    .bind(to_json(&created)?)
    .bind(complete)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    if target == scenario_id {
        super::touch_scenario(&state.db, scenario_id).await?;
    }
    Ok(Json(AppliedSuggestion {
        scenario_id: target,
        suggestion: fetch(&state.db, id).await?,
    }))
}

/// "<scenario> — <title>", shortened, and numbered if the user already has a
/// scenario by that name.
async fn copy_name(
    conn: &mut sqlx::SqliteConnection,
    user_id: &str,
    scenario: &str,
    title: &str,
) -> ApiResult<String> {
    const MAX_NAME: usize = 80;
    let mut base = format!("{scenario} — {title}");
    if base.chars().count() > MAX_NAME {
        base = base.chars().take(MAX_NAME - 1).collect::<String>();
        base = format!("{}…", base.trim_end());
    }
    let taken: HashSet<String> =
        sqlx::query_scalar("SELECT name FROM scenarios WHERE user_id = ?1")
            .bind(user_id)
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .collect();
    if !taken.contains(&base) {
        return Ok(base);
    }
    Ok((2..)
        .map(|n| format!("{base} ({n})"))
        .find(|name| !taken.contains(name))
        .expect("an unused name exists"))
}

/// `POST /suggestions/{id}/dismiss`: set an open suggestion aside, as
/// dismissed or confirmed ("it's correct"). Either silences it in later
/// reviews of this scenario.
async fn dismiss(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<DismissSuggestion>,
) -> ApiResult<Json<Suggestion>> {
    let row = owned(&state.db, id, &user.id).await?;
    if row.status != "open" {
        return Err(ApiError::Conflict(format!(
            "this suggestion is already {}",
            row.status
        )));
    }
    let status = match body.resolution {
        Resolution::Dismissed => SuggestionStatus::Dismissed,
        Resolution::Confirmed => SuggestionStatus::Confirmed,
    };
    sqlx::query(
        "UPDATE suggestions SET status = ?2, resolved_at = datetime('now')
          WHERE id = ?1 AND status = 'open'",
    )
    .bind(id)
    .bind(tag(&status))
    .execute(&state.db)
    .await?;
    Ok(Json(fetch(&state.db, id).await?))
}

/// `POST /suggestions/{id}/reopen`: undo a dismissal or an "it's correct",
/// putting the note back on the board as open. An applied note cannot be
/// reopened: its changes are already in the plan.
async fn reopen(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Suggestion>> {
    let row = owned(&state.db, id, &user.id).await?;
    if row.status != "dismissed" && row.status != "confirmed" {
        return Err(ApiError::Conflict(format!(
            "this suggestion is {}, not set aside",
            row.status
        )));
    }
    sqlx::query(
        "UPDATE suggestions SET status = 'open', resolved_at = NULL
          WHERE id = ?1 AND status IN ('dismissed', 'confirmed')",
    )
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(Json(fetch(&state.db, id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Migration 0008 turns each stored batch into path "a" with one step "a",
    /// readable by the routes' own row reader.
    #[tokio::test]
    async fn old_rows_become_one_path_of_one_step() {
        use sqlx::Connection;
        let mut conn = sqlx::SqliteConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        let migrations = &crate::db::MIGRATOR.migrations;
        for m in migrations.iter().filter(|m| m.version < 8) {
            sqlx::raw_sql(&m.sql).execute(&mut conn).await.unwrap();
        }
        // Rows without their scenario or run, as the test needs no more.
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut conn)
            .await
            .unwrap();
        let long_title = "x".repeat(100);
        for (title, changes, estimate, check, status) in [
            (
                long_title.as_str(),
                r#"[{"op":"remove","target":{"event":5},"path":"/effects/0"}]"#,
                None,
                // An older shape of the stats: dropped on read.
                Some(r#"{"iterations":40,"paired":true}"#),
                "open",
            ),
            (
                "Applied",
                r#"[{"op":"remove","target":{"event":6},"path":"/effects/0"}]"#,
                Some(r#"{"success_rate":0.9,"funding_success_rate":null}"#),
                None,
                "applied",
            ),
            ("A read", "[]", None, None, "open"),
        ] {
            sqlx::query(
                "INSERT INTO suggestions (scenario_id, run_id, source, rule, kind, section, title,
                     reasoning, evidence_json, changes_json, diff_json, estimate_json, check_json,
                     fingerprint, status, resolved_at)
                 VALUES (1, 1, 'rules', 'r', 'fix', 'plan', ?1, 'why', '[]', ?2,
                         '[{\"label\":\"L\",\"from\":\"a\",\"to\":null}]', ?3, ?4, 'f', ?5,
                         CASE WHEN ?5 = 'applied' THEN '2026-09-28 10:00:00' END)",
            )
            .bind(title)
            .bind(changes)
            .bind(estimate)
            .bind(check)
            .bind(status)
            .execute(&mut conn)
            .await
            .unwrap();
        }
        let eight = migrations.iter().find(|m| m.version == 8).unwrap();
        sqlx::raw_sql(&eight.sql).execute(&mut conn).await.unwrap();
        // Later migrations too, so the rows read with today's columns.
        for m in migrations.iter().filter(|m| m.version > 8) {
            sqlx::raw_sql(&m.sql).execute(&mut conn).await.unwrap();
        }

        let rows: Vec<SuggestionRow> = sqlx::query_as(&format!(
            "SELECT {COLUMNS} FROM suggestions s ORDER BY s.id"
        ))
        .fetch_all(&mut conn)
        .await
        .unwrap();
        let [open, applied, read] = rows
            .into_iter()
            .map(|r| r.into_suggestion().unwrap())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();

        let [path] = open.paths.as_slice() else {
            panic!("one path: {:?}", open.paths);
        };
        assert_eq!((path.key.as_str(), path.recommended), ("a", true));
        assert_eq!(path.label, "Suggested change");
        assert!(path.check.is_none(), "an old check shape is dropped");
        let [step] = path.steps.as_slice() else {
            panic!("one step");
        };
        assert_eq!(step.key, "a");
        assert_eq!(step.title.chars().count(), 80);
        assert!(step.title.ends_with('…'));
        assert_eq!(step.changes.len(), 1);
        assert_eq!(step.diff[0].label, "L");
        assert!(!step.applied);
        assert_eq!(open.applied_path, None);

        assert_eq!(applied.applied_path.as_deref(), Some("a"));
        let step = &applied.paths[0].steps[0];
        assert!(step.applied);
        assert_eq!(step.applied_at.as_deref(), Some("2026-09-28 10:00:00"));
        assert_eq!(
            applied.paths[0].estimate.as_ref().unwrap().success_rate,
            Some(0.9)
        );

        assert!(read.paths.is_empty());
    }

    #[test]
    fn figures_blank_out_of_titles() {
        assert_eq!(
            blank_figures("USAA empties in 97 of 99 failures, from $1,450,000 in 2041"),
            "USAA empties in # of # failures, from $# in #"
        );
        assert_eq!(blank_figures("401(k) limit unused"), "#(k) limit unused");
    }

    #[test]
    fn fingerprints_ignore_figures_but_not_subjects() {
        let a = fingerprint(Some("r"), Kind::Read, &[], "USAA empties in 97 of 99");
        let b = fingerprint(Some("r"), Kind::Read, &[], "USAA empties in 12 of 14");
        let c = fingerprint(Some("r"), Kind::Read, &[], "Vanguard empties in 12 of 14");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(
            a,
            fingerprint(None, Kind::Read, &[], "USAA empties in 97 of 99")
        );
    }
}
