//! Grid search optimization with parallel evaluation
//!
//! Grid search exhaustively evaluates points on a regular grid across
//! the parameter space. It's simple and guaranteed to find the global
//! optimum within the grid resolution, but scales poorly with dimension.

use std::collections::HashMap;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::config::SimulationConfig;
use crate::error::MarketError;
use crate::model::MonteCarloConfig;
use crate::simulation::monte_carlo_simulate_with_config;

use super::config::OptimizationConfig;
use super::evaluator::{apply_parameters, calculate_objective, check_constraints};
use super::result::{ConvergenceHistory, EvaluationRecord, OptimizationResult, TerminationReason};

/// Generate all grid points for the parameter space
fn generate_grid_points(
    parameters: &[super::config::OptimizableParameter],
    grid_size: usize,
) -> Vec<Vec<f64>> {
    if parameters.is_empty() {
        return vec![vec![]];
    }

    let bounds: Vec<(f64, f64)> = parameters.iter().map(|p| p.bounds()).collect();
    let mut points = Vec::new();
    let mut indices = vec![0usize; parameters.len()];

    loop {
        // Generate point for current indices
        let point: Vec<f64> = indices
            .iter()
            .zip(bounds.iter())
            .map(|(&idx, &(min, max))| {
                if grid_size <= 1 {
                    (min + max) / 2.0
                } else {
                    min + (max - min) * (idx as f64) / (grid_size - 1) as f64
                }
            })
            .collect();
        points.push(point);

        // Increment indices (like counting in base grid_size)
        let mut carry = true;
        for index in indices.iter_mut() {
            if carry {
                *index += 1;
                if *index >= grid_size {
                    *index = 0;
                    // carry remains true
                } else {
                    carry = false;
                }
            }
        }

        // If we wrapped all the way around, we're done
        if carry {
            break;
        }
    }

    points
}

/// Perform grid search optimization using parallel evaluation
///
/// # Arguments
/// * `base_config` - The base simulation configuration
/// * `opt_config` - Optimization configuration
/// * `grid_size` - Number of points per dimension
pub fn optimize_grid_search(
    base_config: &SimulationConfig,
    opt_config: &OptimizationConfig,
    grid_size: usize,
) -> Result<OptimizationResult, MarketError> {
    let grid_points = generate_grid_points(&opt_config.parameters, grid_size);
    let total_points = grid_points.len();

    if total_points == 0 {
        return Err(MarketError::InvalidDistributionParameters {
            profile_type: "optimization",
            mean: 0.0,
            std_dev: 0.0,
            reason: "no parameters to optimize",
        });
    }

    // Monte Carlo config for each evaluation
    let mc_config = MonteCarloConfig {
        iterations: opt_config.monte_carlo_iterations,
        percentiles: vec![0.05, 0.50, 0.95],
        compute_mean: true,
    };

    // Parallel evaluation of all grid points
    #[cfg(feature = "parallel")]
    let results: Vec<Option<EvaluationRecord>> = grid_points
        .par_iter()
        .map(|values| {
            let config = apply_parameters(base_config, &opt_config.parameters, values)?;
            let summary = monte_carlo_simulate_with_config(&config, &mc_config).ok()?;
            let objective_value = calculate_objective(&opt_config.objective, &summary);
            let constraints_satisfied = check_constraints(&opt_config.constraints, &summary.stats);
            Some(EvaluationRecord {
                parameter_values: values.clone(),
                objective_value,
                constraints_satisfied,
                stats: summary.stats,
            })
        })
        .collect();

    #[cfg(not(feature = "parallel"))]
    let results: Vec<Option<EvaluationRecord>> = grid_points
        .iter()
        .map(|values| {
            let config = apply_parameters(base_config, &opt_config.parameters, values)?;
            let summary = monte_carlo_simulate_with_config(&config, &mc_config).ok()?;
            let objective_value = calculate_objective(&opt_config.objective, &summary);
            let constraints_satisfied = check_constraints(&opt_config.constraints, &summary.stats);
            Some(EvaluationRecord {
                parameter_values: values.clone(),
                objective_value,
                constraints_satisfied,
                stats: summary.stats,
            })
        })
        .collect();

    // Build history and find best result
    let mut history = ConvergenceHistory::new();
    let mut best: Option<EvaluationRecord> = None;

    for record in results.into_iter().flatten() {
        history.record(record.clone());

        if record.constraints_satisfied
            && (best.is_none() || record.objective_value > best.as_ref().unwrap().objective_value)
        {
            best = Some(record);
        }
    }

    // Build result
    match best {
        Some(record) => {
            let mut optimal_parameters = HashMap::new();
            for (param, value) in opt_config
                .parameters
                .iter()
                .zip(record.parameter_values.iter())
            {
                optimal_parameters.insert(param.name(), *value);
            }

            Ok(OptimizationResult {
                optimal_parameters,
                objective_value: record.objective_value,
                optimal_stats: record.stats,
                converged: true, // Grid search always "converges" (exhaustive)
                termination_reason: TerminationReason::Converged,
                iterations: total_points,
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

    #[test]
    fn test_generate_grid_points_1d() {
        use crate::model::EventId;
        use crate::optimization::config::OptimizableParameter;

        let params = vec![OptimizableParameter::RetirementAge {
            event_id: EventId(0),
            min_age: 60,
            max_age: 70,
        }];

        let points = generate_grid_points(&params, 3);
        assert_eq!(points.len(), 3);
        assert!((points[0][0] - 60.0).abs() < 0.001);
        assert!((points[1][0] - 65.0).abs() < 0.001);
        assert!((points[2][0] - 70.0).abs() < 0.001);
    }

    #[test]
    fn test_generate_grid_points_2d() {
        use crate::model::EventId;
        use crate::optimization::config::OptimizableParameter;

        let params = vec![
            OptimizableParameter::RetirementAge {
                event_id: EventId(0),
                min_age: 60,
                max_age: 70,
            },
            OptimizableParameter::WithdrawalAmount {
                event_id: EventId(1),
                min_amount: 0.0,
                max_amount: 100.0,
            },
        ];

        let points = generate_grid_points(&params, 2);
        assert_eq!(points.len(), 4); // 2^2 = 4
    }

    #[test]
    fn test_generate_grid_points_empty() {
        let params = vec![];
        let points = generate_grid_points(&params, 5);
        assert_eq!(points.len(), 1);
        assert!(points[0].is_empty());
    }
}
