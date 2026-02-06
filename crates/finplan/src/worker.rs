//! Background worker for running simulations without blocking the UI.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use finplan_core::analysis::{SweepConfig, SweepProgress, SweepResults, sweep_evaluate};
use finplan_core::config::SimulationConfig;
use finplan_core::model::{
    ConvergenceConfig, ConvergenceMetric, MonteCarloConfig, MonteCarloProgress,
    SimulationResult as CoreResult,
};

use crate::data::convert::to_tui_result;
use crate::state::{
    MonteCarloPreviewSummary, MonteCarloStoredResult, ScenarioSummary, SimulationResult,
};
use crate::util::common::cpu_parallel_batches;
use crate::util::percentiles::{
    PercentileSet, find_percentile_result, find_percentile_result_pair, standard::P50,
};

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
        /// Optional seed for reproducible results (None = random each run)
        seed: Option<u64>,
        birth_date: String,
        start_date: String,
    },
    /// Run Monte Carlo with convergence-based stopping
    MonteCarloConvergence {
        config: SimulationConfig,
        min_iterations: usize,
        max_iterations: usize,
        relative_threshold: f64,
        metric: ConvergenceMetric,
        /// Optional seed for reproducible results (None = random each run)
        seed: Option<u64>,
        birth_date: String,
        start_date: String,
    },
    /// Run Monte Carlo on multiple scenarios (batch mode)
    Batch {
        /// Vec of (scenario_name, config, seed, birth_date, start_date)
        scenarios: Vec<(String, SimulationConfig, Option<u64>, String, String)>,
        iterations: usize,
    },
    /// Run sweep analysis (parameter sensitivity)
    SweepAnalysis {
        config: SimulationConfig,
        sweep_config: SweepConfig,
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
    /// Batch Monte Carlo completed for one scenario
    BatchScenarioComplete {
        scenario_name: String,
        summary: ScenarioSummary,
    },
    /// All batch scenarios completed
    BatchComplete { completed_count: usize },
    /// Simulation was cancelled
    Cancelled,
    /// Error occurred
    Error(String),
    /// Sweep analysis progress
    SweepProgress { current: usize, total: usize },
    /// Sweep analysis completed
    SweepComplete { results: Box<SweepResults> },
}

/// Background worker that runs simulations on a separate thread
pub struct SimulationWorker {
    request_tx: Sender<SimulationRequest>,
    response_rx: Receiver<SimulationResponse>,
    cancel_flag: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
    /// For batch runs: current scenario index
    batch_scenario_index: Arc<AtomicUsize>,
    /// For batch runs: total number of scenarios
    batch_scenario_total: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
}

impl SimulationWorker {
    /// Create a new simulation worker with a background thread
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel();
        let (response_tx, response_rx) = channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicUsize::new(0));
        let batch_scenario_index = Arc::new(AtomicUsize::new(0));
        let batch_scenario_total = Arc::new(AtomicUsize::new(0));

        let ctx = WorkerContext {
            response_tx,
            cancel_flag: cancel_flag.clone(),
            progress: progress.clone(),
            batch_scenario_index: batch_scenario_index.clone(),
            batch_scenario_total: batch_scenario_total.clone(),
        };

        let thread = thread::spawn(move || {
            ctx.run(request_rx);
        });

        Self {
            request_tx,
            response_rx,
            cancel_flag,
            progress,
            batch_scenario_index,
            batch_scenario_total,
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

    /// Get current progress for Monte Carlo (0..total iterations)
    pub fn get_progress(&self) -> usize {
        self.progress.load(Ordering::SeqCst)
    }

    /// Get current batch scenario index (0..total scenarios)
    pub fn get_batch_scenario_index(&self) -> usize {
        self.batch_scenario_index.load(Ordering::SeqCst)
    }

    /// Get total number of scenarios in batch
    pub fn get_batch_scenario_total(&self) -> usize {
        self.batch_scenario_total.load(Ordering::SeqCst)
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

/// Shared state for the background worker thread.
struct WorkerContext {
    response_tx: Sender<SimulationResponse>,
    cancel_flag: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
    batch_scenario_index: Arc<AtomicUsize>,
    batch_scenario_total: Arc<AtomicUsize>,
}

impl WorkerContext {
    fn run(&self, request_rx: Receiver<SimulationRequest>) {
        while let Ok(request) = request_rx.recv() {
            match request {
                SimulationRequest::Shutdown => break,

                SimulationRequest::Single {
                    config,
                    seed,
                    birth_date,
                    start_date,
                } => {
                    tracing::info!(seed = seed, "Starting single simulation");
                    if self.cancel_flag.load(Ordering::SeqCst) {
                        let _ = self.response_tx.send(SimulationResponse::Cancelled);
                        continue;
                    }

                    match Self::run_single(&config, seed, &birth_date, &start_date) {
                        Ok((tui_result, core_result)) => {
                            let _ = self.response_tx.send(SimulationResponse::SingleComplete {
                                tui_result,
                                core_result,
                            });
                        }
                        Err(e) => {
                            let _ = self.response_tx.send(SimulationResponse::Error(e));
                        }
                    }
                }

                SimulationRequest::MonteCarlo {
                    config,
                    iterations,
                    seed,
                    birth_date,
                    start_date,
                } => {
                    tracing::info!(iterations = iterations, seed = ?seed, "Starting Monte Carlo simulation");
                    self.progress.store(0, Ordering::SeqCst);

                    match self.run_monte_carlo(
                        &config,
                        iterations,
                        None,
                        seed,
                        &birth_date,
                        &start_date,
                    ) {
                        Ok(Some(result)) => {
                            let _ = self.response_tx.send(result);
                        }
                        Ok(None) => {
                            let _ = self.response_tx.send(SimulationResponse::Cancelled);
                        }
                        Err(e) => {
                            let _ = self.response_tx.send(SimulationResponse::Error(e));
                        }
                    }
                }

                SimulationRequest::MonteCarloConvergence {
                    config,
                    min_iterations,
                    max_iterations,
                    relative_threshold,
                    metric,
                    seed,
                    birth_date,
                    start_date,
                } => {
                    self.progress.store(0, Ordering::SeqCst);

                    let convergence = Some(ConvergenceConfig {
                        metric,
                        relative_threshold,
                        max_iterations,
                    });

                    match self.run_monte_carlo(
                        &config,
                        min_iterations,
                        convergence,
                        seed,
                        &birth_date,
                        &start_date,
                    ) {
                        Ok(Some(result)) => {
                            let _ = self.response_tx.send(result);
                        }
                        Ok(None) => {
                            let _ = self.response_tx.send(SimulationResponse::Cancelled);
                        }
                        Err(e) => {
                            let _ = self.response_tx.send(SimulationResponse::Error(e));
                        }
                    }
                }

                SimulationRequest::Batch {
                    scenarios,
                    iterations,
                } => {
                    tracing::info!(
                        scenarios = scenarios.len(),
                        iterations = iterations,
                        "Starting batch Monte Carlo simulation"
                    );
                    self.batch_scenario_index.store(0, Ordering::SeqCst);
                    self.batch_scenario_total
                        .store(scenarios.len(), Ordering::SeqCst);
                    self.progress.store(0, Ordering::SeqCst);

                    match self.run_batch_monte_carlo(scenarios, iterations) {
                        Ok(count) => {
                            let _ = self.response_tx.send(SimulationResponse::BatchComplete {
                                completed_count: count,
                            });
                        }
                        Err(_) => {
                            let _ = self.response_tx.send(SimulationResponse::Cancelled);
                        }
                    }
                }

                SimulationRequest::SweepAnalysis {
                    config,
                    sweep_config,
                    birth_date: _,
                    start_date: _,
                } => {
                    let total_points = sweep_config.total_points();
                    tracing::info!(total_points = total_points, "Starting sweep analysis");

                    if self.cancel_flag.load(Ordering::SeqCst) {
                        let _ = self.response_tx.send(SimulationResponse::Cancelled);
                        continue;
                    }

                    self.progress.store(0, Ordering::SeqCst);
                    self.batch_scenario_total
                        .store(total_points, Ordering::SeqCst);

                    let sweep_progress = SweepProgress::from_atomics(
                        self.progress.clone(),
                        self.batch_scenario_total.clone(),
                        self.cancel_flag.clone(),
                    );

                    match sweep_evaluate(&config, &sweep_config, Some(&sweep_progress)) {
                        Ok(results) => {
                            let _ = self.response_tx.send(SimulationResponse::SweepComplete {
                                results: Box::new(results),
                            });
                        }
                        Err(finplan_core::error::SimulationError::Cancelled) => {
                            let _ = self.response_tx.send(SimulationResponse::Cancelled);
                        }
                        Err(e) => {
                            let _ = self
                                .response_tx
                                .send(SimulationResponse::Error(e.to_string()));
                        }
                    }
                }
            }
        }
    }

    fn run_single(
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

    fn run_monte_carlo(
        &self,
        config: &SimulationConfig,
        iterations: usize,
        convergence: Option<ConvergenceConfig>,
        seed: Option<u64>,
        birth_date: &str,
        start_date: &str,
    ) -> Result<Option<SimulationResponse>, String> {
        if self.cancel_flag.load(Ordering::SeqCst) {
            return Ok(None);
        }

        let mc_config = MonteCarloConfig {
            iterations,
            percentiles: vec![0.05, 0.50, 0.95],
            compute_mean: true,
            convergence,
            parallel_batches: cpu_parallel_batches(),
            seed,
            ..Default::default()
        };

        let mc_progress =
            MonteCarloProgress::from_atomics(self.progress.clone(), self.cancel_flag.clone());

        let mc_summary = match finplan_core::simulation::monte_carlo_simulate_with_progress(
            config,
            &mc_config,
            &mc_progress,
        ) {
            Ok(summary) => summary,
            Err(finplan_core::error::SimulationError::Cancelled) => {
                return Ok(None);
            }
            Err(e) => return Err(e.to_string()),
        };

        self.progress.store(iterations, Ordering::SeqCst);

        if self.cancel_flag.load(Ordering::SeqCst) {
            return Ok(None);
        }

        let mut percentile_results = Vec::new();
        for (p, core_result) in &mc_summary.percentile_runs {
            let tui_result =
                to_tui_result(core_result, birth_date, start_date).map_err(|e| e.to_string())?;
            percentile_results.push((*p, tui_result, core_result.clone()));
        }

        let (mean_tui_result, mean_core_result) =
            if let Some(mean_core) = mc_summary.get_mean_result() {
                let mean_tui =
                    to_tui_result(&mean_core, birth_date, start_date).map_err(|e| e.to_string())?;
                (Some(mean_tui), Some(mean_core))
            } else {
                (None, None)
            };

        let (default_tui_result, default_core_result) =
            find_percentile_result_pair(&percentile_results, P50)
                .map(|(tui, core)| (tui.clone(), core.clone()))
                .ok_or_else(|| "Missing P50 result".to_string())?;

        let pset = PercentileSet::from_values_or_default(&mc_summary.stats.percentile_values);

        let preview_summary = MonteCarloPreviewSummary {
            num_iterations: mc_summary.stats.num_iterations,
            success_rate: mc_summary.stats.success_rate,
            p5_final: pset.p5,
            p50_final: pset.p50,
            p95_final: pset.p95,
        };

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

    fn run_batch_monte_carlo(
        &self,
        scenarios: Vec<(String, SimulationConfig, Option<u64>, String, String)>,
        iterations: usize,
    ) -> Result<usize, ()> {
        let mut completed_count = 0;

        for (idx, (scenario_name, config, seed, birth_date, start_date)) in
            scenarios.into_iter().enumerate()
        {
            self.batch_scenario_index.store(idx, Ordering::SeqCst);
            self.progress.store(0, Ordering::SeqCst);

            if self.cancel_flag.load(Ordering::SeqCst) {
                return Err(());
            }

            let mc_config = MonteCarloConfig {
                iterations,
                percentiles: vec![0.05, 0.50, 0.95],
                compute_mean: false,
                parallel_batches: cpu_parallel_batches(),
                seed,
                ..Default::default()
            };

            let mc_progress =
                MonteCarloProgress::from_atomics(self.progress.clone(), self.cancel_flag.clone());

            let mc_summary = match finplan_core::simulation::monte_carlo_simulate_with_progress(
                &config,
                &mc_config,
                &mc_progress,
            ) {
                Ok(summary) => summary,
                Err(finplan_core::error::SimulationError::Cancelled) => {
                    return Err(());
                }
                Err(e) => {
                    tracing::warn!(scenario = scenario_name, error = %e, "Monte Carlo failed");
                    continue;
                }
            };

            let pset = PercentileSet::from_values_or_default(&mc_summary.stats.percentile_values);
            let (p5, p50, p95) = (pset.p5, pset.p50, pset.p95);

            let p50_tui = find_percentile_result(&mc_summary.percentile_runs, P50)
                .and_then(|core_result| to_tui_result(core_result, &birth_date, &start_date).ok());

            let yearly_nw = p50_tui
                .as_ref()
                .map(|tui| tui.years.iter().map(|y| (y.year, y.net_worth)).collect());
            let yearly_real_nw = p50_tui.as_ref().map(|tui| {
                tui.years
                    .iter()
                    .map(|y| (y.year, y.real_net_worth))
                    .collect()
            });

            let (final_real_nw, real_p5, real_p50, real_p95) = if let Some(ref tui) = p50_tui {
                let final_real = tui.final_real_net_worth;
                let inflation_factor = if tui.final_real_net_worth > 0.0 {
                    tui.final_net_worth / tui.final_real_net_worth
                } else {
                    1.0
                };
                let real_p5 = if inflation_factor > 0.0 {
                    p5 / inflation_factor
                } else {
                    p5
                };
                let real_p50 = if inflation_factor > 0.0 {
                    p50 / inflation_factor
                } else {
                    p50
                };
                let real_p95 = if inflation_factor > 0.0 {
                    p95 / inflation_factor
                } else {
                    p95
                };
                (Some(final_real), real_p5, real_p50, real_p95)
            } else {
                (None, p5, p50, p95)
            };

            let summary = ScenarioSummary {
                name: scenario_name.clone(),
                final_net_worth: Some(p50),
                success_rate: Some(mc_summary.stats.success_rate),
                percentiles: Some((p5, p50, p95)),
                yearly_net_worth: yearly_nw,
                final_real_net_worth: final_real_nw,
                real_percentiles: Some((real_p5, real_p50, real_p95)),
                yearly_real_net_worth: yearly_real_nw,
            };

            let _ = self
                .response_tx
                .send(SimulationResponse::BatchScenarioComplete {
                    scenario_name,
                    summary,
                });

            completed_count += 1;
        }

        Ok(completed_count)
    }
}
