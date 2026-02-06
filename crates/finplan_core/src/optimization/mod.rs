//! Optimization module for financial planning scenarios
//!
//! This module provides optimization algorithms to find optimal values for
//! parameters like retirement age, contribution rates, and withdrawal amounts.
//!
//! # Example
//!
//! ```ignore
//! use finplan_core::optimization::{
//!     OptimizationConfig, OptimizationObjective, OptimizableParameter,
//!     OptimizationConstraints, optimize,
//! };
//!
//! let opt_config = OptimizationConfig {
//!     objective: OptimizationObjective::MaximizeSustainableWithdrawal {
//!         withdrawal_event_id: EventId(5),
//!         target_success_rate: 0.95,
//!     },
//!     parameters: vec![
//!         OptimizableParameter::WithdrawalAmount {
//!             event_id: EventId(5),
//!             min_amount: 30_000.0,
//!             max_amount: 150_000.0,
//!         },
//!     ],
//!     constraints: OptimizationConstraints {
//!         min_success_rate: Some(0.95),
//!         ..Default::default()
//!     },
//!     ..Default::default()
//! };
//!
//! let result = optimize(&simulation_config, &opt_config, None)?;
//! println!("Optimal withdrawal: ${:.0}", result.optimal_parameters["WithdrawalAmount(event_5)"]);
//! ```

mod binary_search;
mod config;
mod evaluator;
mod grid_search;
mod nelder_mead;
mod result;

// Re-export public types
pub use config::{
    OptimizableParameter, OptimizationAlgorithm, OptimizationConfig, OptimizationConstraints,
    OptimizationObjective,
};
pub use evaluator::{apply_parameters, calculate_objective, check_constraints, evaluate};
pub use result::{ConvergenceHistory, EvaluationRecord, OptimizationResult, TerminationReason};

// Re-export algorithm-specific functions
pub use binary_search::{ProgressCallback, optimize_binary_search};
pub use grid_search::optimize_grid_search;
pub use nelder_mead::optimize_nelder_mead;

use crate::config::SimulationConfig;
use crate::error::SimulationError;

/// Main optimization entry point
///
/// Automatically selects the best algorithm based on the configuration,
/// or uses the explicitly specified algorithm.
///
/// # Algorithm Selection (Auto mode)
/// - 1 parameter: Binary search (efficient for monotonic problems)
/// - 2-3 parameters: Grid search (exhaustive but feasible)
/// - 4+ parameters: Nelder-Mead (scales better with dimension)
///
/// # Arguments
/// * `base_config` - The base simulation configuration to optimize
/// * `opt_config` - The optimization configuration (objective, parameters, constraints)
/// * `progress_callback` - Optional callback for progress updates
///
/// # Returns
/// An `OptimizationResult` containing the optimal parameters and statistics
pub fn optimize(
    base_config: &SimulationConfig,
    opt_config: &OptimizationConfig,
    progress_callback: Option<ProgressCallback>,
) -> Result<OptimizationResult, SimulationError> {
    let num_params = opt_config.parameters.len();

    if num_params == 0 {
        return Err(SimulationError::Config(
            "no parameters to optimize".to_string(),
        ));
    }

    match &opt_config.algorithm {
        OptimizationAlgorithm::BinarySearch => {
            optimize_binary_search(base_config, opt_config, progress_callback)
        }
        OptimizationAlgorithm::GridSearch { grid_size } => {
            optimize_grid_search(base_config, opt_config, *grid_size)
        }
        OptimizationAlgorithm::NelderMead => {
            optimize_nelder_mead(base_config, opt_config, progress_callback)
        }
        OptimizationAlgorithm::Auto => {
            // Auto-select algorithm based on parameter count
            match num_params {
                1 => {
                    // For single-parameter, check if it's likely monotonic
                    // (withdrawal/contribution optimization tends to be monotonic)
                    optimize_binary_search(base_config, opt_config, progress_callback)
                }
                2..=3 => {
                    // For 2-3 parameters, grid search is still feasible
                    // Use grid_size based on max_iterations: sqrt(max_iterations)^n should be close to max_iterations
                    let grid_size =
                        (opt_config.max_iterations as f64).powf(1.0 / num_params as f64) as usize;
                    let grid_size = grid_size.clamp(3, 20); // Reasonable bounds
                    optimize_grid_search(base_config, opt_config, grid_size)
                }
                _ => {
                    // For many parameters, Nelder-Mead scales better
                    optimize_nelder_mead(base_config, opt_config, progress_callback)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EventId;

    #[test]
    fn test_optimize_no_params() {
        let config = SimulationConfig::default();
        let opt_config = OptimizationConfig {
            parameters: vec![],
            ..Default::default()
        };

        let result = optimize(&config, &opt_config, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_auto_select_binary_search() {
        // With 1 parameter and Auto, should select binary search
        let config = SimulationConfig::default();
        let opt_config = OptimizationConfig {
            objective: OptimizationObjective::MaximizeWealthAtDeath,
            parameters: vec![OptimizableParameter::RetirementAge {
                event_id: EventId(0),
                min_age: 60,
                max_age: 70,
            }],
            algorithm: OptimizationAlgorithm::Auto,
            monte_carlo_iterations: 10, // Low for testing
            max_iterations: 5,
            ..Default::default()
        };

        // This will fail due to no valid simulation config, but that's expected
        // The test is mainly to verify algorithm selection logic compiles
        let _result = optimize(&config, &opt_config, None);
    }
}
