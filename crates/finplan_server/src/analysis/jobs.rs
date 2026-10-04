//! The in-memory job registry, and the work each kind of analysis does.
//!
//! Same contract as a run — POST returns an id, GET polls it, results come back
//! once it says `succeeded` — with the persistence left out. See the module
//! docs for why.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::observability::{
    Component, ErrorClass, JobContext, JobKind as MetricKind, Origin, Outcome as MetricOutcome,
    Phase, QueueExit, QueueSnapshot, Telemetry,
};
use crate::runner::telemetry::{Attempt, PhaseTimer, Submitted};
use finplan_core::analysis::{ProgressRunner, SweepProgress};
use finplan_core::config::SimulationConfig;
use tokio::sync::Semaphore;
use tracing::{Instrument, instrument::WithSubscriber};

use super::cache;
use super::results::AnalysisOutcome;
use crate::db::Db;
use crate::error::{ApiError, ApiResult};
pub use finplan_plan::analysis::AnalysisSpec as JobSpec;
use finplan_plan::analysis::run;

/// How many finished jobs are kept. Enough that switching between Sweep and
/// Solve and back still finds both, small enough that a long session does not
/// accumulate grids nobody will look at again.
const KEEP: usize = 32;

/// Which question was asked. Reported back so a client polling an id it was
/// handed knows what shape of result to expect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Sweep,
    Sensitivity,
    Solve,
    WhatIf,
}

impl From<JobKind> for MetricKind {
    fn from(kind: JobKind) -> Self {
        match kind {
            JobKind::Sweep => Self::Sweep,
            JobKind::Sensitivity => Self::Sensitivity,
            JobKind::Solve => Self::Solve,
            JobKind::WhatIf => Self::WhatIf,
        }
    }
}
impl JobKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sweep => "sweep",
            Self::Sensitivity => "sensitivity",
            Self::Solve => "solve",
            Self::WhatIf => "what-if",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl JobStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
    }
}

/// Which question a spec asks.
fn kind_of(spec: &JobSpec) -> JobKind {
    match spec {
        JobSpec::Sweep { .. } => JobKind::Sweep,
        JobSpec::Sensitivity { .. } => JobKind::Sensitivity,
        JobSpec::Solve { .. } => JobKind::Solve,
        JobSpec::WhatIf { .. } => JobKind::WhatIf,
    }
}

/// What a poll of a job sees.
#[derive(Debug, Clone)]
pub struct JobView {
    pub id: i64,
    pub scenario_id: i64,
    pub kind: JobKind,
    pub status: JobStatus,
    /// Simulations finished so far, and the total the job budgeted for.
    pub completed: u64,
    pub total: u64,
    pub error: Option<String>,
    pub elapsed_ms: Option<u64>,
}

/// One job's slot in the registry.
struct Job {
    scenario_id: i64,
    user_id: String,
    kind: JobKind,
    status: JobStatus,
    completed: Arc<AtomicUsize>,
    total: usize,
    cancel: Arc<AtomicBool>,
    started: Instant,
    submitted: Submitted,
    context: JobContext,
    elapsed_ms: Option<u64>,
    error: Option<String>,
    outcome: Option<AnalysisOutcome>,
}

/// The id a caller polls, plus the flags the worker writes through.
pub struct JobHandle {
    pub id: i64,
    progress: SweepProgress,
}

#[derive(Default)]
struct Registry {
    next_id: i64,
    jobs: HashMap<i64, Job>,
    /// Ids in creation order, so eviction can drop the oldest.
    order: Vec<i64>,
}

/// Handle held by the request handlers.
#[derive(Clone)]
pub struct AnalysisJobs {
    inner: Arc<Mutex<Registry>>,
    telemetry: Telemetry,
    cache_failures: Arc<AtomicUsize>,
    /// Only a finished sweep touches it — see [`cache`].
    db: Db,
    /// Caps concurrent analyses the same way the run pool caps runs: each one
    /// is already rayon-parallel inside.
    permits: Arc<Semaphore>,
}

/// Keep the registry and cooperative cancellation flag consistent if an async
/// owner is aborted or unwinds while waiting for a worker or engine completion.
struct JobCleanup {
    jobs: AnalysisJobs,
    id: i64,
}
impl Drop for JobCleanup {
    fn drop(&mut self) {
        let mut reg = self.jobs.lock();
        if let Some(job) = reg.jobs.get_mut(&self.id)
            && !job.status.is_terminal()
        {
            job.cancel.store(true, Ordering::Relaxed);
            if job.status == JobStatus::Queued {
                job.context.event("analysis.interrupted");
            }
            job.status = JobStatus::Failed;
            job.error = Some("Analysis interrupted".into());
            job.elapsed_ms = Some(job.started.elapsed().as_millis() as u64);
        }
    }
}

/// The convenience alias the outcome enum is returned as.
pub type Outcome = AnalysisOutcome;

impl AnalysisJobs {
    #[must_use]
    pub fn new(db: Db, workers: usize) -> Self {
        Self::new_with_telemetry(db, workers, Telemetry::new(workers))
    }

    #[must_use]
    pub fn new_with_telemetry(db: Db, workers: usize, telemetry: Telemetry) -> Self {
        Self {
            telemetry,
            cache_failures: Arc::new(AtomicUsize::new(0)),
            inner: Arc::new(Mutex::new(Registry::default())),
            db,
            permits: Arc::new(Semaphore::new(workers.max(1))),
        }
    }

    /// Queue an analysis of `config` and return its id immediately.
    ///
    /// The scenario is already compiled by the caller, so a plan that cannot be
    /// lowered has failed before a job exists — the same bargain `POST /runs`
    /// makes.
    pub fn start(
        &self,
        scenario_id: i64,
        user_id: &str,
        base: SimulationConfig,
        spec: JobSpec,
        admission: crate::billing::ComputePermit,
    ) -> JobHandle {
        let total = spec.budget();
        let completed = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        // The engine owns the total it counts against and resets it when a
        // sweep or a solve starts; the job keeps its own budget, which is the
        // one the client's progress bar is drawn against.
        let progress = SweepProgress::from_atomics(
            completed.clone(),
            Arc::new(AtomicUsize::new(total)),
            cancel.clone(),
        );

        let id = {
            let mut reg = self.lock();
            reg.next_id += 1;
            let id = reg.next_id;
            reg.jobs.insert(
                id,
                Job {
                    scenario_id,
                    user_id: user_id.to_string(),
                    kind: kind_of(&spec),
                    status: JobStatus::Queued,
                    completed,
                    total,
                    cancel,
                    started: Instant::now(),
                    submitted: Submitted::now(),
                    context: JobContext::new(
                        kind_of(&spec).into(),
                        Origin::Request,
                        user_id,
                        scenario_id,
                        id,
                    ),
                    elapsed_ms: None,
                    error: None,
                    outcome: None,
                },
            );
            reg.order.push(id);
            reg.evict();
            id
        };

        let jobs = self.clone();
        let permits = self.permits.clone();
        let handle_progress = progress.clone();
        let owner = user_id.to_string();
        let (context, submitted) = {
            let reg = self.lock();
            let job = &reg.jobs[&id];
            (job.context.clone(), job.submitted.clone())
        };
        context.event("analysis.submitted");
        let span = context.span();
        tokio::spawn(
            async move {
                let _admission = admission;
                let _cleanup = JobCleanup {
                    jobs: jobs.clone(),
                    id,
                };
                let Ok(_permit) = permits.acquire_owned().await else {
                    jobs.telemetry
                        .count_error(Component::Analysis, ErrorClass::QueueClosed);
                    tracing::error!(
                        event = "analysis.queue_closed",
                        error_class = "queue_closed"
                    );
                    jobs.finish(
                        id,
                        JobStatus::Failed,
                        None,
                        Some("Analysis queue unavailable".into()),
                    );
                    return;
                };
                // A queued plan may have been removed while this job waited.
                let exists = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM scenarios WHERE id=? AND user_id=?)",
                )
                .bind(scenario_id)
                .bind(&owner)
                .fetch_one(&jobs.db)
                .await;
                match exists {
                    Ok(false) => {
                        jobs.deleted_before_start(id);
                        return;
                    }
                    Err(_) => {
                        jobs.telemetry
                            .count_error(Component::Analysis, ErrorClass::Database);
                        tracing::error!(
                            event = "analysis.prepare_failed",
                            error_class = "database"
                        );
                        jobs.finish(
                            id,
                            JobStatus::Failed,
                            None,
                            Some("Unable to prepare analysis".into()),
                        );
                        return;
                    }
                    Ok(true) => {}
                }
                if !jobs.mark_running(id) {
                    jobs.finish(id, JobStatus::Canceled, None, None);
                    return;
                }
                context.event("analysis.started");
                let _running = jobs.telemetry.job_started(context.kind);
                let mut attempt = Attempt::new(&jobs.telemetry, &context, submitted);
                let prepare = PhaseTimer::new(&jobs.telemetry, context.kind, Phase::Prepare);
                let blocking_queued = Instant::now();
                let dispatch = tracing::dispatcher::get_default(Clone::clone);
                let span = tracing::Span::current();
                let worker_telemetry = jobs.telemetry.clone();
                let kind = context.kind;
                drop(prepare);
                let outcome = tokio::task::spawn_blocking(move || {
                    tracing::dispatcher::with_default(&dispatch, || {
                        span.in_scope(|| {
                            worker_telemetry.phase(
                                kind,
                                Phase::BlockingWait,
                                blocking_queued.elapsed().as_secs_f64(),
                            );
                            let _engine = PhaseTimer::new(&worker_telemetry, kind, Phase::Engine);
                            run(
                                &base,
                                &spec,
                                &mut ProgressRunner::new(Some(&handle_progress)),
                            )
                        })
                    })
                })
                .await;
                let (status, result, error) = match outcome {
                    Ok(Ok(result)) => {
                        if let AnalysisOutcome::Sweep(sweep) = &result {
                            let _persist = PhaseTimer::new(&jobs.telemetry, kind, Phase::Persist);
                            match cache::save(&jobs.db, scenario_id, &owner, sweep).await {
                                Ok(()) => {
                                    let failures = jobs.cache_failures.swap(0, Ordering::Relaxed);
                                    if failures > 0 {
                                        tracing::info!(
                                            event = "analysis.cache_recovered",
                                            failures
                                        );
                                    }
                                }
                                Err(_) => {
                                    jobs.telemetry
                                        .count_error(Component::Analysis, ErrorClass::Persistence);
                                    let failures =
                                        jobs.cache_failures.fetch_add(1, Ordering::Relaxed) + 1;
                                    if failures == 1 || failures.is_multiple_of(60) {
                                        tracing::warn!(
                                            event = "analysis.cache_failed",
                                            error_class = "persistence",
                                            failures
                                        );
                                    }
                                }
                            }
                        }
                        (JobStatus::Succeeded, Some(result), None)
                    }
                    Ok(Err(err)) if err.is_cancelled() => (JobStatus::Canceled, None, None),
                    Ok(Err(_)) => {
                        jobs.telemetry
                            .count_error(Component::Analysis, ErrorClass::Engine);
                        attempt.failure(ErrorClass::Engine);
                        (JobStatus::Failed, None, Some("Analysis failed".into()))
                    }
                    Err(_) => {
                        jobs.telemetry
                            .count_error(Component::Analysis, ErrorClass::EnginePanic);
                        attempt.failure(ErrorClass::EnginePanic);
                        (
                            JobStatus::Failed,
                            None,
                            Some("Analysis worker failed".into()),
                        )
                    }
                };
                if let Some(actual) = jobs.finish(id, status, result, error) {
                    attempt.finish(match actual {
                        JobStatus::Succeeded => MetricOutcome::Succeeded,
                        JobStatus::Canceled => MetricOutcome::Canceled,
                        _ => MetricOutcome::Failed,
                    });
                }
            }
            .instrument(span)
            .with_current_subscriber(),
        );

        JobHandle { id, progress }
    }

    /// The job's current state, if it belongs to `user_id`.
    pub fn view(&self, id: i64, user_id: &str) -> ApiResult<JobView> {
        let reg = self.lock();
        let job = reg.owned(id, user_id)?;
        Ok(JobView {
            id,
            scenario_id: job.scenario_id,
            kind: job.kind,
            status: job.status,
            completed: job.completed.load(Ordering::Relaxed) as u64,
            total: job.total as u64,
            error: job.error.clone(),
            elapsed_ms: job.elapsed_ms,
        })
    }

    /// The finished results, or an explanation of why there are none yet.
    pub fn outcome(&self, id: i64, user_id: &str) -> ApiResult<AnalysisOutcome> {
        let reg = self.lock();
        let job = reg.owned(id, user_id)?;
        match (&job.outcome, job.status) {
            (Some(outcome), _) => Ok(outcome.clone()),
            (None, JobStatus::Failed) => Err(ApiError::unprocessable(
                job.error
                    .clone()
                    .unwrap_or_else(|| "analysis failed".into()),
            )),
            (None, JobStatus::Canceled) => Err(ApiError::bad_request("analysis was canceled")),
            (None, _) => Err(ApiError::bad_request("analysis has not finished")),
        }
    }

    /// Ask a job to stop. Returns the state it is in afterwards.
    ///
    /// Queued and running jobs both honour the flag; the engine checks it
    /// between grid points and between Monte Carlo batches.
    pub fn cancel(&self, id: i64, user_id: &str) -> ApiResult<JobView> {
        {
            let reg = self.lock();
            let job = reg.owned(id, user_id)?;
            if !job.status.is_terminal() && !job.cancel.swap(true, Ordering::Relaxed) {
                job.context.event("analysis.cancel_requested");
            }
        }
        self.view(id, user_id)
    }

    fn deleted_before_start(&self, id: i64) {
        let mut reg = self.lock();
        if let Some(job) = reg.jobs.get_mut(&id)
            && job.status == JobStatus::Queued
        {
            self.telemetry
                .queue_wait(job.kind.into(), QueueExit::Deleted, job.submitted.elapsed());
            job.context.event("analysis.deleted_before_start");
            job.status = JobStatus::Canceled;
            job.elapsed_ms = Some(job.started.elapsed().as_millis() as u64);
        }
    }

    fn mark_running(&self, id: i64) -> bool {
        let mut reg = self.lock();
        if let Some(job) = reg.jobs.get_mut(&id)
            && job.status == JobStatus::Queued
            && !job.cancel.load(Ordering::Relaxed)
        {
            self.telemetry
                .queue_wait(job.kind.into(), QueueExit::Started, job.submitted.elapsed());
            job.status = JobStatus::Running;
            job.started = Instant::now();
            return true;
        }
        false
    }

    fn finish(
        &self,
        id: i64,
        status: JobStatus,
        outcome: Option<AnalysisOutcome>,
        error: Option<String>,
    ) -> Option<JobStatus> {
        let mut reg = self.lock();
        let job = reg.jobs.get_mut(&id)?;
        if job.status.is_terminal() {
            return None;
        }
        let status = if job.cancel.load(Ordering::Relaxed) {
            JobStatus::Canceled
        } else {
            status
        };
        if job.status == JobStatus::Queued && status == JobStatus::Canceled {
            self.telemetry.queue_wait(
                job.kind.into(),
                QueueExit::Canceled,
                job.submitted.elapsed(),
            );
            self.telemetry.canceled_before_start(job.kind.into());
            job.context.event("analysis.canceled");
        }
        job.status = status;
        job.elapsed_ms = Some(job.started.elapsed().as_millis() as u64);
        job.outcome = if status == JobStatus::Succeeded {
            outcome
        } else {
            None
        };
        job.error = if status == JobStatus::Failed {
            error
        } else {
            None
        };
        Some(status)
    }

    /// Queued age uses insertion time; the existing API elapsed timer resets on
    /// worker claim and retains its established meaning.
    pub fn queue_snapshot(&self) -> Vec<QueueSnapshot> {
        let reg = self.lock();
        [
            JobKind::Sweep,
            JobKind::Sensitivity,
            JobKind::Solve,
            JobKind::WhatIf,
        ]
        .into_iter()
        .map(|kind| {
            let mut queued = 0;
            let mut oldest_age_seconds: f64 = 0.0;
            for job in reg
                .jobs
                .values()
                .filter(|job| job.kind == kind && job.status == JobStatus::Queued)
            {
                queued += 1;
                oldest_age_seconds = oldest_age_seconds.max(job.submitted.elapsed());
            }
            QueueSnapshot {
                kind: kind.into(),
                queued,
                oldest_age_seconds,
            }
        })
        .collect()
    }

    /// A poisoned registry means a handler panicked while holding the lock.
    /// The data behind it is still consistent — every critical section here is
    /// a few field writes — so the guard is taken rather than propagated as a
    /// failure the caller cannot act on.
    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl JobHandle {
    /// Stop the job this handle was returned for.
    pub fn cancel(&self) {
        self.progress.cancel();
    }
}

impl Registry {
    fn owned(&self, id: i64, user_id: &str) -> ApiResult<&Job> {
        // A job belonging to someone else is reported as missing, so ids are
        // not probeable — the same answer scenarios give.
        self.jobs
            .get(&id)
            .filter(|job| job.user_id == user_id)
            .ok_or(ApiError::NotFound("analysis"))
    }

    /// Drop the oldest jobs once the registry is over its cap, skipping any
    /// still in flight.
    fn evict(&mut self) {
        while self.order.len() > KEEP {
            let Some(position) = self
                .order
                .iter()
                .position(|id| self.jobs.get(id).is_none_or(|j| j.status.is_terminal()))
            else {
                // Everything held is still running; let the cap slip rather
                // than kill work someone is waiting on.
                break;
            };
            let id = self.order.remove(position);
            self.jobs.remove(&id);
        }
    }
}

/// Why an analysis run on the caller's own thread produced nothing.
#[derive(Debug)]
pub(crate) enum InlineFailure {
    Cancelled,
    Failed,
}

/// Run an analysis to completion on the calling thread, outside the job
/// table: no id, no polling, nothing kept. For answers small enough to wait
/// on in one request. Blocking, so call it from `spawn_blocking`.
pub(crate) fn run_inline(
    base: &SimulationConfig,
    spec: &JobSpec,
    progress: &SweepProgress,
) -> Result<AnalysisOutcome, InlineFailure> {
    run(base, spec, &mut ProgressRunner::new(Some(progress))).map_err(|err| {
        if err.is_cancelled() {
            InlineFailure::Cancelled
        } else {
            InlineFailure::Failed
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use finplan_core::analysis::{SweepConfig, SweepParameter};
    use std::time::Duration;

    async fn fixture() -> (AnalysisJobs, String, SimulationConfig) {
        let db = crate::db::connect("sqlite::memory:", 1).await.unwrap();
        let user = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES (?,?,'unused')")
            .bind(&user)
            .bind(format!("{user}@example.test"))
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO scenarios(id,user_id,name,start_date,duration_years) VALUES (1,?,'Test','2026-01-01',1)")
            .bind(&user).execute(&db).await.unwrap();
        let graph = crate::db::graph::load(&db, 1, &user).await.unwrap();
        let config = finplan_plan::compile::compile(&graph).unwrap().config;
        (AnalysisJobs::new(db, 1), user, config)
    }
    fn spec() -> JobSpec {
        JobSpec::Sensitivity {
            params: vec![],
            fraction: 0.2,
            iterations: 3,
            parallel_batches: 1,
            seed: Some(42),
        }
    }
    async fn idle(jobs: &AnalysisJobs, id: i64, user: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if jobs.view(id, user).unwrap().status.is_terminal()
                    && jobs.permits.available_permits() == 1
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    fn metric(jobs: &AnalysisJobs, line: &str) -> bool {
        jobs.telemetry.encode().unwrap().lines().any(|l| l == line)
    }

    #[tokio::test]
    async fn canceled_analysis_retains_queue_age_and_records_terminal_once() {
        let (jobs, user, config) = fixture().await;
        let permit = jobs.permits.clone().acquire_owned().await.unwrap();
        let handle = jobs.start(
            1,
            &user,
            config,
            spec(),
            crate::billing::admit_compute(&user).unwrap(),
        );
        let before = jobs.queue_snapshot();
        assert_eq!(
            before
                .iter()
                .find(|s| s.kind == MetricKind::Sensitivity)
                .unwrap()
                .queued,
            1
        );
        jobs.cancel(handle.id, &user).unwrap();
        drop(permit);
        idle(&jobs, handle.id, &user).await;
        assert_eq!(
            jobs.view(handle.id, &user).unwrap().status,
            JobStatus::Canceled
        );
        assert!(
            jobs.finish(handle.id, JobStatus::Failed, None, Some("duplicate".into()))
                .is_none()
        );
        assert!(metric(
            &jobs,
            "finplan_jobs_canceled_before_start_total{kind=\"sensitivity\"} 1"
        ));
        assert!(
            jobs.telemetry
                .encode()
                .unwrap()
                .lines()
                .filter(|l| l.starts_with("finplan_job_attempts_total{"))
                .all(|l| l.ends_with(" 0"))
        );
        assert!(
            jobs.queue_snapshot()
                .iter()
                .all(|s| s.queued == 0 && s.oldest_age_seconds == 0.0)
        );
    }

    #[tokio::test]
    async fn successful_analysis_has_one_attempt_and_separate_monotonic_clocks() {
        let (jobs, user, config) = fixture().await;
        let permit = jobs.permits.clone().acquire_owned().await.unwrap();
        let handle = jobs.start(
            1,
            &user,
            config,
            spec(),
            crate::billing::admit_compute(&user).unwrap(),
        );
        let created = jobs.lock().jobs[&handle.id].started;
        drop(permit);
        idle(&jobs, handle.id, &user).await;
        assert_eq!(
            jobs.view(handle.id, &user).unwrap().status,
            JobStatus::Succeeded
        );
        assert!(jobs.lock().jobs[&handle.id].started >= created);
        assert!(!jobs.mark_running(handle.id));
        assert!(
            jobs.finish(handle.id, JobStatus::Succeeded, None, None)
                .is_none()
        );
        assert!(metric(
            &jobs,
            "finplan_job_attempts_total{kind=\"sensitivity\",outcome=\"succeeded\"} 1"
        ));
        assert!(metric(
            &jobs,
            "finplan_jobs_running{kind=\"sensitivity\"} 0"
        ));
        assert!(
            !jobs
                .telemetry
                .encode()
                .unwrap()
                .contains("finplan_run_iterations_completed_total 3")
        );
    }

    #[tokio::test]
    async fn queued_analysis_of_deleted_scenario_never_starts() {
        let (jobs, user, config) = fixture().await;
        let permit = jobs.permits.clone().acquire_owned().await.unwrap();
        let handle = jobs.start(
            1,
            &user,
            config,
            spec(),
            crate::billing::admit_compute(&user).unwrap(),
        );
        sqlx::query("DELETE FROM scenarios WHERE id=1")
            .execute(&jobs.db)
            .await
            .unwrap();
        drop(permit);
        idle(&jobs, handle.id, &user).await;
        assert_eq!(
            jobs.view(handle.id, &user).unwrap().status,
            JobStatus::Canceled
        );
        assert!(metric(
            &jobs,
            "finplan_job_queue_wait_seconds_count{kind=\"sensitivity\",exit=\"deleted\"} 1"
        ));
        assert!(
            jobs.telemetry
                .encode()
                .unwrap()
                .lines()
                .filter(|l| l.starts_with("finplan_job_attempts_total{"))
                .all(|l| l.ends_with(" 0"))
        );
    }

    #[tokio::test]
    async fn sweep_cache_failure_does_not_turn_successful_computation_into_failure() {
        let (jobs, user, mut config) = fixture().await;
        sqlx::query("CREATE TRIGGER fail_cache BEFORE INSERT ON sweep_cache BEGIN SELECT RAISE(ABORT,'synthetic private cache value'); END")
            .execute(&jobs.db).await.unwrap();
        config.birth_date = Some("1960-01-01".parse().unwrap());
        config.events.push(finplan_core::model::Event {
            event_id: finplan_core::model::EventId(0),
            trigger: finplan_core::model::EventTrigger::Age {
                years: 66,
                months: None,
            },
            effects: vec![],
            once: true,
        });
        let spec = JobSpec::Sweep {
            params: vec![],
            config: SweepConfig {
                parameters: vec![SweepParameter::age(
                    finplan_core::model::EventId(0),
                    65,
                    67,
                    2,
                )],
                mc_iterations: 3,
                parallel_batches: 1,
                seed: Some(42),
                ..Default::default()
            },
        };
        let handle = jobs.start(
            1,
            &user,
            config,
            spec,
            crate::billing::admit_compute(&user).unwrap(),
        );
        idle(&jobs, handle.id, &user).await;
        assert_eq!(
            jobs.view(handle.id, &user).unwrap().status,
            JobStatus::Succeeded
        );
        assert!(jobs.outcome(handle.id, &user).is_ok());
        assert!(metric(
            &jobs,
            "finplan_job_attempts_total{kind=\"sweep\",outcome=\"succeeded\"} 1"
        ));
        assert!(metric(
            &jobs,
            "finplan_server_errors_total{component=\"analysis\",class=\"persistence\"} 1"
        ));
        assert!(!jobs.telemetry.encode().unwrap().contains("private"));
    }

    /// A sequential runner with no pool and no progress: what a browser would
    /// plug in.
    struct Sequential;
    impl finplan_core::analysis::McRunner for Sequential {
        fn stats(
            &mut self,
            config: &SimulationConfig,
            mc: &finplan_core::model::MonteCarloConfig,
        ) -> Result<finplan_core::analysis::StatsRun, finplan_core::error::SimulationError>
        {
            finplan_core::simulation::monte_carlo_stats_only(
                config,
                mc,
                &finplan_core::model::MonteCarloProgress::default(),
            )
        }
        fn summary(
            &mut self,
            config: &SimulationConfig,
            mc: &finplan_core::model::MonteCarloConfig,
        ) -> Result<finplan_core::model::MonteCarloSummary, finplan_core::error::SimulationError>
        {
            finplan_core::simulation::monte_carlo_simulate_with_progress(
                config,
                mc,
                &finplan_core::model::MonteCarloProgress::default(),
            )
        }
        fn cancelled(&self) -> bool {
            false
        }
    }

    /// The server's job (rayon-backed, several batches) and the plan crate's
    /// own entry point on a sequential runner give the same answer for the
    /// same seed, for every kind of analysis.
    #[tokio::test]
    async fn server_analyses_equal_the_plan_crate_on_a_sequential_runner() {
        use finplan_plan::analysis::{CreateAnalysis, Limits, discover, prepare};
        use finplan_plan::edit::{self, EditOp};

        let (jobs, user, _) = fixture().await;
        let mut graph: finplan_plan::graph::ScenarioGraph = serde_json::from_str(include_str!(
            "../../../finplan_plan/testdata/default_snapshot.json"
        ))
        .unwrap();
        graph.scenario.duration_years = 10;
        let mut make = |op: serde_json::Value| {
            let op: EditOp = serde_json::from_value(op).unwrap();
            edit::apply(&mut graph, &op).unwrap().id.unwrap_or_default()
        };
        let retire = make(serde_json::json!({"op": "create_parameter", "body": {
            "name": "Retirement age", "value": {"kind": "Age", "years": 45, "months": 0}}}));
        make(serde_json::json!({"op": "create_parameter", "body": {
            "name": "Spending", "value": {"kind": "Money", "value": 2000.0}}}));
        make(serde_json::json!({"op": "create_event", "body": {
            "name": "Retire", "fires_once": true, "enabled": true,
            "trigger": {"kind": "AgeParameter", "parameter_id": retire},
            "effects": [{"kind": "Expense", "from_account_id": 6,
                "amount": {"kind": "Expression", "source": "$Spending"}}]}}));
        let spend = discover(&graph)
            .unwrap()
            .into_iter()
            .find(|p| p.name == "Spending")
            .unwrap()
            .id;

        // Four batches: the pool splits each run, the sequential runner does not.
        let limits = Limits {
            iteration_cap: 1_000,
            parallel_batches: 4,
        };
        let bodies = [
            serde_json::json!({"kind": "sweep", "iterations": 40, "axes": [
                {"parameter_id": spend, "min": 1000.0, "max": 3000.0, "steps": 3}]}),
            serde_json::json!({"kind": "sensitivity", "iterations": 40}),
            serde_json::json!({"kind": "solve", "iterations": 40, "objective": "max-parameter",
                "min_value": 0.9, "vary": [{"parameter_id": spend, "min": 0.0, "max": 5.0e7}]}),
            serde_json::json!({"kind": "what-if", "iterations": 120, "layers": [
                {"kind": "market-shock", "age": 40, "drop": 0.4}]}),
        ];
        for body in bodies {
            let ask = || -> CreateAnalysis { serde_json::from_value(body.clone()).unwrap() };
            let direct = finplan_plan::analysis::analyze(&graph, ask(), &limits, &mut Sequential)
                .unwrap_or_else(|e| panic!("{body}: {e}"));

            let prepared = prepare(&graph, ask(), &limits).unwrap();
            let handle = jobs.start(
                1,
                &user,
                prepared.base,
                prepared.spec,
                crate::billing::admit_compute(&user).unwrap(),
            );
            idle(&jobs, handle.id, &user).await;
            assert_eq!(
                jobs.view(handle.id, &user).unwrap().status,
                JobStatus::Succeeded,
                "{body}"
            );
            let served = jobs.outcome(handle.id, &user).unwrap();
            assert_eq!(
                serde_json::to_value(&served).unwrap(),
                serde_json::to_value(&direct).unwrap(),
                "{body}"
            );
        }
    }
}
