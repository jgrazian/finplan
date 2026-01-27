//! Web worker implementation for running simulations.
//!
//! This initial implementation runs simulations synchronously in the main thread.
//! A future enhancement (Phase 6) will use actual Web Workers for background execution.

use std::cell::RefCell;
use std::collections::VecDeque;

use finplan_core::config::SimulationConfig;
use finplan_core::model::MonteCarloConfig;

use crate::data::convert::to_tui_result;
use crate::platform::worker::{SimulationRequest, SimulationResponse, SimulationWorker};
use crate::state::{MonteCarloPreviewSummary, MonteCarloStoredResult, ScenarioSummary};

/// Web worker implementation that runs simulations synchronously.
///
/// This is a temporary implementation that blocks the UI during simulation.
/// Phase 6 will add proper Web Worker support for background execution.
pub struct WebWorker {
    /// Queue of pending responses
    responses: RefCell<VecDeque<SimulationResponse>>,
    /// Current progress (for Monte Carlo)
    progress: RefCell<usize>,
    /// Batch scenario index
    batch_index: RefCell<usize>,
    /// Batch scenario total
    batch_total: RefCell<usize>,
    /// Cancellation flag
    cancelled: RefCell<bool>,
}

impl WebWorker {
    /// Create a new web worker.
    pub fn new() -> Self {
        Self {
            responses: RefCell::new(VecDeque::new()),
            progress: RefCell::new(0),
            batch_index: RefCell::new(0),
            batch_total: RefCell::new(0),
            cancelled: RefCell::new(false),
        }
    }

    /// Run a single simulation synchronously.
    fn run_single(
        &self,
        config: &SimulationConfig,
        seed: u64,
        birth_date: &str,
        start_date: &str,
    ) -> SimulationResponse {
        match finplan_core::simulation::simulate(config, seed) {
            Ok(core_result) => match to_tui_result(&core_result, birth_date, start_date) {
                Ok(tui_result) => SimulationResponse::SingleComplete {
                    tui_result,
                    core_result,
                },
                Err(e) => SimulationResponse::Error(e.to_string()),
            },
            Err(e) => SimulationResponse::Error(e.to_string()),
        }
    }

    /// Run Monte Carlo simulation synchronously.
    fn run_monte_carlo(
        &self,
        config: &SimulationConfig,
        iterations: usize,
        birth_date: &str,
        start_date: &str,
    ) -> SimulationResponse {
        *self.progress.borrow_mut() = 0;

        let mc_config = MonteCarloConfig {
            iterations,
            percentiles: vec![0.05, 0.50, 0.95],
            compute_mean: true,
        };

        // Run Monte Carlo (synchronously - will block UI)
        let mc_summary =
            match finplan_core::simulation::monte_carlo_simulate_with_config(config, &mc_config) {
                Ok(summary) => summary,
                Err(e) => return SimulationResponse::Error(e.to_string()),
            };

        *self.progress.borrow_mut() = iterations;

        // Convert percentile runs to TUI format
        let mut percentile_results = Vec::new();
        for (p, core_result) in &mc_summary.percentile_runs {
            match to_tui_result(core_result, birth_date, start_date) {
                Ok(tui_result) => {
                    percentile_results.push((*p, tui_result, core_result.clone()));
                }
                Err(e) => return SimulationResponse::Error(e.to_string()),
            }
        }

        // Build mean results from accumulators
        let (mean_tui_result, mean_core_result) =
            if let Some(mean_core) = mc_summary.get_mean_result() {
                match to_tui_result(&mean_core, birth_date, start_date) {
                    Ok(mean_tui) => (Some(mean_tui), Some(mean_core)),
                    Err(_) => (None, None),
                }
            } else {
                (None, None)
            };

        // Extract P50 as default result
        let (default_tui_result, default_core_result) = match percentile_results
            .iter()
            .find(|(p, _, _)| (*p - 0.50).abs() < 0.001)
        {
            Some((_, tui, core)) => (tui.clone(), core.clone()),
            None => return SimulationResponse::Error("Missing P50 result".to_string()),
        };

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

        let stored_result = MonteCarloStoredResult {
            stats: mc_summary.stats,
            percentile_results,
            mean_tui_result,
            mean_core_result,
        };

        SimulationResponse::MonteCarloComplete {
            stored_result: Box::new(stored_result),
            preview_summary,
            default_tui_result,
            default_core_result,
        }
    }

    /// Run batch Monte Carlo on multiple scenarios.
    fn run_batch(
        &self,
        scenarios: Vec<(String, SimulationConfig, String, String)>,
        iterations: usize,
    ) -> SimulationResponse {
        *self.batch_index.borrow_mut() = 0;
        *self.batch_total.borrow_mut() = scenarios.len();
        let mut completed_count = 0;

        for (idx, (scenario_name, config, birth_date, start_date)) in
            scenarios.into_iter().enumerate()
        {
            *self.batch_index.borrow_mut() = idx;
            *self.progress.borrow_mut() = 0;

            if *self.cancelled.borrow() {
                return SimulationResponse::Cancelled;
            }

            let mc_config = MonteCarloConfig {
                iterations,
                percentiles: vec![0.05, 0.50, 0.95],
                compute_mean: false,
            };

            let mc_summary = match finplan_core::simulation::monte_carlo_simulate_with_config(
                &config, &mc_config,
            ) {
                Ok(summary) => summary,
                Err(e) => {
                    tracing::warn!(scenario = scenario_name, error = %e, "Monte Carlo failed");
                    continue;
                }
            };

            // Extract summary data
            let p5 = mc_summary
                .stats
                .percentile_values
                .iter()
                .find(|(p, _)| (*p - 0.05).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);
            let p50 = mc_summary
                .stats
                .percentile_values
                .iter()
                .find(|(p, _)| (*p - 0.50).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);
            let p95 = mc_summary
                .stats
                .percentile_values
                .iter()
                .find(|(p, _)| (*p - 0.95).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);

            // Get P50 TUI result for yearly data and inflation calculations
            let p50_tui = mc_summary
                .percentile_runs
                .iter()
                .find(|(p, _)| (*p - 0.50).abs() < 0.001)
                .and_then(|(_, core_result)| {
                    to_tui_result(core_result, &birth_date, &start_date).ok()
                });

            let yearly_nw = p50_tui
                .as_ref()
                .map(|tui| tui.years.iter().map(|y| (y.year, y.net_worth)).collect());
            let yearly_real_nw = p50_tui.as_ref().map(|tui| {
                tui.years
                    .iter()
                    .map(|y| (y.year, y.real_net_worth))
                    .collect()
            });

            // Calculate real values using inflation factor from P50 TUI result
            let (final_real_nw, real_p5, real_p50, real_p95) = if let Some(ref tui) = p50_tui {
                let final_real = tui.final_real_net_worth;
                // Calculate inflation factor from P50: nominal / real
                let inflation_factor = if tui.final_real_net_worth > 0.0 {
                    tui.final_net_worth / tui.final_real_net_worth
                } else {
                    1.0
                };
                // Apply same factor to convert all percentiles to real terms
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

            // Queue the per-scenario completion response
            self.responses
                .borrow_mut()
                .push_back(SimulationResponse::BatchScenarioComplete {
                    scenario_name,
                    summary,
                });

            completed_count += 1;
        }

        SimulationResponse::BatchComplete { completed_count }
    }
}

impl Default for WebWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationWorker for WebWorker {
    fn send(&self, request: SimulationRequest) -> bool {
        // Clear state for new work
        *self.cancelled.borrow_mut() = false;
        *self.progress.borrow_mut() = 0;

        // Run synchronously and queue the response
        let response = match request {
            SimulationRequest::Single {
                config,
                seed,
                birth_date,
                start_date,
            } => self.run_single(&config, seed, &birth_date, &start_date),
            SimulationRequest::MonteCarlo {
                config,
                iterations,
                birth_date,
                start_date,
            } => self.run_monte_carlo(&config, iterations, &birth_date, &start_date),
            SimulationRequest::Batch {
                scenarios,
                iterations,
            } => self.run_batch(scenarios, iterations),
            SimulationRequest::Shutdown => {
                // No-op on web - nothing to shut down
                return true;
            }
        };

        self.responses.borrow_mut().push_back(response);
        true
    }

    fn try_recv(&self) -> Option<SimulationResponse> {
        self.responses.borrow_mut().pop_front()
    }

    fn get_progress(&self) -> usize {
        *self.progress.borrow()
    }

    fn get_batch_scenario_index(&self) -> usize {
        *self.batch_index.borrow()
    }

    fn get_batch_scenario_total(&self) -> usize {
        *self.batch_total.borrow()
    }

    fn cancel(&self) {
        *self.cancelled.borrow_mut() = true;
    }

    fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }

    fn shutdown(&self) {
        // No-op on web
    }
}
