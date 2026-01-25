//! Background worker abstraction for platform-independent simulation execution.
//!
//! This module defines the [`SimulationWorker`] trait and associated types
//! that abstract background simulation execution for both native (std::thread)
//! and web (Web Worker) platforms.

use finplan_core::config::SimulationConfig;
use finplan_core::model::SimulationResult as CoreResult;

use crate::state::{
    MonteCarloPreviewSummary, MonteCarloStoredResult, ScenarioSummary, SimulationResult,
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
        birth_date: String,
        start_date: String,
    },
    /// Run Monte Carlo on multiple scenarios (batch mode)
    Batch {
        /// Vec of (scenario_name, config, birth_date, start_date)
        scenarios: Vec<(String, SimulationConfig, String, String)>,
        iterations: usize,
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
}

/// Platform-independent simulation worker interface.
///
/// This trait abstracts background simulation execution so it works on both
/// native (using std::thread) and web (using Web Workers) platforms.
pub trait SimulationWorker {
    /// Send a simulation request to the worker
    ///
    /// Returns true if the request was sent successfully
    fn send(&self, request: SimulationRequest) -> bool;

    /// Try to receive a response (non-blocking)
    fn try_recv(&self) -> Option<SimulationResponse>;

    /// Get current progress for Monte Carlo (0..total iterations)
    fn get_progress(&self) -> usize;

    /// Get current batch scenario index (0..total scenarios)
    fn get_batch_scenario_index(&self) -> usize;

    /// Get total number of scenarios in batch
    fn get_batch_scenario_total(&self) -> usize;

    /// Request cancellation of the current operation
    fn cancel(&self);

    /// Check if cancellation was requested
    fn is_cancelled(&self) -> bool;

    /// Shutdown the worker
    fn shutdown(&self);
}
