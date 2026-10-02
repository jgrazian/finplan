//! The model-written half of a review.
//!
//! `POST /scenarios/{id}/review` stores the rule notes and answers at once;
//! when a review model is configured it also hands the same run to
//! [`AiReviews::start`], which runs [`suggest::ai::generate`] in the background
//! and adds the model's notes to the review when it finishes. `Review.ai`
//! reports the pass as running, done or failed.
//!
//! One pass per scenario: a newer review supersedes a running one. The review
//! row names the current pass (`suggestion_reviews.ai_job`); a superseded pass
//! is aborted, and one that finishes anyway finds its job no longer current
//! and writes nothing. A pass that completes adds to the open notes earlier
//! passes wrote (the review carried them over), skipping any it would
//! repeat. A server restart leaves no pass
//! running: [`recover`] marks them failed.
//!
//! The model's tools run through the same code the routes use: previews
//! through `preview::run_preview` (so each is admitted as compute like any
//! other), changes through `suggest::resolve`.
//!
//! Observability: a pass runs inside the job span of the `POST /review` that
//! started it ([`JobContext`] captured in the handler), so every log line it
//! and the model loop write carries that request's `request_id`, which is also
//! stored on the review row (`ai_request_id`). It is a `review_ai` job to the
//! shared job metrics (queue wait, running, attempts, processing time), and
//! [`PassMetrics`] feeds the `finplan_review_ai_*` families: pass outcomes and
//! wall time, model latency, tokens, cost, tool calls, notes and retries.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{Path, Query, State};
use finplan_core::model::{TaxBracket, TaxConfig};
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::task::AbortHandle;
use tracing::{Instrument, instrument::WithSubscriber};

use super::ai_activity::{Activity, ActivityObserver, AiStep};
use super::preview;
use super::suggestion_chat::Thread;
use super::suggestions::{
    DraftMeta, NewSuggestion, SuggestionCheck, SuggestionEstimate, SuggestionPath,
    SuggestionSource, SuggestionStep, all_changes, fingerprint, insert,
};
use crate::auth::session::CurrentUser;
use crate::compile::rows::ScenarioGraph;
use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::observability::{
    AiMotive, AiPassOutcome, AiRetryReason, AiSuggestionOutcome, AiTokenType, AiTool,
    AiToolOutcome, Component, ErrorClass, JobContext, JobKind, Outcome, QueueExit, Telemetry,
};
use crate::runner::telemetry::{Attempt, Submitted};
use crate::state::AppState;
use crate::suggest::ai::tools::PathRank;
use crate::suggest::ai::{
    AiClient, AiDraft, AiError, AiOutcome, BoxFuture, LedgerSum, Observer, ReviewContext, Stop,
    ToolHost, TurnReport, render_breakdown, render_path, stop_tag,
};
use crate::suggest::{self, Change, ChangeProblem, Created, DiffLine};

/// Model-written passes running at once, across every scenario. Each holds a
/// conversation open for minutes and previews as it goes.
const MAX_CONCURRENT_PASSES: usize = 2;

/// The review model and the passes it is running.
#[derive(Clone)]
pub struct AiReviews {
    client: Arc<AiClient>,
    /// scenario id -> its running pass
    running: Arc<Mutex<HashMap<i64, Running>>>,
    permits: Arc<Semaphore>,
    /// The same model with the drafting agent's budget, tools and routing
    /// (`api::draft_agent`).
    draft_client: Arc<AiClient>,
    /// draft (scenario id) -> its running drafting segment
    draft_running: Arc<Mutex<HashMap<i64, Running>>>,
    /// What each running chat turn has done so far (`api::suggestion_chat`).
    chat_activity: Activity<Thread>,
    /// What each scenario's running review pass has done so far.
    review_activity: Activity<i64>,
}

struct Running {
    job: String,
    abort: AbortHandle,
}

/// One pass over one run, as the review handler hands it over.
pub(super) struct Pass {
    /// `suggestion_reviews.ai_job`, already stored as the review's current job.
    pub job: String,
    /// The starting request's context: its `request_id` tags every log line.
    pub job_context: JobContext,
    /// When the review committed the pass, for queue wait and end-to-end time.
    pub submitted: Submitted,
    pub scenario_id: i64,
    pub run_id: i64,
    pub user: CurrentUser,
    /// The run's input snapshot: what the model's changes resolve against.
    pub graph: ScenarioGraph,
    pub context: ReviewContext,
    /// Iterations for the model's previews: the review's own check budget.
    pub check_iterations: usize,
}

/// A fresh job id for a new pass.
pub(super) fn new_job() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl AiReviews {
    pub fn new(client: Arc<AiClient>, draft: &crate::suggest::ai::DraftConfig) -> Self {
        Self {
            draft_client: Arc::new(client.for_drafts(draft)),
            client,
            running: Arc::new(Mutex::new(HashMap::new())),
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT_PASSES)),
            draft_running: Arc::new(Mutex::new(HashMap::new())),
            chat_activity: Default::default(),
            review_activity: Default::default(),
        }
    }

    pub(super) fn chat_activity(&self) -> &Activity<Thread> {
        &self.chat_activity
    }

    /// Steps of `scenario_id`'s running review pass; empty when none runs.
    pub(super) fn review_steps(&self, scenario_id: i64) -> Vec<AiStep> {
        self.review_activity.steps(scenario_id)
    }

    /// The drafting agent's client.
    pub(super) fn draft_client(&self) -> &Arc<AiClient> {
        &self.draft_client
    }

    /// Note `job`'s task as the draft's running segment, aborting the one it
    /// replaces.
    pub(super) fn track_draft(&self, scenario_id: i64, job: &str, abort: AbortHandle) {
        let previous = self
            .draft_running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                scenario_id,
                Running {
                    job: job.to_owned(),
                    abort,
                },
            );
        if let Some(previous) = previous {
            previous.abort.abort();
        }
    }

    pub(super) fn forget_draft(&self, scenario_id: i64, job: &str) {
        let mut running = self.draft_running.lock().unwrap_or_else(|e| e.into_inner());
        if running.get(&scenario_id).is_some_and(|r| r.job == job) {
            running.remove(&scenario_id);
        }
    }

    /// Stop the draft's running segment, if any.
    pub(super) fn abort_draft(&self, scenario_id: i64) {
        if let Some(running) = self
            .draft_running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&scenario_id)
        {
            running.abort.abort();
        }
    }

    pub fn model(&self) -> &str {
        &self.client.settings().model
    }

    /// The model client, shared with chat turns (`suggestion_chat`).
    pub(super) fn client(&self) -> &Arc<AiClient> {
        &self.client
    }

    /// The model slots review passes and chat turns share.
    pub(super) fn permits(&self) -> Arc<Semaphore> {
        self.permits.clone()
    }

    /// Run `pass` in the background, aborting the scenario's previous pass.
    pub(super) fn start(&self, state: &AppState, pass: Pass) {
        let scenario_id = pass.scenario_id;
        let job = pass.job.clone();
        // The job span (request_id, user, scenario, run) with the pass's own
        // id inside it: the pass, the model loop, its retries and its tool
        // calls all log under both.
        let span = pass.job_context.span();
        let span = span.in_scope(|| tracing::info_span!("review_ai", ai_job = %job));
        let task = tokio::spawn(
            run(state.clone(), self.clone(), pass)
                .instrument(span.clone())
                .with_current_subscriber(),
        );
        let previous = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                scenario_id,
                Running {
                    job: job.clone(),
                    abort: task.abort_handle(),
                },
            );
        if let Some(previous) = previous {
            previous.abort.abort();
            tracing::info!(event = "review_ai.superseded", scenario_id);
        }

        // An aborted pass was superseded and has nothing to report; one that
        // panicked would otherwise read as running until the next restart.
        let reviews = self.clone();
        let db = state.db.clone();
        tokio::spawn(
            async move {
                if let Err(error) = task.await
                    && error.is_panic()
                {
                    tracing::error!(event = "review_ai.panicked", scenario_id);
                    fail(
                        &db,
                        scenario_id,
                        &job,
                        "the review model stopped unexpectedly",
                    )
                    .await;
                }
                reviews.forget(scenario_id, &job);
            }
            .instrument(span)
            .with_current_subscriber(),
        );
    }

    fn forget(&self, scenario_id: i64, job: &str) {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if running.get(&scenario_id).is_some_and(|r| r.job == job) {
            running.remove(&scenario_id);
        }
    }
}

/// Mark passes a previous process left running as failed: nothing is going
/// to finish them.
pub async fn recover(db: &Db) -> Result<u64, sqlx::Error> {
    let recovered = sqlx::query(
        "UPDATE suggestion_reviews
            SET ai_status = 'failed', ai_error = 'interrupted by a server restart',
                ai_finished_at = datetime('now')
          WHERE ai_status = 'running'",
    )
    .execute(db)
    .await?
    .rows_affected();
    if recovered > 0 {
        tracing::info!(event = "review_ai.recovered", passes = recovered);
    }
    Ok(recovered)
}

async fn run(state: AppState, reviews: AiReviews, pass: Pass) {
    let telemetry = state.telemetry.clone();
    let Ok(_permit) = reviews.permits.clone().acquire_owned().await else {
        return;
    };
    telemetry.queue_wait(
        JobKind::ReviewAi,
        QueueExit::Started,
        pass.submitted.elapsed(),
    );
    let _running = telemetry.job_started(JobKind::ReviewAi);
    let mut attempt = Attempt::new(&telemetry, &pass.job_context, pass.submitted.clone());
    // Records the pass's outcome and wall time however it ends — including
    // being aborted by a newer review, which reads as `superseded`.
    let mut ended = PassEnd::new(&telemetry, reviews.model());
    let _activity = reviews.review_activity.begin(pass.scenario_id);
    let metrics = PassMetrics::new(&telemetry, reviews.model());
    let observer = ActivityObserver::new(&metrics, &reviews.review_activity, pass.scenario_id);

    let loaded = Tools::load(
        &state,
        &pass.user,
        pass.scenario_id,
        pass.run_id,
        pass.check_iterations,
        pass.graph.clone(),
    )
    .await;
    let tools = match loaded {
        Ok(tools) => tools,
        Err(_) => {
            telemetry.count_error(Component::ReviewAi, ErrorClass::Database);
            tracing::error!(event = "review_ai.start_failed", error_class = "database");
            fail(
                &state.db,
                pass.scenario_id,
                &pass.job,
                "the review could not start",
            )
            .await;
            attempt.failure(ErrorClass::Database);
            attempt.finish(Outcome::Failed);
            ended.set(AiPassOutcome::Failed);
            return;
        }
    };
    match suggest::ai::generate_observed(&reviews.client, &pass.context, &tools, &observer).await {
        Ok(outcome) => {
            let stop = stop_tag(&outcome.stop);
            let accepted = outcome.drafts.len();
            match store(&state.db, &pass, &outcome).await {
                Ok(Some(stored)) => {
                    let discarded = accepted - stored;
                    telemetry.review_ai_suggestions(AiSuggestionOutcome::Stored, stored as u64);
                    telemetry
                        .review_ai_suggestions(AiSuggestionOutcome::Discarded, discarded as u64);
                    tracing::info!(
                        event = "review_ai.stored",
                        scenario_id = pass.scenario_id,
                        run_id = pass.run_id,
                        stop,
                        accepted,
                        stored,
                        discarded,
                        turns = outcome.usage.turns,
                        previews = outcome.usage.previews,
                        input_tokens = outcome.usage.input_tokens,
                        output_tokens = outcome.usage.output_tokens,
                        cache_read_input_tokens = outcome.usage.cache_read_input_tokens,
                        cost_usd = outcome.usage.cost_usd,
                        cost_source = outcome.usage.cost_source(),
                        wall_ms = ended.elapsed_ms(),
                    );
                    attempt.finish(Outcome::Succeeded);
                    ended.set(pass_outcome(&outcome.stop));
                }
                Ok(None) => {
                    telemetry
                        .review_ai_suggestions(AiSuggestionOutcome::Discarded, accepted as u64);
                    tracing::info!(
                        event = "review_ai.discarded",
                        scenario_id = pass.scenario_id,
                        reason = "superseded",
                        accepted,
                        cost_usd = outcome.usage.cost_usd,
                    );
                    attempt.finish(Outcome::Canceled);
                    ended.set(AiPassOutcome::Superseded);
                }
                Err(_) => {
                    telemetry.count_error(Component::ReviewAi, ErrorClass::Database);
                    tracing::error!(
                        event = "review_ai.store_failed",
                        scenario_id = pass.scenario_id,
                        error_class = "database",
                    );
                    fail(
                        &state.db,
                        pass.scenario_id,
                        &pass.job,
                        "the review notes could not be saved",
                    )
                    .await;
                    attempt.failure(ErrorClass::Database);
                    attempt.finish(Outcome::Failed);
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
            // `generate` has already scrubbed the key out of the text.
            tracing::warn!(
                event = "review_ai.failed",
                scenario_id = pass.scenario_id,
                error_class = class.as_str(),
                error = %error
            );
            let message = match error {
                AiError::Api(_) => "the review model could not be reached",
                AiError::Malformed(_) => "the review model's reply could not be read",
            };
            fail(&state.db, pass.scenario_id, &pass.job, message).await;
            attempt.failure(class);
            attempt.finish(Outcome::Failed);
            ended.set(AiPassOutcome::Failed);
        }
    }
}

pub(super) fn pass_outcome(stop: &Stop) -> AiPassOutcome {
    match stop {
        Stop::Finished => AiPassOutcome::Finished,
        Stop::TurnLimit => AiPassOutcome::TurnLimit,
        Stop::SuggestionLimit => AiPassOutcome::SuggestionLimit,
        Stop::MaxTokens => AiPassOutcome::MaxTokens,
        Stop::Refused { .. } => AiPassOutcome::Refused,
        Stop::Interrupted { .. } => AiPassOutcome::Interrupted,
        Stop::Unexpected { .. } => AiPassOutcome::Unexpected,
    }
}

/// Counts a pass once, with its wall time from taking a pass slot, when it
/// is dropped: a pass aborted by a newer review never sets an outcome and
/// counts as superseded; one that panicked counts as failed.
pub(super) struct PassEnd {
    telemetry: Telemetry,
    model: String,
    started: Instant,
    outcome: Option<AiPassOutcome>,
}

impl PassEnd {
    pub(super) fn new(telemetry: &Telemetry, model: &str) -> Self {
        Self {
            telemetry: telemetry.clone(),
            model: model.to_owned(),
            started: Instant::now(),
            outcome: None,
        }
    }
    pub(super) fn set(&mut self, outcome: AiPassOutcome) {
        self.outcome = Some(outcome);
    }
    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

impl Drop for PassEnd {
    fn drop(&mut self) {
        // No outcome: aborted by a newer review, or unwinding from a panic.
        let outcome = self.outcome.unwrap_or(if std::thread::panicking() {
            AiPassOutcome::Failed
        } else {
            AiPassOutcome::Superseded
        });
        self.telemetry
            .review_ai_pass(&self.model, outcome, self.started.elapsed().as_secs_f64());
    }
}

/// The model loop's reports, as `finplan_review_ai_*` metrics. Labels are
/// the configured model and bounded enums only. Chat turns report through it
/// too, so those families total every review-model request.
pub(super) struct PassMetrics {
    telemetry: Telemetry,
    model: String,
}

impl PassMetrics {
    pub(super) fn new(telemetry: &Telemetry, model: &str) -> Self {
        Self {
            telemetry: telemetry.clone(),
            model: model.to_owned(),
        }
    }
}

impl Observer for PassMetrics {
    fn turn(&self, turn: &TurnReport) {
        let t = &self.telemetry;
        t.review_ai_turn(&self.model, turn.seconds);
        t.review_ai_tokens(&self.model, AiTokenType::Input, turn.input_tokens);
        t.review_ai_tokens(&self.model, AiTokenType::Output, turn.output_tokens);
        t.review_ai_tokens(
            &self.model,
            AiTokenType::CacheRead,
            turn.cache_read_input_tokens,
        );
        t.review_ai_tokens(
            &self.model,
            AiTokenType::CacheWrite,
            turn.cache_creation_input_tokens,
        );
        if let Some((usd, source)) = turn.cost {
            t.review_ai_cost(&self.model, source, usd);
        }
    }
    fn tool(&self, tool: AiTool, outcome: AiToolOutcome, seconds: f64) {
        self.telemetry.review_ai_tool(tool, outcome, seconds);
    }
    fn retry(&self, reason: AiRetryReason) {
        self.telemetry.review_ai_retry(&self.model, reason);
    }
    fn submission(&self, accepted: bool) {
        self.telemetry.review_ai_suggestions(
            if accepted {
                AiSuggestionOutcome::Accepted
            } else {
                AiSuggestionOutcome::Rejected
            },
            1,
        );
    }
    fn motive(&self, motive: AiMotive, accepted: bool) {
        self.telemetry.review_ai_motive(
            motive,
            if accepted {
                AiSuggestionOutcome::Accepted
            } else {
                AiSuggestionOutcome::Rejected
            },
        );
    }
}

/// Record a failed pass, if it is still the review's current one.
async fn fail(db: &Db, scenario_id: i64, job: &str, message: &str) {
    let result = sqlx::query(
        "UPDATE suggestion_reviews
            SET ai_status = 'failed', ai_error = ?3, ai_finished_at = datetime('now')
          WHERE scenario_id = ?1 AND ai_job = ?2 AND ai_status = 'running'",
    )
    .bind(scenario_id)
    .bind(job)
    .bind(message)
    .execute(db)
    .await;
    if result.is_err() {
        tracing::error!(event = "review_ai.status_failed", scenario_id);
    }
}

/// What the user reads about a pass that stopped short; the notes it
/// accepted before stopping are kept either way.
fn stop_note(stop: &Stop) -> Option<&'static str> {
    match stop {
        Stop::Finished | Stop::TurnLimit | Stop::SuggestionLimit => None,
        Stop::MaxTokens => Some("the review model ran out of room before finishing"),
        Stop::Refused { .. } => Some("the review model declined to review this plan"),
        Stop::Interrupted { .. } => Some("the review model stopped partway through"),
        Stop::Unexpected { .. } => Some("the review model stopped unexpectedly"),
    }
}

/// Store the pass's notes and mark it done — unless a newer review has
/// replaced it, in which case nothing is written and `None` comes back.
async fn store(db: &Db, pass: &Pass, outcome: &AiOutcome) -> ApiResult<Option<usize>> {
    let mut tx = db.begin().await?;
    // First, so the transaction holds the write lock before it reads.
    let current = sqlx::query(
        "UPDATE suggestion_reviews
            SET ai_status = 'done', ai_stop = ?3, ai_error = ?4, ai_finished_at = datetime('now'),
                ai_turns = ?5, ai_input_tokens = ?6, ai_output_tokens = ?7, ai_cost_usd = ?8
          WHERE scenario_id = ?1 AND ai_job = ?2 AND ai_status = 'running'",
    )
    .bind(pass.scenario_id)
    .bind(&pass.job)
    .bind(stop_tag(&outcome.stop))
    .bind(stop_note(&outcome.stop))
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

    // Earlier passes' notes stay (the review carried them over); dismissed or
    // confirmed notes stay silent, an open one is not duplicated, and one
    // applied from this run is not raised again.
    let standing: HashSet<String> = sqlx::query_scalar(
        "SELECT fingerprint FROM suggestions
          WHERE scenario_id = ?1
            AND (status IN ('open', 'dismissed', 'confirmed')
                 OR (status = 'applied' AND run_id = ?2))",
    )
    .bind(pass.scenario_id)
    .bind(pass.run_id)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();

    let mut seen = HashSet::new();
    let mut stored = 0;
    for draft in &outcome.drafts {
        let new = suggestion(pass, draft);
        if standing.contains(&new.fingerprint) || !seen.insert(new.fingerprint.clone()) {
            continue;
        }
        insert(&mut tx, pass.scenario_id, &new).await?;
        stored += 1;
    }
    tx.commit().await?;
    Ok(Some(stored))
}

fn suggestion(pass: &Pass, draft: &AiDraft) -> NewSuggestion {
    draft_suggestion(pass.run_id, draft, Some(pass.job.clone()), None)
}

/// A draft the model's checks accepted, as a suggestion to store: its own
/// previews become its paths' checks. `review_job` marks a review pass's
/// notes; `parent_id` a chat thread's.
pub(super) fn draft_suggestion(
    run_id: i64,
    draft: &AiDraft,
    review_job: Option<String>,
    parent_id: Option<i64>,
) -> NewSuggestion {
    let paths: Vec<SuggestionPath> = draft
        .paths
        .iter()
        .map(|path| SuggestionPath {
            key: path.key.clone(),
            label: path.label.clone(),
            reasoning: path.reasoning.clone(),
            recommended: path.recommended,
            steps: path
                .steps
                .iter()
                .map(|step| SuggestionStep {
                    key: step.key.clone(),
                    title: step.title.clone(),
                    reasoning: step.reasoning.clone(),
                    changes: step.changes.clone(),
                    diff: step.diff.clone(),
                    applied: false,
                    applied_at: None,
                })
                .collect(),
            estimate: path.estimate.as_ref().map(|e| SuggestionEstimate {
                success_rate: e.success_rate,
                funding_success_rate: e.funding_success_rate,
            }),
            // The preview the model ran on exactly these steps, so the path
            // arrives checked without simulating it again.
            check: path.preview.as_ref().and_then(check_from),
        })
        .collect();
    NewSuggestion {
        run_id: Some(run_id),
        source: SuggestionSource::Ai,
        rule: None,
        kind: draft.kind,
        section: draft.section,
        title: draft.title.clone(),
        summary: Some(draft.summary.clone()),
        reasoning: draft.reasoning.clone(),
        evidence: draft.evidence.clone(),
        fingerprint: fingerprint(None, draft.kind, &all_changes(&paths), &draft.title),
        paths,
        review_job,
        parent_id,
        draft: DraftMeta::default(),
    }
}

/// A `Preview` as JSON, read back as a check; one with problems or without
/// both sides simulated is not a check.
pub(super) fn check_from(preview: &Value) -> Option<SuggestionCheck> {
    let clean = preview
        .get("problems")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty);
    clean
        .then(|| serde_json::from_value(preview.clone()).ok())
        .flatten()
}

/// The model's tools, bound to one run: a review pass's or a chat turn's.
pub(super) struct Tools<'a> {
    state: &'a AppState,
    user: &'a CurrentUser,
    scenario_id: i64,
    run_id: i64,
    /// Iterations for the model's previews.
    check_iterations: usize,
    /// The run's snapshot, with every profile of the user's loaded.
    graph: ScenarioGraph,
    /// Every profile's name, for diffs.
    profiles: Vec<(i64, String)>,
}

impl<'a> Tools<'a> {
    /// Every profile of the user's, loaded into the plan the model's steps
    /// resolve against: the snapshot kept only its run's, and an edit that
    /// names another is checked against the plan's profiles. Their names also
    /// read in the diffs.
    pub(super) async fn load(
        state: &'a AppState,
        user: &'a CurrentUser,
        scenario_id: i64,
        run_id: i64,
        check_iterations: usize,
        mut graph: ScenarioGraph,
    ) -> ApiResult<Self> {
        let profiles: Vec<(i64, String)> =
            sqlx::query_as("SELECT id, name FROM return_profiles WHERE user_id = ?1")
                .bind(&user.id)
                .fetch_all(&state.db)
                .await?;
        preview::load_profiles(
            &state.db,
            &user.id,
            &mut graph,
            profiles.iter().map(|(id, _)| *id).collect(),
        )
        .await?;
        let tax: Vec<i64> = sqlx::query_scalar("SELECT id FROM tax_configs WHERE user_id = ?1")
            .bind(&user.id)
            .fetch_all(&state.db)
            .await?;
        let inflation: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM inflation_profiles WHERE user_id = ?1")
                .bind(&user.id)
                .fetch_all(&state.db)
                .await?;
        preview::load_assumptions(&state.db, &user.id, &mut graph, tax, inflation).await?;
        Ok(Self {
            state,
            user,
            scenario_id,
            run_id,
            check_iterations,
            graph,
            profiles,
        })
    }
}

impl ToolHost for Tools<'_> {
    fn preview<'b>(&'b self, changes: Vec<Change>) -> BoxFuture<'b, Result<Value, String>> {
        Box::pin(async move {
            let preview = preview::run_preview(
                self.state,
                self.user,
                self.scenario_id,
                Some(self.run_id),
                &changes,
                Some(self.check_iterations),
                None,
            )
            .await
            .map_err(|error| match error {
                // Not the model's to read: internal detail stays in the logs.
                ApiError::Database(_) | ApiError::Internal(_) => {
                    tracing::warn!(event = "review_ai.preview_failed", error = %error);
                    "the preview could not run; try different changes or stop".to_string()
                }
                other => other.to_string(),
            })?;
            serde_json::to_value(&preview).map_err(|e| e.to_string())
        })
    }

    fn preflight(&self) -> Result<Value, String> {
        serde_json::to_value(super::onboarding::review(&self.graph)).map_err(|e| e.to_string())
    }

    fn goal_seek<'b>(
        &'b self,
        request: crate::suggest::ai::tools::goal_seek::GoalSeekRequest,
    ) -> BoxFuture<'b, Result<Value, String>> {
        Box::pin(async move {
            super::analysis::ai_goal_seek(self.state, self.user, &self.graph, request)
                .await
                .map_err(|error| match error {
                    // Not the model's to read: internal detail stays in the logs.
                    ApiError::Database(_) | ApiError::Internal(_) => {
                        tracing::warn!(event = "review_ai.goal_seek_failed", error = %error);
                        "the goal seek could not run; use the results you have or stop".to_string()
                    }
                    other => other.to_string(),
                })
        })
    }

    fn inspect_path<'b>(
        &'b self,
        rank: PathRank,
        years: Option<(i64, i64)>,
    ) -> BoxFuture<'b, Result<String, String>> {
        Box::pin(async move {
            let percentile = match rank.percentile() {
                Some(p) => Some(p),
                // The worst path is the lowest percentile the run stored.
                None => sqlx::query_scalar::<_, Option<f64>>(
                    "SELECT MIN(percentile) FROM run_net_worth_points
                      WHERE run_id = ?1 AND percentile IS NOT NULL",
                )
                .bind(self.run_id)
                .fetch_one(&self.state.db)
                .await
                .map_err(|e| {
                    tracing::warn!(event = "review_ai.inspect_failed", error = %e);
                    "the run's paths could not be read".to_string()
                })?,
            };
            let Some(percentile) = percentile else {
                return Err("this run stored no percentile paths".into());
            };
            let results = super::runs::results(
                State(self.state.clone()),
                self.user.clone(),
                Path(self.run_id),
                Query(super::runs::ResultsQuery {
                    series: Some(percentile.to_string()),
                }),
            )
            .await
            .map_err(|error| match error {
                ApiError::Database(_) | ApiError::Internal(_) => {
                    tracing::warn!(event = "review_ai.inspect_failed", error = %error);
                    "the run's paths could not be read".to_string()
                }
                other => other.to_string(),
            })?
            .0;
            if !results.path_details {
                return Err(
                    "a newer run replaced this one's paths; only its summary remains".into(),
                );
            }
            Ok(render_path(&self.graph, &results, rank.as_str(), years))
        })
    }

    fn cash_flow_breakdown<'b>(
        &'b self,
        rank: PathRank,
        years: Option<(i64, i64)>,
    ) -> BoxFuture<'b, Result<String, String>> {
        Box::pin(async move {
            let unreadable = |e: sqlx::Error| {
                tracing::warn!(event = "review_ai.breakdown_failed", error = %e);
                "the run's ledger could not be read".to_string()
            };
            // The stored path nearest the rank; the worst is the lowest, and
            // nearest to zero.
            let percentile: Option<f64> = sqlx::query_scalar(
                "SELECT percentile FROM run_ledger
                  WHERE run_id = ?1 AND percentile IS NOT NULL
                  GROUP BY percentile ORDER BY ABS(percentile - ?2) LIMIT 1",
            )
            .bind(self.run_id)
            .bind(rank.percentile().unwrap_or(0.0))
            .fetch_optional(&self.state.db)
            .await
            .map_err(unreadable)?;
            let Some(percentile) = percentile else {
                return Err(
                    "this run keeps no ledger: a newer run replaced its paths, or it stored none"
                        .into(),
                );
            };
            let (from, to) = years.unwrap_or((i64::MIN, i64::MAX));
            // Cash in and out of the plan, and taxes; not money moved between
            // its accounts. An RMD is written twice, as the required
            // distribution and as the cash it paid in; only the cash counts.
            let sums: Vec<LedgerSum> = sqlx::query_as(
                "SELECT event_id,
                        CASE WHEN event_id IS NULL THEN account_id END AS account_id,
                        CASE WHEN category = 'tax' THEN 'taxes'
                             WHEN kind = 'Income' THEN 'income'
                             WHEN kind = 'Expense' THEN 'expenses'
                             WHEN kind = 'Contribution' THEN 'contributions'
                             ELSE 'withdrawals' END AS bucket,
                        SUM(amount) AS amount, MIN(year) AS first_year, MAX(year) AS last_year
                   FROM run_ledger
                  WHERE run_id = ?1 AND percentile = ?2 AND amount IS NOT NULL
                    AND year BETWEEN ?3 AND ?4
                    AND (category = 'tax'
                         OR kind IN ('Income', 'Expense', 'Contribution', 'Withdrawal')
                         OR (kind = 'RMD' AND detail LIKE 'into %'))
                  GROUP BY 1, 2, 3",
            )
            .bind(self.run_id)
            .bind(percentile)
            .bind(from)
            .bind(to)
            .fetch_all(&self.state.db)
            .await
            .map_err(unreadable)?;
            Ok(render_breakdown(
                &self.graph,
                &sums,
                rank.as_str(),
                percentile,
                years,
            ))
        })
    }

    fn sensitivity<'b>(
        &'b self,
        request: crate::suggest::ai::tools::sensitivity::SensitivityRequest,
    ) -> BoxFuture<'b, Result<Value, crate::suggest::ai::tools::sensitivity::SensitivityError>>
    {
        Box::pin(super::analysis::ai_sensitivity(
            self.state,
            self.user,
            &self.graph,
            request,
        ))
    }

    fn plan_tax_config(&self) -> Option<TaxConfig> {
        let config = self.graph.tax_config.as_ref()?;
        Some(TaxConfig {
            federal_brackets: self
                .graph
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
        match suggest::resolve_steps(&self.graph, steps, &Created::new()) {
            Ok(Ok(stepped)) => Ok(stepped
                .steps
                .iter()
                .map(|step| step.diff(self.profiles.iter().cloned()))
                .collect()),
            Ok(Err(failed)) => Err((failed.step, failed.problems)),
            // A server fault, not the model's: reported without detail.
            Err(error) => {
                tracing::warn!(event = "review_ai.resolve_failed", error = %error);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_needs_both_sides_and_no_problems() {
        let stats = serde_json::json!({
            "success_rate": 0.9, "funding_success_rate": 0.9,
            "real_final": null, "funding": null
        });
        let full = serde_json::json!({
            "base_run_id": 1, "iterations": 40, "paired": true, "diff": [], "problems": [],
            "base": stats, "edited": stats
        });
        let check = check_from(&full).expect("a clean preview is a check");
        assert_eq!(check.iterations, 40);
        assert!(check.paired);

        let mut problems = full.clone();
        problems["problems"] = serde_json::json!([{"kind": "bad_path"}]);
        assert!(check_from(&problems).is_none());

        let mut unsimulated = full;
        unsimulated["edited"] = Value::Null;
        assert!(check_from(&unsimulated).is_none());
    }

    #[test]
    fn stops_that_end_early_say_so() {
        assert_eq!(stop_tag(&Stop::Finished), "finished");
        assert!(stop_note(&Stop::Finished).is_none());
        assert!(stop_note(&Stop::SuggestionLimit).is_none());
        let interrupted = Stop::Interrupted {
            error: "secret detail".into(),
        };
        assert_eq!(stop_tag(&interrupted), "interrupted");
        // The public note never carries the underlying error text.
        assert!(!stop_note(&interrupted).unwrap().contains("secret"));
    }
}
