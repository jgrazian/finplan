//! Grid search optimization with parallel evaluation
//!
//! Grid search exhaustively evaluates points on a regular grid across
//! the parameter space. It's simple and guaranteed to find the global
//! optimum within the grid resolution, but scales poorly with dimension.

use std::collections::HashMap;

use rayon::prelude::*;

use crate::config::SimulationConfig;
use crate::error::SimulationError;

use super::config::OptimizationConfig;
use super::evaluator::evaluate;
use super::result::{ConvergenceHistory, EvaluationRecord, OptimizationResult, TerminationReason};

/// Generate all grid points for the parameter space
fn generate_grid_points(
    parameters: &[super::config::OptimizableParameter],
    grid_size: usize,
) -> Vec<Vec<f64>> {
    if parameters.is_empty() {
        return vec![vec![]];
    }

    let axes: Vec<Vec<f64>> = parameters
        .iter()
        .map(|parameter| {
            let (min, max) = parameter.bounds();
            let mut axis: Vec<f64> = (0..grid_size.max(1))
                .map(|index| {
                    let coordinate = if grid_size <= 1 {
                        f64::midpoint(min, max)
                    } else {
                        (min + (max - min) * (index as f64 / (grid_size - 1) as f64))
                            .clamp(min, max)
                    };
                    if parameter.is_discrete() {
                        coordinate.round()
                    } else {
                        coordinate
                    }
                })
                .collect();
            axis.dedup();
            axis
        })
        .collect();
    let mut points = Vec::new();
    let mut indices = vec![0usize; parameters.len()];

    loop {
        points.push(
            indices
                .iter()
                .zip(&axes)
                .map(|(&index, axis)| axis[index])
                .collect(),
        );

        // Increment indices (like counting in base grid_size)
        let mut carry = true;
        for (index, axis) in indices.iter_mut().zip(&axes) {
            if carry {
                *index += 1;
                if *index >= axis.len() {
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
) -> Result<OptimizationResult, SimulationError> {
    super::config::validate_optimization(base_config, opt_config)?;
    if grid_size == 0 {
        return Err(SimulationError::Config("grid size must be positive".into()));
    }
    let grid_points = generate_grid_points(&opt_config.parameters, grid_size);
    let total_points = grid_points.len();

    // Propagate invalid candidates/simulation failures instead of silently
    // presenting configuration errors as an infeasible search.
    let results: Result<Vec<EvaluationRecord>, SimulationError> = grid_points
        .par_iter()
        .map(|values| evaluate(base_config, opt_config, values))
        .collect();

    // Build history and find best result
    let mut history = ConvergenceHistory::new();
    let mut best: Option<EvaluationRecord> = None;

    for record in results? {
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
                optimal_parameters.insert(param.parameter_id, *value);
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
        use crate::model::{ParameterId, ParameterValue};
        use crate::optimization::config::OptimizableParameter;

        let params = vec![OptimizableParameter {
            parameter_id: ParameterId(0),
            min_value: ParameterValue::Money(60.0),
            max_value: ParameterValue::Money(70.0),
        }];

        let points = generate_grid_points(&params, 3);
        assert_eq!(points.len(), 3);
        assert!((points[0][0] - 60.0).abs() < 0.001);
        assert!((points[1][0] - 65.0).abs() < 0.001);
        assert!((points[2][0] - 70.0).abs() < 0.001);
    }

    #[test]
    fn test_generate_grid_points_2d() {
        use crate::model::{ParameterId, ParameterValue};
        use crate::optimization::config::OptimizableParameter;

        let params = vec![
            OptimizableParameter {
                parameter_id: ParameterId(0),
                min_value: ParameterValue::Money(60.0),
                max_value: ParameterValue::Money(70.0),
            },
            OptimizableParameter {
                parameter_id: ParameterId(1),
                min_value: ParameterValue::Money(0.0),
                max_value: ParameterValue::Money(100.0),
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
