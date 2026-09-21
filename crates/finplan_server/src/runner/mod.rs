//! Bounded background Monte Carlo execution with persisted restart recovery.
pub mod inputs;
pub mod ledger;
pub mod store;
pub(crate) mod telemetry;
#[cfg(test)]
mod tests;

use crate::compile::{self, rows::ScenarioGraph};
use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::observability::{
    Component, ErrorClass, JobContext, JobKind, Origin, Outcome, Phase, QueueExit, RejectionReason,
    Telemetry,
};
use finplan_core::model::{ConvergenceConfig, MonteCarloConfig, MonteCarloProgress};
use finplan_core::simulation::monte_carlo_simulate_with_progress;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use telemetry::{AbortTask, Attempt, PhaseTimer, Submitted};
use tokio::sync::{Mutex, Semaphore, mpsc};
use tracing::{Instrument, instrument::WithSubscriber};

struct Waiting {
    context: JobContext,
    submitted: Submitted,
    resolved: AtomicBool,
}
impl Waiting {
    fn exit(&self, telemetry: &Telemetry, exit: QueueExit) {
        if !self.resolved.swap(true, Ordering::Relaxed) {
            telemetry.queue_wait(JobKind::Run, exit, self.submitted.elapsed());
            if matches!(exit, QueueExit::Canceled) {
                telemetry.canceled_before_start(JobKind::Run);
                self.context.event("run.canceled");
            }
        }
    }
}
struct Work {
    run_id: i64,
    admission: crate::billing::ComputePermit,
    waiting: Arc<Waiting>,
}

#[derive(Clone)]
pub struct RunQueue {
    tx: mpsc::Sender<Work>,
    #[cfg(test)]
    hooks: Arc<StdMutex<HashMap<i64, Arc<tests::Hook>>>>,
    #[cfg(test)]
    worker_permits: Arc<Semaphore>,
    db: Db,
    telemetry: Telemetry,
    // Held across claim and flag registration, and across cancel's transition.
    in_flight: Arc<StdMutex<HashMap<i64, Arc<AtomicBool>>>>,
    transitions: Arc<Mutex<()>>,
    waiting: Arc<StdMutex<HashMap<i64, Arc<Waiting>>>>,
}

/// Synchronous cleanup also runs when an async worker is aborted or unwinds.
struct RunningRegistration {
    run_id: i64,
    flags: Arc<StdMutex<HashMap<i64, Arc<AtomicBool>>>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for RunningRegistration {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let mut flags = self.flags.lock().unwrap_or_else(|e| e.into_inner());
        if flags
            .get(&self.run_id)
            .is_some_and(|flag| Arc::ptr_eq(flag, &self.cancel))
        {
            flags.remove(&self.run_id);
        }
    }
}
struct PendingCleanup {
    run_id: i64,
    waiting: Arc<StdMutex<HashMap<i64, Arc<Waiting>>>>,
    item: Arc<Waiting>,
}
impl Drop for PendingCleanup {
    fn drop(&mut self) {
        let mut pending = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
        if pending
            .get(&self.run_id)
            .is_some_and(|item| Arc::ptr_eq(item, &self.item))
        {
            pending.remove(&self.run_id);
        }
    }
}

/// A run id alone is not an execution lease: SQLite can reuse it after a
/// cascade. Check the current attempt generation and stored ownership while
/// holding the same lock as submission dispatch and worker claims.
#[derive(Clone)]
struct ExecutionLease {
    db: Db,
    context: JobContext,
    flags: Arc<StdMutex<HashMap<i64, Arc<AtomicBool>>>>,
    cancel: Arc<AtomicBool>,
    transitions: Arc<Mutex<()>>,
}
impl ExecutionLease {
    async fn lock(&self) -> Result<tokio::sync::MutexGuard<'_, ()>, RunError> {
        let guard = self.transitions.lock().await;
        let current = self
            .flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&self.context.job_id)
            .is_some_and(|flag| Arc::ptr_eq(flag, &self.cancel));
        if !current {
            return Err(RunError::Deleted);
        }
        let present: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE id=? AND user_id=? AND scenario_id=? AND status='running')")
            .bind(self.context.job_id).bind(&self.context.user_id).bind(self.context.scenario_id).fetch_one(&self.db).await?;
        if !present {
            return Err(RunError::Deleted);
        }
        Ok(guard)
    }
}

/// Owned channel capacity, reserved before the run's database transaction.
/// Dropping on validation/commit failure releases both channel and admission.
pub struct ReservedRun {
    permit: mpsc::OwnedPermit<Work>,
    queue: RunQueue,
    admission: crate::billing::ComputePermit,
}
impl ReservedRun {
    pub fn send(self, run_id: i64, context: JobContext) {
        self.send_with_clock(run_id, context, Submitted::now());
    }
    fn send_with_clock(self, run_id: i64, context: JobContext, submitted: Submitted) {
        let item = Arc::new(Waiting {
            context,
            submitted,
            resolved: AtomicBool::new(false),
        });
        let waiting = {
            let mut pending = self.queue.waiting.lock().unwrap_or_else(|e| e.into_inner());
            if item.context.origin == Origin::Request {
                if let Some(previous) = pending.insert(run_id, item.clone()) {
                    previous.exit(&self.queue.telemetry, QueueExit::Deleted);
                }
                item
            } else {
                pending.entry(run_id).or_insert(item).clone()
            }
        };
        self.permit.send(Work {
            run_id,
            admission: self.admission,
            waiting,
        });
    }
}
impl RunQueue {
    /// Serialize a submission transaction and dispatch with worker claims. Take
    /// this before BEGIN so a worker never waits on its write while holding it.
    pub async fn submission_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.transitions.lock().await
    }
    pub fn reserve(
        &self,
        admission: crate::billing::ComputePermit,
        origin: Origin,
    ) -> ApiResult<ReservedRun> {
        let permit = self.tx.clone().try_reserve_owned().map_err(|err| {
            let reason = match err {
                mpsc::error::TrySendError::Full(_) => RejectionReason::QueueFull,
                mpsc::error::TrySendError::Closed(_) => RejectionReason::QueueClosed,
            };
            self.telemetry.rejection(reason, origin);
            ApiError::Conflict("Run queue unavailable. Retry shortly.".into())
        })?;
        Ok(ReservedRun {
            permit,
            queue: self.clone(),
            admission,
        })
    }
    pub async fn enqueue(&self, run_id: i64) -> ApiResult<()> {
        let (user, scenario, age): (String, i64, f64) = sqlx::query_as(
            "SELECT user_id, scenario_id, CAST(unixepoch() - unixepoch(created_at) AS REAL) FROM runs WHERE id=? AND status='queued'")
            .bind(run_id).fetch_optional(&self.db).await?.ok_or(ApiError::NotFound("queued run"))?;
        let admission =
            crate::billing::admit_compute_observed(&user, &self.telemetry, Origin::Recovery)?;
        let reservation = self.reserve(admission, Origin::Recovery)?;
        let context = JobContext::new(JobKind::Run, Origin::Recovery, &user, scenario, run_id);
        context.event("run.recovered");
        self.telemetry.recovered();
        reservation.send_with_clock(run_id, context, Submitted::recovered(age, &self.telemetry));
        Ok(())
    }
    /// Resolve against the real transition, not a stale status read in a handler.
    pub async fn cancel(&self, run_id: i64) -> ApiResult<bool> {
        self.cancel_matching(run_id, None).await
    }
    pub async fn cancel_owned(
        &self,
        run_id: i64,
        user_id: &str,
        scenario_id: i64,
    ) -> ApiResult<bool> {
        self.cancel_matching(run_id, Some((user_id, scenario_id)))
            .await
    }
    async fn cancel_matching(&self, run_id: i64, owner: Option<(&str, i64)>) -> ApiResult<bool> {
        let _transition = self.transitions.lock().await;
        if let Some((user_id, scenario_id)) = owner {
            let owned: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM runs WHERE id=? AND user_id=? AND scenario_id=?)",
            )
            .bind(run_id)
            .bind(user_id)
            .bind(scenario_id)
            .fetch_one(&self.db)
            .await?;
            if !owned {
                return Err(ApiError::NotFound("run"));
            }
        }
        let changed = sqlx::query("UPDATE runs SET status='canceled', finished_at=datetime('now') WHERE id=? AND status='queued'")
            .bind(run_id).execute(&self.db).await?.rows_affected();
        if changed == 1 {
            let pending = self
                .waiting
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&run_id);
            if let Some(waiting) = pending {
                waiting.exit(&self.telemetry, QueueExit::Canceled);
            } else {
                let (user, scenario, age): (String,i64,f64) = sqlx::query_as("SELECT user_id,scenario_id,CAST(unixepoch()-unixepoch(created_at) AS REAL) FROM runs WHERE id=?")
                    .bind(run_id).fetch_one(&self.db).await?;
                self.telemetry
                    .queue_wait(JobKind::Run, QueueExit::Canceled, age.max(0.0));
                self.telemetry.canceled_before_start(JobKind::Run);
                JobContext::new(JobKind::Run, Origin::Recovery, &user, scenario, run_id)
                    .event("run.canceled");
            }
            return Ok(true);
        }
        if let Some(flag) = self
            .in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&run_id)
        {
            flag.store(true, Ordering::Relaxed);
            return Ok(true);
        }
        Ok(false)
    }
    pub fn deleted(&self, run_id: i64, queued_age: Option<f64>) {
        if let Some(waiting) = self
            .waiting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&run_id)
        {
            waiting.exit(&self.telemetry, QueueExit::Deleted);
        } else if let Some(age) = queued_age {
            self.telemetry
                .queue_wait(JobKind::Run, QueueExit::Deleted, age.max(0.0));
        }
    }
}

pub fn spawn(db: Db, workers: usize) -> RunQueue {
    spawn_with_telemetry(db, workers, Telemetry::new(workers))
}
pub fn spawn_with_telemetry(db: Db, workers: usize, telemetry: Telemetry) -> RunQueue {
    let (tx, mut rx) = mpsc::channel::<Work>(16);
    let in_flight = Arc::new(StdMutex::new(HashMap::new()));
    let transitions = Arc::new(Mutex::new(()));
    let waiting = Arc::new(StdMutex::new(HashMap::new()));
    let permits = Arc::new(Semaphore::new(workers.max(1)));
    #[cfg(test)]
    let hooks = Arc::new(StdMutex::new(HashMap::<i64, Arc<tests::Hook>>::new()));
    let queue = RunQueue {
        #[cfg(test)]
        hooks: hooks.clone(),
        #[cfg(test)]
        worker_permits: permits.clone(),
        tx,
        db: db.clone(),
        telemetry: telemetry.clone(),
        in_flight: in_flight.clone(),
        transitions: transitions.clone(),
        waiting: waiting.clone(),
    };
    tokio::spawn(async move {
        while let Some(work) = rx.recv().await {
            let Ok(permit) = permits.clone().acquire_owned().await else { break; };
            let db = db.clone();
            let in_flight = in_flight.clone();
            let transitions = transitions.clone();
            let waiting = waiting.clone();
            let telemetry = telemetry.clone();
            #[cfg(test)] let hooks = hooks.clone();
            let span = work.waiting.context.span();
            tokio::spawn(async move {
                let _permit = permit;
                let _admission = work.admission;
                let run_id = work.run_id;
                let _pending_cleanup = PendingCleanup {run_id, waiting: waiting.clone(), item: work.waiting.clone()};
                // A direct cancellation/deletion can resolve this queued item
                // before dispatch. SQLite can reuse deleted INTEGER ids, so a
                // superseded delivery must never claim a later submission.
                let current = waiting.lock().unwrap_or_else(|e| e.into_inner()).get(&run_id)
                    .is_some_and(|item| Arc::ptr_eq(item, &work.waiting));
                if work.waiting.resolved.load(Ordering::Relaxed) || !current { return; }
                let cancel = Arc::new(AtomicBool::new(false));
                let claim = {
                    let _transition = transitions.lock().await;
                    let current = waiting.lock().unwrap_or_else(|e| e.into_inner()).get(&run_id)
                        .is_some_and(|item| Arc::ptr_eq(item, &work.waiting));
                    if work.waiting.resolved.load(Ordering::Relaxed) || !current { return; }
                    let claimed = sqlx::query("UPDATE runs SET status='running', started_at=datetime('now') WHERE id=? AND user_id=? AND scenario_id=? AND status='queued'")
                        .bind(run_id).bind(&work.waiting.context.user_id).bind(work.waiting.context.scenario_id).execute(&db).await;
                    if claimed.as_ref().is_ok_and(|r| r.rows_affected() == 1) { in_flight.lock().unwrap_or_else(|e| e.into_inner()).insert(run_id, cancel.clone()); }
                    claimed
                };
                let claimed = match claim {
                    Ok(result) => result.rows_affected() == 1,
                    Err(_) => {
                        telemetry.count_error(Component::Run, ErrorClass::Database);
                        tracing::error!(event="run.claim_failed", error_class="database");
                        return;
                    }
                };
                if !claimed {
                    // A deleted queued item can disappear through a cascading
                    // scenario/user delete. Direct cancel/delete already resolved it.
                    match sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM runs WHERE id=?)").bind(run_id).fetch_one(&db).await {
                        Ok(false) => work.waiting.exit(&telemetry, QueueExit::Deleted),
                        Ok(true) => {},
                        Err(_) => { telemetry.count_error(Component::Run, ErrorClass::Database); tracing::warn!(event="run.queue_resolution_failed", error_class="database"); }
                    }
                    return;
                }
                let _registration = RunningRegistration {run_id, flags: in_flight.clone(), cancel: cancel.clone()};
                {
                    let mut pending = waiting.lock().unwrap_or_else(|e| e.into_inner());
                    if pending.get(&run_id).is_some_and(|item| Arc::ptr_eq(item, &work.waiting)) {
                        pending.remove(&run_id);
                    }
                }
                work.waiting.exit(&telemetry, QueueExit::Started);
                work.waiting.context.event("run.started");
                let _running = telemetry.job_started(JobKind::Run);
                let mut attempt = Attempt::new(&telemetry, &work.waiting.context, work.waiting.submitted.clone());
                #[cfg(test)] let hook = hooks.lock().unwrap().remove(&run_id);
                #[cfg(test)] if let Some(hook) = &hook { hook.claimed.notify_one(); hook.proceed.notified().await; }
                let lease = ExecutionLease {db:db.clone(),context:work.waiting.context.clone(),flags:in_flight.clone(),cancel:cancel.clone(),transitions:transitions.clone()};
                let result = execute(&db, run_id, cancel, &telemetry, &lease,
                    #[cfg(test)] hook,
                ).await;
                let outcome = match result {
                    Ok(()) => Outcome::Succeeded,
                    Err(RunError::Canceled) => Outcome::Canceled,
                    Err(RunError::Deleted) => Outcome::Interrupted,
                    Err(err) => {
                        telemetry.count_error(Component::Run, err.class());
                        attempt.failure(err.class());
                        let _phase = PhaseTimer::new(&telemetry, JobKind::Run, Phase::Persist);
                        let persisted = async {
                            let _lease = lease.lock().await?;
                            store::mark_failed(&db, run_id, err.public_message()).await.map_err(|_| RunError::Persistence)
                        }.await;
                        match persisted {
                            Ok(true) => Outcome::Failed,
                            Ok(false) | Err(RunError::Deleted) => Outcome::Interrupted,
                            Err(_) => {
                                telemetry.count_error(Component::Run, ErrorClass::Persistence);
                                tracing::error!(event="run.terminal_persistence_failed", error_class="persistence");
                                Outcome::Interrupted
                            }
                        }
                    }
                };
                attempt.finish(outcome);
            }.instrument(span).with_current_subscriber());
        }
    }.with_current_subscriber());
    queue
}

pub async fn requeue_orphans(db: &Db, queue: &RunQueue) -> Result<(), sqlx::Error> {
    tracing::info!(event = "recovery.started");
    let initialization = async {
        sqlx::query("UPDATE runs SET status='queued', completed_iterations=0, started_at=NULL WHERE status='running'").execute(db).await?;
        sqlx::query_scalar::<_,i64>("SELECT COALESCE(MAX(id),0) FROM runs").fetch_one(db).await
    }.await;
    let upper = match initialization {
        Ok(upper) => upper,
        Err(err) => {
            queue
                .telemetry
                .count_error(Component::Recovery, ErrorClass::Database);
            tracing::error!(event = "recovery.failed", error_class = "database");
            return Err(err);
        }
    };
    let db = db.clone();
    let queue = queue.clone();
    tokio::spawn(async move {
        let mut after = 0;
        let mut recovered = 0_u64;
        loop {
            let next = sqlx::query_scalar::<_,i64>("SELECT id FROM runs WHERE status='queued' AND id>? AND id<=? ORDER BY id LIMIT 1")
                .bind(after).bind(upper).fetch_optional(&db).await;
            let id = match next {
                Ok(Some(id)) => id,
                Ok(None) => break,
                Err(_) => { queue.telemetry.count_error(Component::Recovery, ErrorClass::Database); tracing::error!(event="recovery.failed", error_class="database"); return; }
            };
            let mut failures = 0_u64;
            loop {
                match queue.enqueue(id).await {
                    Ok(()) => { recovered += 1; break; },
                    Err(ApiError::Conflict(_)) => tokio::time::sleep(Duration::from_millis(250)).await,
                    Err(ApiError::NotFound(_)) => break,
                    Err(_) => {
                        failures += 1;
                        queue.telemetry.count_error(Component::Recovery, ErrorClass::Database);
                        if failures == 1 || failures.is_multiple_of(60) { tracing::warn!(event="recovery.admission_failed", error_class="database", failures); }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
            after = id;
        }
        tracing::info!(event="recovery.completed", recovered);
    }.with_current_subscriber());
    Ok(())
}

#[derive(Debug)]
enum RunError {
    Db,
    Preparation,
    Engine,
    EnginePanic,
    Persistence,
    Canceled,
    Deleted,
}
impl From<sqlx::Error> for RunError {
    fn from(error: sqlx::Error) -> Self {
        if matches!(error, sqlx::Error::RowNotFound) {
            Self::Deleted
        } else {
            Self::Db
        }
    }
}
impl RunError {
    fn class(&self) -> ErrorClass {
        match self {
            Self::Db => ErrorClass::Database,
            Self::Preparation => ErrorClass::Preparation,
            Self::Engine => ErrorClass::Engine,
            Self::EnginePanic => ErrorClass::EnginePanic,
            Self::Persistence => ErrorClass::Persistence,
            _ => ErrorClass::Internal,
        }
    }
    fn public_message(&self) -> &'static str {
        match self {
            Self::Preparation => "Run inputs are unavailable or invalid; create a new run",
            Self::Engine => "Simulation failed",
            Self::EnginePanic => "Simulation worker failed",
            Self::Persistence => "Unable to save run results",
            _ => "Run processing failed",
        }
    }
}
async fn execute(
    db: &Db,
    run_id: i64,
    cancel: Arc<AtomicBool>,
    telemetry: &Telemetry,
    lease: &ExecutionLease,
    #[cfg(test)] hook: Option<Arc<tests::Hook>>,
) -> Result<(), RunError> {
    let prepare = PhaseTimer::new(telemetry, JobKind::Run, Phase::Prepare);
    let preparation_lease = lease.lock().await?;
    let params: (
        i64,
        String,
        i64,
        Option<i64>,
        i64,
        i64,
        i64,
        i64,
        Option<i64>,
    ) = sqlx::query_as(
        "SELECT scenario_id, user_id, iterations, seed, batch_size, parallel_batches, compute_mean,
                converge, max_iterations
           FROM runs WHERE id = ?1",
    )
    .bind(run_id)
    .fetch_one(db)
    .await?;

    let (
        _scenario_id,
        _user_id,
        iterations,
        seed,
        batch_size,
        parallel_batches,
        compute_mean,
        converge,
        max_iterations,
    ) = params;

    let percentiles: Vec<f64> = sqlx::query_scalar(
        "SELECT percentile FROM run_percentiles WHERE run_id = ?1 ORDER BY percentile",
    )
    .bind(run_id)
    .fetch_all(db)
    .await?;

    let (snapshot, version): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT snapshot_json, model_version FROM runs WHERE id = ?1")
            .bind(run_id)
            .fetch_one(db)
            .await?;
    if version.as_deref() != Some(inputs::MODEL_VERSION) {
        return Err(RunError::Preparation);
    }
    let graph: ScenarioGraph =
        serde_json::from_str(snapshot.as_deref().ok_or(RunError::Preparation)?)
            .map_err(|_| RunError::Preparation)?;
    let compiled = compile::compile(&graph).map_err(|_| RunError::Preparation)?;

    // On a converging run `iterations` is the minimum sample before the metric
    // is tested, and `max_iterations` the ceiling. The row always carries a
    // ceiling when `converge` is set; falling back to the minimum only makes
    // such a run behave like the fixed one it would otherwise have been.
    let convergence = (converge != 0).then(|| ConvergenceConfig {
        max_iterations: max_iterations.unwrap_or(iterations) as usize,
        relative_threshold: 0.01,
        ..ConvergenceConfig::default()
    });

    let mc_config = MonteCarloConfig {
        iterations: iterations as usize,
        percentiles: if percentiles.is_empty() {
            vec![0.05, 0.50, 0.95]
        } else {
            percentiles
        },
        compute_mean: compute_mean != 0,
        convergence,
        batch_size: batch_size as usize,
        parallel_batches: parallel_batches as usize,
        seed: seed.map(|s| s as u64),
    };

    drop(prepare);
    drop(preparation_lease);
    let completed = Arc::new(AtomicUsize::new(0));
    let progress = MonteCarloProgress::from_atomics_accumulating(completed.clone(), cancel.clone());
    let reporter = {
        let db = db.clone();
        let telemetry = telemetry.clone();
        let lease = lease.clone();
        let span = tracing::Span::current();
        AbortTask(tokio::spawn(
            async move {
                let mut ticker = tokio::time::interval(Duration::from_millis(400));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut last = 0;
                let mut failures = 0_u64;
                loop {
                    ticker.tick().await;
                    let current = completed.load(Ordering::Relaxed);
                    if current == last {
                        continue;
                    }
                    let _lease = match lease.lock().await {
                        Ok(guard) => guard,
                        Err(RunError::Deleted) => {
                            lease.cancel.store(true, Ordering::Relaxed);
                            break;
                        }
                        Err(_) => {
                            failures += 1;
                            telemetry.recoverable_error_event(
                                Component::Run,
                                ErrorClass::Database,
                                "run.progress_failed",
                            );
                            continue;
                        }
                    };
                    match sqlx::query(
                        "UPDATE runs SET completed_iterations=?2 WHERE id=?1 AND status='running'",
                    )
                    .bind(run_id)
                    .bind(current as i64)
                    .execute(&db)
                    .await
                    {
                        Ok(_) => {
                            last = current;
                            if failures != 0 {
                                tracing::info!(event = "run.progress_recovered", failures);
                                failures = 0;
                            }
                        }
                        Err(_) => {
                            failures += 1;
                            telemetry.count_error(Component::Run, ErrorClass::Database);
                            if failures == 1 || failures.is_multiple_of(60) {
                                tracing::warn!(
                                    event = "run.progress_failed",
                                    error_class = "database",
                                    failures
                                );
                            }
                        }
                    }
                }
            }
            .instrument(span)
            .with_current_subscriber(),
        ))
    };
    let config = compiled.config.clone();
    let blocking_queued = Instant::now();
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let worker_telemetry = telemetry.clone();
    #[cfg(test)]
    let panic = hook.as_ref().is_some_and(|hook| hook.panic);
    let outcome = tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            span.in_scope(|| {
                worker_telemetry.phase(
                    JobKind::Run,
                    Phase::BlockingWait,
                    blocking_queued.elapsed().as_secs_f64(),
                );
                let engine_started = Instant::now();
                let _engine = PhaseTimer::new(&worker_telemetry, JobKind::Run, Phase::Engine);
                #[cfg(test)]
                if panic {
                    panic!("synthetic engine panic");
                }
                (
                    monte_carlo_simulate_with_progress(&config, &mc_config, &progress),
                    engine_started.elapsed().as_secs_f64(),
                )
            })
        })
    })
    .await;
    #[cfg(test)]
    if let Some(hook) = hook
        && hook.pause_after_engine
    {
        hook.engine_finished.notify_one();
        hook.finish_allowed.notified().await;
    }
    drop(reporter);
    if cancel.load(Ordering::Relaxed) {
        let _persist = PhaseTimer::new(telemetry, JobKind::Run, Phase::Persist);
        let _lease = lease.lock().await?;
        return if store::mark_canceled(db, run_id)
            .await
            .map_err(|_| RunError::Persistence)?
        {
            Err(RunError::Canceled)
        } else {
            Err(RunError::Deleted)
        };
    }
    let (summary, engine_seconds) = match outcome {
        Ok((Ok(summary), seconds)) => (summary, seconds),
        Ok((Err(_), _)) => return Err(RunError::Engine),
        Err(_) => return Err(RunError::EnginePanic),
    };
    let _persist = PhaseTimer::new(telemetry, JobKind::Run, Phase::Persist);
    let _lease = lease.lock().await?;
    match store::persist(db, run_id, &compiled, &summary).await {
        Ok(()) => {}
        Err(sqlx::Error::RowNotFound) => return Err(RunError::Deleted),
        Err(_) => return Err(RunError::Persistence),
    }
    telemetry.iterations_completed(summary.stats.num_iterations as u64, engine_seconds);
    tracing::info!(
        event = "run.samples_persisted",
        actual_iterations = summary.stats.num_iterations,
        engine_seconds,
        iterations_per_second =
            summary.stats.num_iterations as f64 / engine_seconds.max(f64::MIN_POSITIVE)
    );
    Ok(())
}
