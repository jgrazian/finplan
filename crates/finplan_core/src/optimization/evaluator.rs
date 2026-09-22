//! Parameter application and objective evaluation
//!
//! Provides functions to apply parameter values to a simulation configuration
//! and evaluate the objective function using Monte Carlo simulation.

use crate::config::SimulationConfig;
use crate::error::SimulationError;
use crate::model::{MonteCarloConfig, MonteCarloStats, MonteCarloSummary};
use crate::simulation::monte_carlo_simulate_with_config;

use super::config::{
    OptimizableParameter, OptimizationConfig, OptimizationConstraints, OptimizationObjective,
};
use super::result::EvaluationRecord;

/// Apply parameter values to a simulation configuration
///
/// Coordinates use `OptimizableParameter::bounds` units. Returns `None` for
/// missing or duplicate targets, invalid bounds, type mismatches, or out-of-range values.
#[must_use]
pub fn apply_parameters(
    base_config: &SimulationConfig,
    parameters: &[OptimizableParameter],
    values: &[f64],
) -> Option<SimulationConfig> {
    if parameters.len() != values.len() {
        return None;
    }

    super::config::validate_parameters(base_config, parameters).ok()?;
    let mut config = base_config.clone();
    for (parameter, value) in parameters.iter().zip(values) {
        config
            .parameters
            .insert(parameter.parameter_id, parameter.value_at(*value)?);
    }
    Some(config)
}

/// Evaluate parameters and return a full evaluation record
pub fn evaluate(
    base_config: &SimulationConfig,
    opt_config: &OptimizationConfig,
    values: &[f64],
) -> Result<EvaluationRecord, SimulationError> {
    // Apply parameters to get modified config
    let config = apply_parameters(base_config, &opt_config.parameters, values).ok_or(
        SimulationError::Config("failed to apply parameters to configuration".to_string()),
    )?;

    // Run Monte Carlo simulation
    let mc_config = MonteCarloConfig {
        iterations: opt_config.monte_carlo_iterations,
        percentiles: vec![0.05, 0.50, 0.95],
        compute_mean: true,
        ..Default::default()
    };

    let summary = monte_carlo_simulate_with_config(&config, &mc_config)?;

    // Calculate objective value
    let objective_value = calculate_objective(&opt_config.objective, &summary);

    // Check constraints
    let constraints_satisfied = check_constraints(&opt_config.constraints, &summary.stats);

    Ok(EvaluationRecord {
        parameter_values: opt_config
            .parameters
            .iter()
            .map(|p| config.parameters[&p.parameter_id])
            .collect(),
        objective_value,
        constraints_satisfied,
        stats: summary.stats,
    })
}

/// Calculate the objective function value from simulation results
#[must_use]
pub fn calculate_objective(objective: &OptimizationObjective, summary: &MonteCarloSummary) -> f64 {
    match objective {
        OptimizationObjective::MaximizeWealthAt { .. } => {
            // Use mean final net worth as proxy (would need date-specific tracking)
            summary.stats.mean_final_net_worth
        }
        OptimizationObjective::MaximizeWealthAtRetirement { .. } => {
            // Use mean final net worth (would need retirement date tracking)
            summary.stats.mean_final_net_worth
        }
        OptimizationObjective::MaximizeWealthAtDeath => summary.stats.mean_final_net_worth,
        OptimizationObjective::MaximizeSustainableWithdrawal {
            target_success_rate,
            ..
        } => {
            // For withdrawal optimization, we want high success rate
            // Return a penalty if below target, otherwise return the success rate
            if summary.stats.success_rate >= *target_success_rate {
                summary.stats.success_rate
            } else {
                // Large negative penalty for failing to meet target
                summary.stats.success_rate - 10.0
            }
        }
        OptimizationObjective::MinimizeLifetimeTax => {
            // Get total lifetime taxes from mean result
            let total_tax = summary.get_mean_result().map_or(0.0, |result| {
                result.yearly_taxes.iter().map(|t| t.total_tax).sum::<f64>()
            });
            // Negate since we want to minimize
            -total_tax
        }
    }
}

/// Check if all constraints are satisfied
#[must_use]
pub fn check_constraints(constraints: &OptimizationConstraints, stats: &MonteCarloStats) -> bool {
    // Check minimum success rate
    if let Some(min_rate) = constraints.min_success_rate
        && stats.success_rate < min_rate
    {
        return false;
    }

    // Check minimum final net worth
    if let Some(min_worth) = constraints.min_final_net_worth
        && stats.mean_final_net_worth < min_worth
    {
        return false;
    }
    // Note: max_withdrawal_rate would need additional context to check
    // (would need to know the withdrawal amount and portfolio value)

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_constraints_empty() {
        let constraints = OptimizationConstraints::default();
        let stats = MonteCarloStats {
            funding_success_rate: None,
            num_iterations: 100,
            success_rate: 0.95,
            mean_final_net_worth: 1_000_000.0,
            std_dev_final_net_worth: 100_000.0,
            min_final_net_worth: 500_000.0,
            max_final_net_worth: 1_500_000.0,
            percentile_values: vec![],
            converged: None,
            convergence_metric: None,
            convergence_value: None,
        };
        assert!(check_constraints(&constraints, &stats));
    }

    #[test]
    fn test_check_constraints_success_rate() {
        let constraints = OptimizationConstraints {
            min_success_rate: Some(0.90),
            ..Default::default()
        };
        let good_stats = MonteCarloStats {
            funding_success_rate: None,
            num_iterations: 100,
            success_rate: 0.95,
            mean_final_net_worth: 1_000_000.0,
            std_dev_final_net_worth: 100_000.0,
            min_final_net_worth: 500_000.0,
            max_final_net_worth: 1_500_000.0,
            percentile_values: vec![],
            converged: None,
            convergence_metric: None,
            convergence_value: None,
        };
        let bad_stats = MonteCarloStats {
            success_rate: 0.85,
            ..good_stats.clone()
        };

        assert!(check_constraints(&constraints, &good_stats));
        assert!(!check_constraints(&constraints, &bad_stats));
    }
}
