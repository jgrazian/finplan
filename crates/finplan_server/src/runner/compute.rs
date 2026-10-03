//! Offloaded runs (spec 19): a local plan's Monte Carlo run, executed on the
//! runner's worker slots.
//!
//! These jobs share the workers (`RunQueue::worker_permits`) with stored runs,
//! so offload cannot starve or exceed the configured concurrency, but they are
//! not stored runs: no scenario, no `run_*` rows. The compiled plan travels in
//! the [`ComputeJob`] message and is dropped when the job ends; only progress
//! and the finished [`RunResults`] JSON reach `compute_jobs`, and nothing about
//! the plan reaches a log.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use finplan_core::model::{MonteCarloConfig, MonteCarloProgress};
use finplan_core::simulation::monte_carlo_simulate_with_progress;
use finplan_plan::compile::CompiledScenario;
use finplan_plan::results::{RunSettings, project};
use tracing::{Instrument, instrument::WithSubscriber};

use super::RunQueue;
use super::telemetry::{AbortTask, Attempt, PhaseTimer, Submitted};
use crate::billing::ComputePermit;
use crate::observability::{
    Component, ErrorClass, JobContext, JobKind, Origin, Outcome, Phase, QueueExit, Tier,
};

/// One offloaded run, handed to a worker. The plan is in memory only.
pub struct ComputeJob {
    /// The `compute_jobs` row, already inserted as `queued`.
    pub id: i64,
    pub user_id: String,
    pub tier: Tier,
    pub compiled: CompiledScenario,
    pub config: MonteCarloConfig,
    /// What the budget was charged, for the log.
    pub cost: i64,
    /// Held to the job's terminal state, like every run's.
    pub admission: ComputePermit,
}

/// A job's cancellation: the flag the engine polls, and a wake-up for a job
/// still waiting for a worker slot, so canceling it frees its admission at
/// once instead of when a slot comes free.
pub(super) struct ComputeFlag {
    cancel: Arc<AtomicBool>,
    wake: tokio::sync::Notify,
}

/// Removes the job's cancel flag however its task ends.
struct FlagGuard {
    id: i64,
    flags: Arc<StdMutex<HashMap<i64, Arc<ComputeFlag>>>>,
    flag: Arc<ComputeFlag>,
}
impl Drop for FlagGuard {
    fn drop(&mut self) {
        // Also stops a progress reporter that outlived its job.
        self.flag.cancel.store(true, Ordering::Relaxed);
        let mut flags = self.flags.lock().unwrap_or_else(|e| e.into_inner());
        if flags
            .get(&self.id)
            .is_some_and(|flag| Arc::ptr_eq(flag, &self.flag))
        {
            flags.remove(&self.id);
        }
    }
}

enum ComputeError {
    /// Canceled or deleted; nothing left to record.
    Canceled,
    Failed {
        class: ErrorClass,
        message: &'static str,
    },
}

struct Finished {
    json: String,
    iterations: u64,
    engine_seconds: f64,
}

impl RunQueue {
    /// Start an offloaded job. The `compute_jobs` row must exist already; the
    /// job waits for a worker slot, then runs.
    pub fn submit_compute(&self, job: ComputeJob) {
        let flag = Arc::new(ComputeFlag {
            cancel: Arc::new(AtomicBool::new(false)),
            wake: tokio::sync::Notify::new(),
        });
        self.compute_flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(job.id, flag.clone());
        let context = JobContext::new(JobKind::Offload, Origin::Request, &job.user_id, 0, job.id);
        context.event("offload.submitted");
        tracing::info!(
            event = "offload.admitted",
            tier = job.tier.as_str(),
            cost = job.cost,
            iterations = job.config.iterations
        );
        let queue = self.clone();
        let span = context.span();
        tokio::spawn(
            async move { queue.run_compute(job, flag, context).await }
                .instrument(span)
                .with_current_subscriber(),
        );
    }

    /// Ask a queued or running job to stop. A running job notices between
    /// batches and holds its slot until then; a queued one gives up its place
    /// and its admission immediately.
    pub fn cancel_compute(&self, id: i64) {
        if let Some(flag) = self
            .compute_flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        {
            flag.cancel.store(true, Ordering::Relaxed);
            flag.wake.notify_one();
        }
    }

    async fn run_compute(self, job: ComputeJob, flag: Arc<ComputeFlag>, context: JobContext) {
        let cancel = flag.cancel.clone();
        let ComputeJob {
            id,
            user_id,
            tier,
            compiled,
            config,
            cost,
            admission,
        } = job;
        let _admission = admission;
        let _flag = FlagGuard {
            id,
            flags: self.compute_flags.clone(),
            flag: flag.clone(),
        };
        let telemetry = self.telemetry.clone();
        let submitted = Submitted::now();

        let _permit = tokio::select! {
            permit = self.worker_permits.clone().acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => return,
            },
            // Canceled while waiting: give the admission back now.
            () = flag.wake.notified() => {
                telemetry.queue_wait(JobKind::Offload, QueueExit::Canceled, submitted.elapsed());
                telemetry.canceled_before_start(JobKind::Offload);
                context.event("offload.canceled");
                return;
            }
        };
        if cancel.load(Ordering::Relaxed) {
            telemetry.queue_wait(JobKind::Offload, QueueExit::Canceled, submitted.elapsed());
            telemetry.canceled_before_start(JobKind::Offload);
            context.event("offload.canceled");
            return;
        }
        let claimed = sqlx::query(
            "UPDATE compute_jobs SET status = 'running' WHERE id = ?1 AND status = 'queued'",
        )
        .bind(id)
        .execute(&self.db)
        .await;
        match claimed {
            Ok(result) if result.rows_affected() == 1 => {}
            Ok(_) => {
                // Deleted while it waited.
                telemetry.queue_wait(JobKind::Offload, QueueExit::Deleted, submitted.elapsed());
                return;
            }
            Err(_) => {
                telemetry.count_error(Component::Run, ErrorClass::Database);
                tracing::error!(event = "offload.claim_failed", error_class = "database");
                self.fail_compute(id, "Run processing failed").await;
                return;
            }
        }
        telemetry.queue_wait(JobKind::Offload, QueueExit::Started, submitted.elapsed());
        context.event("offload.started");
        let _running = telemetry.job_started(JobKind::Offload);
        let mut attempt = Attempt::new(&telemetry, &context, submitted);

        let outcome = match self.execute_compute(id, compiled, config, &cancel).await {
            Ok(finished) => {
                let persist = PhaseTimer::new(&telemetry, JobKind::Offload, Phase::Persist);
                let stored = sqlx::query(
                    "UPDATE compute_jobs
                        SET status = 'succeeded', result_json = ?2, progress = ?3,
                            finished_at = datetime('now'),
                            expires_at = datetime('now', ?4)
                      WHERE id = ?1 AND status = 'running'",
                )
                .bind(id)
                .bind(&finished.json)
                .bind(finished.iterations as i64)
                .bind(crate::offload::RESULT_TTL)
                .execute(&self.db)
                .await;
                drop(persist);
                match stored {
                    Ok(result) if result.rows_affected() == 1 => {
                        telemetry
                            .iterations_completed(finished.iterations, finished.engine_seconds);
                        tracing::info!(
                            event = "offload.results_stored",
                            actual_iterations = finished.iterations,
                            engine_seconds = finished.engine_seconds,
                            result_bytes = finished.json.len()
                        );
                        Outcome::Succeeded
                    }
                    // Deleted while it ran.
                    Ok(_) => Outcome::Canceled,
                    Err(_) => {
                        telemetry.count_error(Component::Run, ErrorClass::Persistence);
                        attempt.failure(ErrorClass::Persistence);
                        self.fail_compute(id, "Unable to save run results").await;
                        Outcome::Failed
                    }
                }
            }
            Err(ComputeError::Canceled) => Outcome::Canceled,
            Err(ComputeError::Failed { class, message }) => {
                telemetry.count_error(Component::Run, class);
                attempt.failure(class);
                self.fail_compute(id, message).await;
                Outcome::Failed
            }
        };
        tracing::info!(
            event = "offload.finished",
            user_id = %user_id,
            tier = tier.as_str(),
            cost,
            outcome = outcome.as_str()
        );
        attempt.finish(outcome);
    }

    /// Mark a running job failed and give its charge back: the server failed,
    /// not the user.
    async fn fail_compute(&self, id: i64, message: &'static str) {
        let failed: Result<Option<(String, String, i64)>, _> = sqlx::query_as(
            "UPDATE compute_jobs
                SET status = 'failed', error = ?2, finished_at = datetime('now'),
                    expires_at = datetime('now', ?3)
              WHERE id = ?1 AND status IN ('queued', 'running')
          RETURNING user_id, month, cost",
        )
        .bind(id)
        .bind(message)
        .bind(crate::offload::RESULT_TTL)
        .fetch_optional(&self.db)
        .await;
        match failed {
            Ok(Some((user, month, cost))) => {
                crate::offload::refund(&self.db, &user, &month, cost).await;
            }
            Ok(None) => {}
            Err(_) => {
                self.telemetry
                    .count_error(Component::Run, ErrorClass::Persistence);
                tracing::error!(
                    event = "offload.terminal_persistence_failed",
                    error_class = "persistence"
                );
            }
        }
    }

    /// Run the engine, project the summary and serialize it, off the reactor,
    /// reporting progress meanwhile.
    async fn execute_compute(
        &self,
        id: i64,
        compiled: CompiledScenario,
        config: MonteCarloConfig,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Finished, ComputeError> {
        let completed = Arc::new(AtomicUsize::new(0));
        let progress =
            MonteCarloProgress::from_atomics_accumulating(completed.clone(), cancel.clone());
        let reporter = {
            let db = self.db.clone();
            let telemetry = self.telemetry.clone();
            let cancel = cancel.clone();
            let completed = completed.clone();
            let span = tracing::Span::current();
            AbortTask(tokio::spawn(
                async move {
                    let mut ticker = tokio::time::interval(Duration::from_millis(400));
                    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    let mut last = 0;
                    loop {
                        ticker.tick().await;
                        let current = completed.load(Ordering::Relaxed);
                        if current == last {
                            continue;
                        }
                        match sqlx::query(
                            "UPDATE compute_jobs SET progress = ?2
                              WHERE id = ?1 AND status = 'running'",
                        )
                        .bind(id)
                        .bind(current as i64)
                        .execute(&db)
                        .await
                        {
                            Ok(result) if result.rows_affected() == 0 => {
                                // Deleted: the owner canceled.
                                cancel.store(true, Ordering::Relaxed);
                                break;
                            }
                            Ok(_) => last = current,
                            Err(_) => {
                                telemetry.count_error(Component::Run, ErrorClass::Database);
                            }
                        }
                    }
                }
                .instrument(span)
                .with_current_subscriber(),
            ))
        };

        let queued = Instant::now();
        let span = tracing::Span::current();
        let dispatch = tracing::dispatcher::get_default(Clone::clone);
        let telemetry = self.telemetry.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            tracing::dispatcher::with_default(&dispatch, || {
                span.in_scope(|| {
                    telemetry.phase(
                        JobKind::Offload,
                        Phase::BlockingWait,
                        queued.elapsed().as_secs_f64(),
                    );
                    let started = Instant::now();
                    let engine = PhaseTimer::new(&telemetry, JobKind::Offload, Phase::Engine);
                    let summary =
                        monte_carlo_simulate_with_progress(&compiled.config, &config, &progress);
                    drop(engine);
                    let engine_seconds = started.elapsed().as_secs_f64();
                    if progress.is_cancelled() {
                        return Err(ComputeError::Canceled);
                    }
                    let summary = summary.map_err(|error| {
                        // The log keeps the reason, as for stored runs; the
                        // caller sees only the public message.
                        tracing::warn!(event = "offload.engine_error", engine_error = %error);
                        ComputeError::Failed {
                            class: ErrorClass::Engine,
                            message: "Simulation failed",
                        }
                    })?;
                    let results = project(&compiled, &summary, &RunSettings::default());
                    let json =
                        serde_json::to_string(&results).map_err(|_| ComputeError::Failed {
                            class: ErrorClass::Internal,
                            message: "Unable to save run results",
                        })?;
                    Ok(Finished {
                        json,
                        iterations: summary.stats.num_iterations as u64,
                        engine_seconds,
                    })
                })
            })
        })
        .await;
        drop(reporter);
        match outcome {
            Ok(result) => result,
            Err(_) => Err(ComputeError::Failed {
                class: ErrorClass::EnginePanic,
                message: "Simulation worker failed",
            }),
        }
    }
}
