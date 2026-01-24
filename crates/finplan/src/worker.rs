//! Background worker for running simulations without blocking the UI.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use finplan_core::config::SimulationConfig;
use finplan_core::model::{MonteCarloConfig, SimulationResult as CoreResult};

use crate::data::convert::to_tui_result;
use crate::state::{MonteCarloPreviewSummary, MonteCarloStoredResult, SimulationResult};

/// Request sent to the background worker
#[derive(Debug)]
pub enum SimulationRequest {
    /// Run a single deterministic simulation
    Single {
        config: SimulationConfig,
        seed: u64,
        birth_date: String,
        start_date: String,
    },
    /// Run Monte Carlo simulation
    MonteCarlo {
        config: SimulationConfig,
        iterations: usize,
        birth_date: String,
        start_date: String,
    },
    /// Graceful shutdown
    Shutdown,
}

/// Response from the background worker
#[derive(Debug)]
pub enum SimulationResponse {
    /// Progress update for Monte Carlo
    Progress { current: usize, total: usize },
    /// Single simulation completed
    SingleComplete {
        tui_result: SimulationResult,
        core_result: CoreResult,
    },
    /// Monte Carlo simulation completed (boxed to reduce enum size)
    MonteCarloComplete {
        stored_result: Box<MonteCarloStoredResult>,
        preview_summary: MonteCarloPreviewSummary,
        default_tui_result: SimulationResult,
        default_core_result: CoreResult,
    },
    /// Simulation was cancelled
    Cancelled,
    /// Error occurred
    Error(String),
}

/// Background worker that runs simulations on a separate thread
pub struct SimulationWorker {
    request_tx: Sender<SimulationRequest>,
    response_rx: Receiver<SimulationResponse>,
    cancel_flag: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
}

impl SimulationWorker {
    /// Create a new simulation worker with a background thread
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel();
        let (response_tx, response_rx) = channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicUsize::new(0));

        let flag_clone = cancel_flag.clone();
        let progress_clone = progress.clone();

        let thread = thread::spawn(move || {
            worker_loop(request_rx, response_tx, flag_clone, progress_clone);
        });

        Self {
            request_tx,
            response_rx,
            cancel_flag,
            progress,
            thread: Some(thread),
        }
    }

    /// Send a simulation request to the worker
    pub fn send(&self, request: SimulationRequest) -> bool {
        // Clear cancel flag for new work
        self.cancel_flag.store(false, Ordering::SeqCst);
        self.progress.store(0, Ordering::SeqCst);
        self.request_tx.send(request).is_ok()
    }

    /// Try to receive a response (non-blocking)
    pub fn try_recv(&self) -> Option<SimulationResponse> {
        self.response_rx.try_recv().ok()
    }

    /// Get current progress for Monte Carlo (0..total)
    pub fn get_progress(&self) -> usize {
        self.progress.load(Ordering::SeqCst)
    }

    /// Request cancellation of the current operation
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    /// Check if cancellation was requested
    pub fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst)
    }

    /// Shutdown the worker thread
    pub fn shutdown(&self) {
        let _ = self.request_tx.send(SimulationRequest::Shutdown);
    }
}

impl Default for SimulationWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SimulationWorker {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Main worker loop running on background thread
fn worker_loop(
    request_rx: Receiver<SimulationRequest>,
    response_tx: Sender<SimulationResponse>,
    cancel_flag: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
) {
    while let Ok(request) = request_rx.recv() {
        match request {
            SimulationRequest::Shutdown => break,

            SimulationRequest::Single {
                config,
                seed,
                birth_date,
                start_date,
            } => {
                if cancel_flag.load(Ordering::SeqCst) {
                    let _ = response_tx.send(SimulationResponse::Cancelled);
                    continue;
                }

                match run_single_simulation(&config, seed, &birth_date, &start_date) {
                    Ok((tui_result, core_result)) => {
                        let _ = response_tx.send(SimulationResponse::SingleComplete {
                            tui_result,
                            core_result,
                        });
                    }
                    Err(e) => {
                        let _ = response_tx.send(SimulationResponse::Error(e));
                    }
                }
            }

            SimulationRequest::MonteCarlo {
                config,
                iterations,
                birth_date,
                start_date,
            } => {
                progress.store(0, Ordering::SeqCst);

                match run_monte_carlo_simulation(
                    &config,
                    iterations,
                    &birth_date,
                    &start_date,
                    &cancel_flag,
                    &progress,
                    &response_tx,
                ) {
                    Ok(Some(result)) => {
                        let _ = response_tx.send(result);
                    }
                    Ok(None) => {
                        // Cancelled
                        let _ = response_tx.send(SimulationResponse::Cancelled);
                    }
                    Err(e) => {
                        let _ = response_tx.send(SimulationResponse::Error(e));
                    }
                }
            }
        }
    }
}

fn run_single_simulation(
    config: &SimulationConfig,
    seed: u64,
    birth_date: &str,
    start_date: &str,
) -> Result<(SimulationResult, CoreResult), String> {
    let core_result =
        finplan_core::simulation::simulate(config, seed).map_err(|e| e.to_string())?;

    let tui_result =
        to_tui_result(&core_result, birth_date, start_date).map_err(|e| e.to_string())?;

    Ok((tui_result, core_result))
}

fn run_monte_carlo_simulation(
    config: &SimulationConfig,
    iterations: usize,
    birth_date: &str,
    start_date: &str,
    cancel_flag: &Arc<AtomicBool>,
    progress: &Arc<AtomicUsize>,
    _response_tx: &Sender<SimulationResponse>,
) -> Result<Option<SimulationResponse>, String> {
    // Check for cancellation before starting
    if cancel_flag.load(Ordering::SeqCst) {
        return Ok(None);
    }

    // Configure Monte Carlo simulation
    let mc_config = MonteCarloConfig {
        iterations,
        percentiles: vec![0.05, 0.50, 0.95],
        compute_mean: true,
    };

    // Run Monte Carlo with progress updates
    // Note: finplan_core doesn't expose per-iteration callbacks, so we run the full simulation
    // and update progress based on completion. For true progress, we'd need to modify the core.
    let mc_summary = finplan_core::simulation::monte_carlo_simulate_with_config(config, &mc_config)
        .map_err(|e| e.to_string())?;

    // Update progress to completion
    progress.store(iterations, Ordering::SeqCst);

    // Check for cancellation after simulation
    if cancel_flag.load(Ordering::SeqCst) {
        return Ok(None);
    }

    // Convert percentile runs to TUI format
    let mut percentile_results = Vec::new();
    for (p, core_result) in &mc_summary.percentile_runs {
        let tui_result =
            to_tui_result(core_result, birth_date, start_date).map_err(|e| e.to_string())?;
        percentile_results.push((*p, tui_result, core_result.clone()));
    }

    // Build mean results from accumulators
    let (mean_tui_result, mean_core_result) = if let Some(mean_core) = mc_summary.get_mean_result()
    {
        let mean_tui =
            to_tui_result(&mean_core, birth_date, start_date).map_err(|e| e.to_string())?;
        (Some(mean_tui), Some(mean_core))
    } else {
        (None, None)
    };

    // Extract P50 as default result
    let (default_tui_result, default_core_result) = percentile_results
        .iter()
        .find(|(p, _, _)| (*p - 0.50).abs() < 0.001)
        .map(|(_, tui, core)| (tui.clone(), core.clone()))
        .ok_or_else(|| "Missing P50 result".to_string())?;

    // Build preview summary
    let p5_final = mc_summary
        .stats
        .percentile_values
        .iter()
        .find(|(p, _)| (*p - 0.05).abs() < 0.001)
        .map(|(_, v)| *v)
        .unwrap_or(0.0);
    let p50_final = mc_summary
        .stats
        .percentile_values
        .iter()
        .find(|(p, _)| (*p - 0.50).abs() < 0.001)
        .map(|(_, v)| *v)
        .unwrap_or(0.0);
    let p95_final = mc_summary
        .stats
        .percentile_values
        .iter()
        .find(|(p, _)| (*p - 0.95).abs() < 0.001)
        .map(|(_, v)| *v)
        .unwrap_or(0.0);

    let preview_summary = MonteCarloPreviewSummary {
        num_iterations: mc_summary.stats.num_iterations,
        success_rate: mc_summary.stats.success_rate,
        p5_final,
        p50_final,
        p95_final,
    };

    // Build stored result
    let stored_result = MonteCarloStoredResult {
        stats: mc_summary.stats,
        percentile_results,
        mean_tui_result,
        mean_core_result,
    };

    Ok(Some(SimulationResponse::MonteCarloComplete {
        stored_result: Box::new(stored_result),
        preview_summary,
        default_tui_result,
        default_core_result,
    }))
}
