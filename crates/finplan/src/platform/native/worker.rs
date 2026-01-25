//! Native worker implementation using std::thread.

use crate::platform::worker::{SimulationRequest, SimulationResponse, SimulationWorker};
use crate::worker::SimulationWorker as InternalWorker;

/// Native worker implementation that wraps the internal SimulationWorker.
pub struct NativeWorker {
    inner: InternalWorker,
}

impl NativeWorker {
    /// Create a new native worker with a background thread.
    pub fn new() -> Self {
        Self {
            inner: InternalWorker::new(),
        }
    }
}

impl Default for NativeWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationWorker for NativeWorker {
    fn send(&self, request: SimulationRequest) -> bool {
        // Convert platform request to internal request
        let internal_request = match request {
            SimulationRequest::Single {
                config,
                seed,
                birth_date,
                start_date,
            } => crate::worker::SimulationRequest::Single {
                config,
                seed,
                birth_date,
                start_date,
            },
            SimulationRequest::MonteCarlo {
                config,
                iterations,
                birth_date,
                start_date,
            } => crate::worker::SimulationRequest::MonteCarlo {
                config,
                iterations,
                birth_date,
                start_date,
            },
            SimulationRequest::Batch {
                scenarios,
                iterations,
            } => crate::worker::SimulationRequest::Batch {
                scenarios,
                iterations,
            },
            SimulationRequest::Shutdown => crate::worker::SimulationRequest::Shutdown,
        };
        self.inner.send(internal_request)
    }

    fn try_recv(&self) -> Option<SimulationResponse> {
        // Convert internal response to platform response
        self.inner.try_recv().map(|resp| match resp {
            crate::worker::SimulationResponse::Progress { current, total } => {
                SimulationResponse::Progress { current, total }
            }
            crate::worker::SimulationResponse::SingleComplete {
                tui_result,
                core_result,
            } => SimulationResponse::SingleComplete {
                tui_result,
                core_result,
            },
            crate::worker::SimulationResponse::MonteCarloComplete {
                stored_result,
                preview_summary,
                default_tui_result,
                default_core_result,
            } => SimulationResponse::MonteCarloComplete {
                stored_result,
                preview_summary,
                default_tui_result,
                default_core_result,
            },
            crate::worker::SimulationResponse::BatchScenarioComplete {
                scenario_name,
                summary,
            } => SimulationResponse::BatchScenarioComplete {
                scenario_name,
                summary,
            },
            crate::worker::SimulationResponse::BatchComplete { completed_count } => {
                SimulationResponse::BatchComplete { completed_count }
            }
            crate::worker::SimulationResponse::Cancelled => SimulationResponse::Cancelled,
            crate::worker::SimulationResponse::Error(e) => SimulationResponse::Error(e),
        })
    }

    fn get_progress(&self) -> usize {
        self.inner.get_progress()
    }

    fn get_batch_scenario_index(&self) -> usize {
        self.inner.get_batch_scenario_index()
    }

    fn get_batch_scenario_total(&self) -> usize {
        self.inner.get_batch_scenario_total()
    }

    fn cancel(&self) {
        self.inner.cancel()
    }

    fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    fn shutdown(&self) {
        self.inner.shutdown()
    }
}
