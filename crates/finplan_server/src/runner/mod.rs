//! Background execution of Monte Carlo runs.
//!
//! `POST /runs` inserts a `queued` row and hands the id to `RunQueue`. A pool of
//! workers picks runs up, compiles the scenario, executes the simulation on a
//! blocking thread (it is CPU-bound and internally rayon-parallel), mirrors
//! progress into the `runs` row and finally persists results.
//!
//! Because run state lives in SQLite rather than in memory, a crash mid-run is
//! recoverable: `requeue_orphans` re-queues anything left in `running` at boot.

pub mod store;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use finplan_core::model::{MonteCarloConfig, MonteCarloProgress};
use finplan_core::simulation::monte_carlo_simulate_with_progress;
use tokio::sync::{Mutex, Semaphore, mpsc};

use crate::compile::{self, rows::ScenarioGraph};
use crate::db::Db;

/// Handle used by request handlers to enqueue work and cancel in-flight runs.
#[derive(Clone)]
pub struct RunQueue {
    tx: mpsc::UnboundedSender<i64>,
    /// Cancellation flags for runs currently executing, keyed by run id.
    in_flight: Arc<Mutex<HashMap<i64, Arc<AtomicBool>>>>,
}

impl RunQueue {
    pub fn enqueue(&self, run_id: i64) {
        // The receiver lives for the process lifetime; a send failure only
        // happens during shutdown, where dropping the work is correct.
        if self.tx.send(run_id).is_err() {
            tracing::warn!(run_id, "run queue closed; run will stay queued");
        }
    }

    /// Signal a running simulation to stop. Returns false if the run is not
    /// currently executing (it may be queued, or already finished).
    pub async fn cancel(&self, run_id: i64) -> bool {
        let in_flight = self.in_flight.lock().await;
        match in_flight.get(&run_id) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }
}

/// Spawn the worker pool and return the queue handle.
pub fn spawn(db: Db, workers: usize) -> RunQueue {
    let (tx, rx) = mpsc::unbounded_channel::<i64>();
    let in_flight: Arc<Mutex<HashMap<i64, Arc<AtomicBool>>>> = Arc::new(Mutex::new(HashMap::new()));

    let queue = RunQueue {
        tx,
        in_flight: in_flight.clone(),
    };

    let permits = Arc::new(Semaphore::new(workers.max(1)));
    let rx = Arc::new(Mutex::new(rx));

    tokio::spawn(async move {
        loop {
            let run_id = {
                let mut guard = rx.lock().await;
                match guard.recv().await {
                    Some(id) => id,
                    None => break,
                }
            };

            // Acquire before spawning so at most `workers` runs execute at once.
            let permit = match permits.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => break,
            };

            let db = db.clone();
            let in_flight = in_flight.clone();
            tokio::spawn(async move {
                let cancel = Arc::new(AtomicBool::new(false));
                in_flight.lock().await.insert(run_id, cancel.clone());

                if let Err(err) = execute(&db, run_id, cancel).await {
                    tracing::error!(run_id, error = %err, "run failed");
                    let _ = store::mark_failed(&db, run_id, &err.to_string()).await;
                }

                in_flight.lock().await.remove(&run_id);
                drop(permit);
            });
        }
    });

    queue
}

/// Anything a queued run may need to be re-driven after a restart.
pub async fn requeue_orphans(db: &Db, queue: &RunQueue) -> Result<(), sqlx::Error> {
    // A run marked `running` with no worker behind it is an orphan from a
    // previous process; put it back in the queue from the start.
    sqlx::query(
        "UPDATE runs SET status = 'queued', completed_iterations = 0, started_at = NULL
          WHERE status = 'running'",
    )
    .execute(db)
    .await?;

    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM runs WHERE status = 'queued' ORDER BY created_at")
            .fetch_all(db)
            .await?;

    for id in &ids {
        queue.enqueue(*id);
    }
    if !ids.is_empty() {
        tracing::info!(count = ids.len(), "requeued pending runs");
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum RunError {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("{0}")]
    Compile(String),
    #[error("simulation failed: {0}")]
    Engine(String),
    #[error("canceled")]
    Canceled,
}

async fn execute(db: &Db, run_id: i64, cancel: Arc<AtomicBool>) -> Result<(), RunError> {
    // Claim the run. The status guard makes this idempotent if the same id is
    // ever delivered twice.
    let claimed = sqlx::query(
        "UPDATE runs SET status = 'running', started_at = datetime('now')
          WHERE id = ?1 AND status = 'queued'",
    )
    .bind(run_id)
    .execute(db)
    .await?
    .rows_affected();

    if claimed == 0 {
        tracing::debug!(run_id, "run was already claimed or canceled");
        return Ok(());
    }

    let params: (i64, String, i64, Option<i64>, i64, i64, i64) = sqlx::query_as(
        "SELECT scenario_id, user_id, iterations, seed, batch_size, parallel_batches, compute_mean
           FROM runs WHERE id = ?1",
    )
    .bind(run_id)
    .fetch_one(db)
    .await?;

    let (scenario_id, user_id, iterations, seed, batch_size, parallel_batches, compute_mean) =
        params;

    let percentiles: Vec<f64> = sqlx::query_scalar(
        "SELECT percentile FROM run_percentiles WHERE run_id = ?1 ORDER BY percentile",
    )
    .bind(run_id)
    .fetch_all(db)
    .await?;

    let graph = ScenarioGraph::load(db, scenario_id, &user_id)
        .await
        .map_err(|e| RunError::Compile(e.to_string()))?;
    let compiled = compile::compile(&graph).map_err(|e| RunError::Compile(e.to_string()))?;

    let mc_config = MonteCarloConfig {
        iterations: iterations as usize,
        percentiles: if percentiles.is_empty() {
            vec![0.05, 0.50, 0.95]
        } else {
            percentiles
        },
        compute_mean: compute_mean != 0,
        convergence: None,
        batch_size: batch_size as usize,
        parallel_batches: parallel_batches as usize,
        seed: seed.map(|s| s as u64),
    };

    let completed = Arc::new(AtomicUsize::new(0));
    let progress = MonteCarloProgress::from_atomics(completed.clone(), cancel.clone());

    // Mirror the atomic counter into the runs row so pollers see live progress.
    let reporter = {
        let db = db.clone();
        let completed = completed.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(400));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut last = 0usize;
            loop {
                ticker.tick().await;
                let current = completed.load(Ordering::Relaxed);
                if current != last {
                    last = current;
                    let _ = sqlx::query(
                        "UPDATE runs SET completed_iterations = ?2 WHERE id = ?1 AND status = 'running'",
                    )
                    .bind(run_id)
                    .bind(current as i64)
                    .execute(&db)
                    .await;
                }
            }
        })
    };

    let config = compiled.config.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        monte_carlo_simulate_with_progress(&config, &mc_config, &progress)
    })
    .await;

    reporter.abort();

    // A cancel flag set during the run makes the engine return early with a
    // partial result, so check the flag before interpreting the outcome.
    if cancel.load(Ordering::Relaxed) {
        store::mark_canceled(db, run_id).await?;
        return Err(RunError::Canceled);
    }

    let summary = match outcome {
        Ok(Ok(summary)) => summary,
        Ok(Err(err)) => return Err(RunError::Engine(err.to_string())),
        Err(join_err) => {
            return Err(RunError::Engine(format!(
                "simulation worker panicked: {join_err}"
            )));
        }
    };

    store::persist(db, run_id, &compiled, &summary).await?;
    Ok(())
}
