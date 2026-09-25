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
use finplan_core::analysis::{
    SolveConfig, SweepConfig, SweepParameter, SweepProgress, solve, sweep_simulate_lazy,
};
use finplan_core::config::SimulationConfig;
use finplan_core::model::{MonteCarloConfig, MonteCarloProgress, MonteCarloStats};
use finplan_core::simulation::{monte_carlo_simulate_with_progress, monte_carlo_stats_only};
use tokio::sync::Semaphore;
use tracing::{Instrument, instrument::WithSubscriber};

use super::cache;
use super::params::PlanParameter;
use super::results::{
    AnalysisOutcome, AnalysisParameter, AnalysisPoint, SensitivityResults, SensitivityRow,
    SolveOutcome, SweepAxis, SweepCell, SweepResults, WhatIfFan, WhatIfOutcome, WhatIfStep,
};
use crate::db::Db;
use crate::error::{ApiError, ApiResult};

/// Terminal net worth percentiles every analysis asks for, so a cell, a probe
/// and the plan baseline are all measured the same way.
const PERCENTILES: [f64; 5] = [0.05, 0.25, 0.50, 0.75, 0.95];

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

/// The work a job will do, already lowered to engine types.
pub enum JobSpec {
    Sweep {
        params: Vec<PlanParameter>,
        config: SweepConfig,
    },
    Sensitivity {
        params: Vec<PlanParameter>,
        fraction: f64,
        iterations: usize,
        parallel_batches: usize,
        seed: Option<u64>,
    },
    Solve {
        params: Vec<PlanParameter>,
        config: SolveConfig,
    },
    /// The plan and each cumulative override step, already lowered: `steps[0]`
    /// is the plan itself, `steps[i]` the plan with the first `i` layers on.
    WhatIf {
        steps: Vec<SimulationConfig>,
        iterations: usize,
        parallel_batches: usize,
        seed: Option<u64>,
        /// Retirement age before and after the layers, where the plan names one.
        plan_retirement_age: Option<f64>,
        what_if_retirement_age: Option<f64>,
    },
}

impl JobSpec {
    fn kind(&self) -> JobKind {
        match self {
            Self::Sweep { .. } => JobKind::Sweep,
            Self::Sensitivity { .. } => JobKind::Sensitivity,
            Self::Solve { .. } => JobKind::Solve,
            Self::WhatIf { .. } => JobKind::WhatIf,
        }
    }

    /// Simulations the job will run, for the progress denominator. An upper
    /// bound where the search may stop early, which is what a progress bar
    /// wants anyway.
    pub(crate) fn budget(&self) -> usize {
        match self {
            Self::Sweep { config, .. } => (config.total_points() + 1) * config.mc_iterations,
            Self::Sensitivity {
                params, iterations, ..
            } => (params.len() * 2 + 1) * iterations,
            Self::Solve { config, .. } => config.probe_budget() * config.mc_iterations,
            Self::WhatIf {
                steps, iterations, ..
            } => steps.len() * iterations,
        }
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
                    kind: spec.kind(),
                    status: JobStatus::Queued,
                    completed,
                    total,
                    cancel,
                    started: Instant::now(),
                    submitted: Submitted::now(),
                    context: JobContext::new(
                        spec.kind().into(),
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
                            run(&base, &spec, &handle_progress)
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
                    Ok(Err(err)) if err.is_cancel() => (JobStatus::Canceled, None, None),
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

/// A failure from inside the worker, with cancellation kept apart: a canceled
/// analysis is not an error to report, it is the state the caller asked for.
struct WorkerError {
    cancel: bool,
}

impl WorkerError {
    fn is_cancel(&self) -> bool {
        self.cancel
    }
}

impl From<finplan_core::error::SimulationError> for WorkerError {
    fn from(err: finplan_core::error::SimulationError) -> Self {
        let cancel = matches!(err, finplan_core::error::SimulationError::Cancelled);
        Self { cancel }
    }
}

type WorkerResult<T> = Result<T, WorkerError>;

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
    run(base, spec, progress).map_err(|err| {
        if err.is_cancel() {
            InlineFailure::Cancelled
        } else {
            InlineFailure::Failed
        }
    })
}

fn run(
    base: &SimulationConfig,
    spec: &JobSpec,
    progress: &SweepProgress,
) -> WorkerResult<AnalysisOutcome> {
    match spec {
        JobSpec::Sweep { params, config } => Ok(AnalysisOutcome::Sweep(run_sweep(
            base, params, config, progress,
        )?)),
        JobSpec::Sensitivity {
            params,
            fraction,
            iterations,
            parallel_batches,
            seed,
        } => Ok(AnalysisOutcome::Sensitivity(run_sensitivity(
            base,
            params,
            *fraction,
            *iterations,
            *parallel_batches,
            *seed,
            progress,
        )?)),
        JobSpec::Solve { params, config } => {
            let mut results = solve(base, config, Some(progress))?;
            for probe in results.probes.iter_mut().chain(results.best.iter_mut()) {
                for (i, value) in probe.values.iter_mut().enumerate() {
                    *value = params[i].display_coordinate(&config.parameters[i], *value);
                }
                if let Some((lo, hi)) = probe.bracket.as_mut() {
                    *lo = params[0].display_coordinate(&config.parameters[0], *lo);
                    *hi = params[0].display_coordinate(&config.parameters[0], *hi);
                }
            }
            let described: Vec<AnalysisParameter> = params.iter().map(Into::into).collect();
            Ok(AnalysisOutcome::Solve(SolveOutcome::new(
                &results, &described,
            )))
        }
        JobSpec::WhatIf {
            steps,
            iterations,
            parallel_batches,
            seed,
            plan_retirement_age,
            what_if_retirement_age,
        } => Ok(AnalysisOutcome::WhatIf(run_what_if(
            steps,
            *iterations,
            *parallel_batches,
            *seed,
            (*plan_retirement_age, *what_if_retirement_age),
            progress,
        )?)),
    }
}

/// Run each cumulative what-if step on the same seed, and read the plan and
/// the last step's fans off the engine's real-dollar envelope — the same
/// deflated, pointwise quantiles a run stores for its Results chart.
fn run_what_if(
    steps: &[SimulationConfig],
    iterations: usize,
    parallel_batches: usize,
    seed: Option<u64>,
    (plan_retirement_age, what_if_retirement_age): (Option<f64>, Option<f64>),
    progress: &SweepProgress,
) -> WorkerResult<WhatIfOutcome> {
    let Some(plan) = steps.first() else {
        return Err(WorkerError { cancel: false });
    };
    let birth_date = plan.birth_date;

    let mut outcome_steps = Vec::with_capacity(steps.len());
    let mut envelopes = Vec::with_capacity(steps.len());
    for config in steps {
        if progress.is_cancelled() {
            return Err(finplan_core::error::SimulationError::Cancelled.into());
        }
        let mut config = config.clone();
        config.collect_ledger = false;
        let mc = MonteCarloConfig {
            iterations,
            percentiles: PERCENTILES.to_vec(),
            compute_mean: false,
            parallel_batches,
            seed,
            ..Default::default()
        };
        let summary = monte_carlo_simulate_with_progress(&config, &mc, &progress.as_mc_progress())?;
        let real = summary
            .real_net_worth
            .ok_or(WorkerError { cancel: false })?;
        let at = |date: jiff::civil::Date| match birth_date {
            Some(birth) => fractional_years(date) - fractional_years(birth),
            None => fractional_years(date),
        };
        outcome_steps.push(WhatIfStep {
            point: AnalysisPoint::from(&summary.stats),
            median_end_real: real.points.last().map_or(0.0, |p| p.p50),
            p10_dry_at: real
                .points
                .iter()
                .find(|p| p.p10 <= 0.0)
                .map(|p| at(p.date)),
        });
        envelopes.push(real.points);
    }

    let fan = |points: &[finplan_core::model::RealQuantilePoint]| WhatIfFan {
        p25: points.iter().map(|p| p.p25).collect(),
        p50: points.iter().map(|p| p.p50).collect(),
        p75: points.iter().map(|p| p.p75).collect(),
    };
    let plan_points = envelopes.first().cloned().unwrap_or_default();
    let last_points = envelopes.last().cloned().unwrap_or_default();
    let years: Vec<f64> = plan_points
        .iter()
        .map(|p| fractional_years(p.date))
        .collect();
    let ages = birth_date.map(|birth| {
        let born = fractional_years(birth);
        years.iter().map(|year| year - born).collect()
    });

    Ok(WhatIfOutcome {
        steps: outcome_steps,
        ages,
        years,
        plan_fan: fan(&plan_points),
        what_if_fan: fan(&last_points),
        plan_retirement_age,
        what_if_retirement_age,
    })
}

/// A date as a fractional calendar year: 2030-07-02 is about 2030.5.
fn fractional_years(date: jiff::civil::Date) -> f64 {
    f64::from(date.year()) + (f64::from(date.day_of_year()) - 1.0) / f64::from(date.days_in_year())
}

/// The plan as configured, measured the same way every cell is.
fn plan_point(
    base: &SimulationConfig,
    iterations: usize,
    parallel_batches: usize,
    seed: Option<u64>,
    progress: &SweepProgress,
) -> WorkerResult<AnalysisPoint> {
    Ok(AnalysisPoint::from(&simulate(
        base,
        iterations,
        parallel_batches,
        seed,
        progress,
    )?))
}

fn simulate(
    config: &SimulationConfig,
    iterations: usize,
    parallel_batches: usize,
    seed: Option<u64>,
    progress: &SweepProgress,
) -> WorkerResult<MonteCarloStats> {
    if progress.is_cancelled() {
        return Err(finplan_core::error::SimulationError::Cancelled.into());
    }
    let mut config = config.clone();
    // Nothing here reads the ledger, and it is the most expensive thing a
    // simulation collects.
    config.collect_ledger = false;

    let mc = MonteCarloConfig {
        iterations,
        percentiles: PERCENTILES.to_vec(),
        compute_mean: false,
        parallel_batches,
        seed,
        ..Default::default()
    };
    // Stats only, which is all any of the three analyses read. The second pass
    // re-runs simulations to rebuild percentile paths none of them look at, and
    // on the way it accumulates a real-dollar envelope that requires every
    // iteration to share one snapshot date grid — something a plan with a
    // balance- or net-worth-triggered event cannot promise.
    let inner: MonteCarloProgress = progress.as_mc_progress();
    let (stats, _seeds) = monte_carlo_stats_only(&config, &mc, &inner)?;
    Ok(stats)
}

fn run_sweep(
    base: &SimulationConfig,
    params: &[PlanParameter],
    config: &SweepConfig,
    progress: &SweepProgress,
) -> WorkerResult<SweepResults> {
    // The grid first: it resets the shared counter as it starts, so anything
    // measured before it would be counted and then forgotten.
    let grid = sweep_simulate_lazy(base, config, Some(progress))?;
    let values: Vec<Vec<f64>> = config
        .all_sweep_values()
        .into_iter()
        .enumerate()
        .map(|(i, values)| {
            values
                .into_iter()
                .map(|value| {
                    params.get(i).map_or(value, |p| {
                        p.display_coordinate(&config.parameters[i], value)
                    })
                })
                .collect()
        })
        .collect();
    let plan = plan_point(
        base,
        config.mc_iterations,
        config.parallel_batches,
        config.seed,
        progress,
    )?;

    let axes: Vec<SweepAxis> = params
        .iter()
        .zip(&values)
        .map(|(param, steps)| SweepAxis {
            parameter_id: param.id.clone(),
            label: param.name.clone(),
            role: param.name.clone(),
            kind: AnalysisParameter::from(param).kind,
            values: steps.clone(),
        })
        .collect();

    let mut cells = Vec::with_capacity(grid.total_points());
    for indices in grid.stats.indices() {
        let Some(stats) = grid.get_stats(&indices) else {
            continue;
        };
        cells.push(SweepCell {
            indices: indices.iter().map(|&i| i as u32).collect(),
            point: AnalysisPoint::from(stats),
        });
    }

    Ok(SweepResults {
        default_metric: Some("funding".to_string()),
        axes,
        cells,
        plan,
        plan_indices: plan_indices(params, &values),
        iterations: config.mc_iterations as u32,
    })
}

/// Where the plan's own values land on the swept axes.
///
/// `None` as soon as one parameter's current value falls outside the range that
/// was swept: half a marker is worse than none, because it would be drawn on a
/// cell the plan is not in.
fn plan_indices(params: &[PlanParameter], values: &[Vec<f64>]) -> Option<Vec<u32>> {
    params
        .iter()
        .zip(values)
        .map(|(param, steps)| {
            let (lo, hi) = (*steps.first()?, *steps.last()?);
            if param.current < lo.min(hi) || param.current > lo.max(hi) {
                return None;
            }
            let nearest = steps
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (*a - param.current)
                        .abs()
                        .total_cmp(&(*b - param.current).abs())
                })
                .map(|(i, _)| i as u32)?;
            Some(nearest)
        })
        .collect()
}

/// Move each parameter through its band on its own, and rank by how much
/// success moved.
///
/// Two simulations per parameter rather than a grid, which is what makes this
/// cheap enough to be the thing you run before deciding what to sweep.
fn run_sensitivity(
    base: &SimulationConfig,
    params: &[PlanParameter],
    fraction: f64,
    iterations: usize,
    parallel_batches: usize,
    seed: Option<u64>,
    progress: &SweepProgress,
) -> WorkerResult<SensitivityResults> {
    let plan = plan_point(base, iterations, parallel_batches, seed, progress)?;

    let mut rows = Vec::with_capacity(params.len());
    for param in params {
        let (low_value, high_value) = param.perturbed(fraction);
        if (high_value - low_value).abs() < f64::EPSILON {
            continue;
        }
        let at = |value: f64| -> WorkerResult<AnalysisPoint> {
            let axis: SweepParameter = param
                .sweep(value, value, 1)
                .map_err(|_| WorkerError { cancel: false })?;
            let modified = finplan_core::analysis::apply_parameter(base, &axis, axis.min_value)
                .map_err(WorkerError::from)?;
            Ok(AnalysisPoint::from(&simulate(
                &modified,
                iterations,
                parallel_batches,
                seed,
                progress,
            )?))
        };

        let low = at(low_value)?;
        let high = at(high_value)?;
        rows.push(SensitivityRow {
            parameter_id: param.id.clone(),
            label: param.name.clone(),
            kind: AnalysisParameter::from(param).kind,
            low_value,
            high_value,
            span: (high.success_rate - low.success_rate).abs() * 100.0,
            low,
            high,
        });
    }

    // Biggest mover first: the ranking is the point of the screen.
    rows.sort_by(|a, b| b.span.total_cmp(&a.span));

    Ok(SensitivityResults {
        rows,
        plan,
        fraction,
        iterations: iterations as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let graph = crate::compile::rows::ScenarioGraph::load(&db, 1, &user)
            .await
            .unwrap();
        let config = crate::compile::compile(&graph).unwrap().config;
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
}
