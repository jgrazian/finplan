//! The in-memory job registry, and the work each kind of analysis does.
//!
//! Same contract as a run — POST returns an id, GET polls it, results come back
//! once it says `succeeded` — with the persistence left out. See the module
//! docs for why.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use finplan_core::analysis::{
    SolveConfig, SweepConfig, SweepParameter, SweepProgress, solve, sweep_simulate_lazy,
};
use finplan_core::config::SimulationConfig;
use finplan_core::model::{MonteCarloConfig, MonteCarloProgress, MonteCarloStats};
use finplan_core::simulation::monte_carlo_stats_only;
use tokio::sync::Semaphore;

use super::cache;
use super::params::PlanParameter;
use super::results::{
    AnalysisOutcome, AnalysisParameter, AnalysisPoint, SensitivityResults, SensitivityRow,
    SolveOutcome, SweepAxis, SweepCell, SweepResults,
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
}

impl JobKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sweep => "sweep",
            Self::Sensitivity => "sensitivity",
            Self::Solve => "solve",
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
}

impl JobSpec {
    fn kind(&self) -> JobKind {
        match self {
            Self::Sweep { .. } => JobKind::Sweep,
            Self::Sensitivity { .. } => JobKind::Sensitivity,
            Self::Solve { .. } => JobKind::Solve,
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
    /// Only a finished sweep touches it — see [`cache`].
    db: Db,
    /// Caps concurrent analyses the same way the run pool caps runs: each one
    /// is already rayon-parallel inside.
    permits: Arc<Semaphore>,
}

/// The convenience alias the outcome enum is returned as.
pub type Outcome = AnalysisOutcome;

impl AnalysisJobs {
    #[must_use]
    pub fn new(db: Db, workers: usize) -> Self {
        Self {
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
        tokio::spawn(async move {
            let _admission = admission;
            let Ok(_permit) = permits.acquire_owned().await else {
                return;
            };
            // A job canceled while it waited for a permit never starts.
            if handle_progress.is_cancelled() {
                jobs.finish(id, JobStatus::Canceled, None, None);
                return;
            }
            jobs.mark_running(id);

            let outcome =
                tokio::task::spawn_blocking(move || run(&base, &spec, &handle_progress)).await;

            match outcome {
                Ok(Ok(result)) => {
                    // A sweep outlives its job: the screen it draws is restored
                    // from here after a reload. A write that fails costs the
                    // restore and nothing else, so it is logged, not raised.
                    if let AnalysisOutcome::Sweep(sweep) = &result
                        && let Err(err) = cache::save(&jobs.db, scenario_id, &owner, sweep).await
                    {
                        tracing::warn!(scenario_id, error = %err, "failed to cache sweep");
                    }
                    jobs.finish(id, JobStatus::Succeeded, Some(result), None);
                }
                Ok(Err(err)) if err.is_cancel() => {
                    jobs.finish(id, JobStatus::Canceled, None, None);
                }
                Ok(Err(err)) => jobs.finish(id, JobStatus::Failed, None, Some(err.message)),
                Err(join) => jobs.finish(
                    id,
                    JobStatus::Failed,
                    None,
                    Some(format!("analysis worker panicked: {join}")),
                ),
            }
        });

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
            if !job.status.is_terminal() {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.view(id, user_id)
    }

    fn mark_running(&self, id: i64) {
        let mut reg = self.lock();
        if let Some(job) = reg.jobs.get_mut(&id)
            && job.status == JobStatus::Queued
        {
            job.status = JobStatus::Running;
            job.started = Instant::now();
        }
    }

    fn finish(
        &self,
        id: i64,
        status: JobStatus,
        outcome: Option<AnalysisOutcome>,
        error: Option<String>,
    ) {
        let mut reg = self.lock();
        if let Some(job) = reg.jobs.get_mut(&id) {
            job.status = status;
            job.elapsed_ms = Some(job.started.elapsed().as_millis() as u64);
            job.outcome = outcome;
            job.error = error;
        }
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
    message: String,
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
        Self {
            message: err.to_string(),
            cancel,
        }
    }
}

type WorkerResult<T> = Result<T, WorkerError>;

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
            let results = solve(base, config, Some(progress))?;
            let described: Vec<AnalysisParameter> = params.iter().map(Into::into).collect();
            Ok(AnalysisOutcome::Solve(SolveOutcome::new(
                &results, &described,
            )))
        }
    }
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
    let values = config.all_sweep_values();
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
            label: format!("{} · {}", param.event_name, param.role),
            role: param.role.to_string(),
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
            let axis: SweepParameter = param.sweep(value, value, 1);
            let modified = finplan_core::analysis::apply_parameter(base, &axis, value)
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
            label: format!("{} · {}", param.event_name, param.role),
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
