//! Binary search optimization for single-parameter problems
//!
//! Binary search is efficient for single-parameter optimization where the
//! objective function is monotonic with respect to the parameter (e.g.,
//! finding the maximum withdrawal that maintains a target success rate).

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::error::SimulationError;

use super::config::OptimizationConfig;
use super::evaluator::evaluate;
use super::result::{ConvergenceHistory, OptimizationResult, TerminationReason};

/// Progress callback for binary search optimization
///
/// Arguments: (iteration, `current_value`, `objective_value`)
pub type ProgressCallback = Box<dyn Fn(usize, f64, f64) + Send + Sync>;

/// Perform binary search optimization for a single parameter
///
/// This function assumes the objective function has a monotonic relationship
/// with the parameter value (common for withdrawal optimization).
///
/// # Arguments
/// * `base_config` - The base simulation configuration
/// * `opt_config` - Optimization configuration (must have exactly 1 parameter)
/// * `progress_callback` - Optional callback for progress updates
pub fn optimize_binary_search(
    base_config: &SimulationConfig,
    opt_config: &OptimizationConfig,
    progress_callback: Option<ProgressCallback>,
) -> Result<OptimizationResult, SimulationError> {
    // Verify we have exactly one parameter
    if opt_config.parameters.len() != 1 {
        return Err(SimulationError::Config(
            "binary search requires exactly one parameter".to_string(),
        ));
    }

    let param = &opt_config.parameters[0];
    let (min_val, max_val) = param.bounds();
    let param_name = param.name();

    let mut history = ConvergenceHistory::new();
    let mut low = min_val;
    let mut high = max_val;
    let mut best_feasible: Option<(f64, f64, super::result::EvaluationRecord)> = None;
    let mut iteration = 0;

    // Evaluate endpoints first
    let low_record = evaluate(base_config, opt_config, &[low])?;
    history.record(low_record.clone());
    if low_record.constraints_satisfied {
        best_feasible = Some((low, low_record.objective_value, low_record));
    }

    let high_record = evaluate(base_config, opt_config, &[high])?;
    history.record(high_record.clone());
    if high_record.constraints_satisfied
        && (best_feasible.is_none()
            || high_record.objective_value > best_feasible.as_ref().unwrap().1)
    {
        best_feasible = Some((high, high_record.objective_value, high_record));
    }

    // Binary search loop
    while iteration < opt_config.max_iterations && (high - low) > opt_config.tolerance * max_val {
        iteration += 1;
        let mid = f64::midpoint(low, high);

        let record = evaluate(base_config, opt_config, &[mid])?;
        history.record(record.clone());

        if let Some(ref callback) = progress_callback {
            callback(iteration, mid, record.objective_value);
        }

        // Update best feasible if this is better
        if record.constraints_satisfied
            && (best_feasible.is_none()
                || record.objective_value > best_feasible.as_ref().unwrap().1)
        {
            best_feasible = Some((mid, record.objective_value, record.clone()));
        }

        // Determine search direction based on constraint satisfaction
        // If mid violates constraints, search toward the feasible region
        // Otherwise, search toward higher objective values
        if record.constraints_satisfied {
            // Constraints satisfied - try higher values (for maximization)
            low = mid;
        } else {
            // Constraints violated - try lower values (more conservative)
            high = mid;
        }
    }

    // Build result
    match best_feasible {
        Some((optimal_val, objective_value, record)) => {
            let mut optimal_parameters = HashMap::new();
            optimal_parameters.insert(param_name, optimal_val);

            let converged = (high - low) <= opt_config.tolerance * max_val;

            Ok(OptimizationResult {
                optimal_parameters,
                objective_value,
                optimal_stats: record.stats,
                converged,
                termination_reason: if converged {
                    TerminationReason::Converged
                } else {
                    TerminationReason::MaxIterationsReached
                },
                iterations: iteration,
                total_simulations: history.num_evaluations() * opt_config.monte_carlo_iterations,
                history,
            })
        }
        None => Ok(OptimizationResult::no_feasible_solution(history)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EventId;
    use crate::optimization::config::{
        OptimizableParameter, OptimizationConstraints, OptimizationObjective,
    };

    #[test]
    fn test_binary_search_requires_single_param() {
        let config = SimulationConfig::default();
        let opt_config = OptimizationConfig {
            objective: OptimizationObjective::MaximizeWealthAtDeath,
            parameters: vec![
                OptimizableParameter::RetirementAge {
                    event_id: EventId(0),
                    min_age: 55,
                    max_age: 70,
                },
                OptimizableParameter::WithdrawalAmount {
                    event_id: EventId(1),
                    min_amount: 30000.0,
                    max_amount: 100000.0,
                },
            ],
            constraints: OptimizationConstraints::default(),
            ..Default::default()
        };

        let result = optimize_binary_search(&config, &opt_config, None);
        assert!(result.is_err());
    }
}
