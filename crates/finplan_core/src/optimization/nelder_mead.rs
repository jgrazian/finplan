//! Nelder-Mead simplex optimization
//!
//! The Nelder-Mead algorithm is a derivative-free optimization method that
//! works well for continuous, multi-parameter optimization problems. It
//! maintains a simplex of N+1 points in N-dimensional space and iteratively
//! transforms the simplex toward the optimum.

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::error::MarketError;

use super::config::OptimizationConfig;
use super::evaluator::evaluate;
use super::result::{ConvergenceHistory, EvaluationRecord, OptimizationResult, TerminationReason};

/// Progress callback for Nelder-Mead optimization
///
/// Arguments: (iteration, best_objective, current_simplex_size)
pub type ProgressCallback = Box<dyn Fn(usize, f64, f64) + Send + Sync>;

/// Standard Nelder-Mead coefficients
const REFLECTION_COEF: f64 = 1.0;
const EXPANSION_COEF: f64 = 2.0;
const CONTRACTION_COEF: f64 = 0.5;
const SHRINK_COEF: f64 = 0.5;

/// A point in parameter space with its evaluation
#[derive(Clone)]
struct SimplexVertex {
    values: Vec<f64>,
    objective: f64,
    constraints_satisfied: bool,
    record: EvaluationRecord,
}

/// Initialize the simplex with N+1 points
fn initialize_simplex(
    base_config: &SimulationConfig,
    opt_config: &OptimizationConfig,
) -> Result<Vec<SimplexVertex>, MarketError> {
    let n = opt_config.parameters.len();
    let bounds: Vec<(f64, f64)> = opt_config.parameters.iter().map(|p| p.bounds()).collect();

    let mut simplex = Vec::with_capacity(n + 1);

    // Start at the center of the parameter space
    let center: Vec<f64> = bounds.iter().map(|(min, max)| (min + max) / 2.0).collect();

    // Evaluate center point
    let center_record = evaluate(base_config, opt_config, &center)?;
    simplex.push(SimplexVertex {
        values: center.clone(),
        objective: penalized_objective(&center_record),
        constraints_satisfied: center_record.constraints_satisfied,
        record: center_record,
    });

    // Create n additional points by perturbing each dimension
    for i in 0..n {
        let mut point = center.clone();
        let (min, max) = bounds[i];
        let range = max - min;

        // Perturb this dimension by 10% of range (or move toward upper bound if at center)
        if point[i] + 0.1 * range <= max {
            point[i] += 0.1 * range;
        } else {
            point[i] -= 0.1 * range;
        }

        let record = evaluate(base_config, opt_config, &point)?;
        simplex.push(SimplexVertex {
            values: point,
            objective: penalized_objective(&record),
            constraints_satisfied: record.constraints_satisfied,
            record,
        });
    }

    Ok(simplex)
}

/// Penalized objective: returns a large negative value if constraints violated
fn penalized_objective(record: &EvaluationRecord) -> f64 {
    if record.constraints_satisfied {
        record.objective_value
    } else {
        // Large penalty that decreases with how close we are to satisfying constraints
        record.objective_value - 1e9
    }
}

/// Calculate the centroid of all points except the worst
fn centroid(simplex: &[SimplexVertex]) -> Vec<f64> {
    let n = simplex[0].values.len();
    let mut center = vec![0.0; n];

    // Exclude the last (worst) point
    for vertex in simplex.iter().take(simplex.len() - 1) {
        for (i, val) in vertex.values.iter().enumerate() {
            center[i] += val;
        }
    }

    let count = (simplex.len() - 1) as f64;
    for val in &mut center {
        *val /= count;
    }

    center
}

/// Reflect a point through the centroid
fn reflect(point: &[f64], centroid: &[f64], coef: f64) -> Vec<f64> {
    point
        .iter()
        .zip(centroid.iter())
        .map(|(p, c)| c + coef * (c - p))
        .collect()
}

/// Clamp values to bounds
fn clamp_to_bounds(values: &mut [f64], bounds: &[(f64, f64)]) {
    for (val, (min, max)) in values.iter_mut().zip(bounds.iter()) {
        *val = val.clamp(*min, *max);
    }
}

/// Calculate simplex size (max distance from centroid)
fn simplex_size(simplex: &[SimplexVertex], centroid: &[f64]) -> f64 {
    simplex
        .iter()
        .map(|v| {
            v.values
                .iter()
                .zip(centroid.iter())
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt()
        })
        .fold(0.0_f64, |a, b| a.max(b))
}

/// Perform Nelder-Mead simplex optimization
///
/// # Arguments
/// * `base_config` - The base simulation configuration
/// * `opt_config` - Optimization configuration
/// * `progress_callback` - Optional callback for progress updates
pub fn optimize_nelder_mead(
    base_config: &SimulationConfig,
    opt_config: &OptimizationConfig,
    progress_callback: Option<ProgressCallback>,
) -> Result<OptimizationResult, MarketError> {
    let n = opt_config.parameters.len();
    if n == 0 {
        return Err(MarketError::InvalidDistributionParameters {
            profile_type: "optimization",
            mean: 0.0,
            std_dev: 0.0,
            reason: "no parameters to optimize",
        });
    }

    let bounds: Vec<(f64, f64)> = opt_config.parameters.iter().map(|p| p.bounds()).collect();
    let mut history = ConvergenceHistory::new();

    // Initialize simplex
    let mut simplex = initialize_simplex(base_config, opt_config)?;

    // Record initial evaluations
    for vertex in &simplex {
        history.record(vertex.record.clone());
    }

    let mut iteration = 0;

    // Main optimization loop
    while iteration < opt_config.max_iterations {
        iteration += 1;

        // Sort simplex by objective (best first, worst last)
        simplex.sort_by(|a, b| {
            b.objective
                .partial_cmp(&a.objective)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let cent = centroid(&simplex);
        let size = simplex_size(&simplex, &cent);

        // Check convergence
        if size < opt_config.tolerance {
            break;
        }

        // Progress callback
        if let Some(ref callback) = progress_callback {
            callback(iteration, simplex[0].objective, size);
        }

        // Extract values we need before mutating simplex
        let best_objective = simplex[0].objective;
        let second_worst_objective = simplex[simplex.len() - 2].objective;
        let worst_objective = simplex[simplex.len() - 1].objective;
        let worst_values = simplex[simplex.len() - 1].values.clone();
        let worst_idx = simplex.len() - 1;

        // Try reflection
        let mut reflected = reflect(&worst_values, &cent, REFLECTION_COEF);
        clamp_to_bounds(&mut reflected, &bounds);
        let reflected_record = evaluate(base_config, opt_config, &reflected)?;
        let reflected_obj = penalized_objective(&reflected_record);
        history.record(reflected_record.clone());

        if reflected_obj > best_objective {
            // Reflected is best so far - try expansion
            let mut expanded = reflect(&worst_values, &cent, EXPANSION_COEF);
            clamp_to_bounds(&mut expanded, &bounds);
            let expanded_record = evaluate(base_config, opt_config, &expanded)?;
            let expanded_obj = penalized_objective(&expanded_record);
            history.record(expanded_record.clone());

            if expanded_obj > reflected_obj {
                // Accept expansion
                simplex[worst_idx] = SimplexVertex {
                    values: expanded,
                    objective: expanded_obj,
                    constraints_satisfied: expanded_record.constraints_satisfied,
                    record: expanded_record,
                };
            } else {
                // Accept reflection
                simplex[worst_idx] = SimplexVertex {
                    values: reflected,
                    objective: reflected_obj,
                    constraints_satisfied: reflected_record.constraints_satisfied,
                    record: reflected_record,
                };
            }
        } else if reflected_obj > second_worst_objective {
            // Reflected is better than second worst - accept it
            simplex[worst_idx] = SimplexVertex {
                values: reflected,
                objective: reflected_obj,
                constraints_satisfied: reflected_record.constraints_satisfied,
                record: reflected_record,
            };
        } else {
            // Try contraction
            let contract_point = if reflected_obj > worst_objective {
                &reflected
            } else {
                &worst_values
            };

            let mut contracted: Vec<f64> = cent
                .iter()
                .zip(contract_point.iter())
                .map(|(c, p)| c + CONTRACTION_COEF * (p - c))
                .collect();
            clamp_to_bounds(&mut contracted, &bounds);
            let contracted_record = evaluate(base_config, opt_config, &contracted)?;
            let contracted_obj = penalized_objective(&contracted_record);
            history.record(contracted_record.clone());

            if contracted_obj > worst_objective {
                // Accept contraction
                simplex[worst_idx] = SimplexVertex {
                    values: contracted,
                    objective: contracted_obj,
                    constraints_satisfied: contracted_record.constraints_satisfied,
                    record: contracted_record,
                };
            } else {
                // Shrink the simplex toward the best point
                let best_values = simplex[0].values.clone();
                for vertex in simplex.iter_mut().skip(1) {
                    let mut shrunk: Vec<f64> = best_values
                        .iter()
                        .zip(vertex.values.iter())
                        .map(|(b, v)| b + SHRINK_COEF * (v - b))
                        .collect();
                    clamp_to_bounds(&mut shrunk, &bounds);
                    let shrunk_record = evaluate(base_config, opt_config, &shrunk)?;
                    let shrunk_obj = penalized_objective(&shrunk_record);
                    history.record(shrunk_record.clone());

                    *vertex = SimplexVertex {
                        values: shrunk,
                        objective: shrunk_obj,
                        constraints_satisfied: shrunk_record.constraints_satisfied,
                        record: shrunk_record,
                    };
                }
            }
        }
    }

    // Final sort to get best
    simplex.sort_by(|a, b| {
        b.objective
            .partial_cmp(&a.objective)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Find best feasible solution
    let best_feasible = simplex
        .iter()
        .find(|v| v.constraints_satisfied)
        .or_else(|| history.best_evaluation().map(|_| &simplex[0]));

    match best_feasible {
        Some(vertex) => {
            let mut optimal_parameters = HashMap::new();
            for (param, value) in opt_config.parameters.iter().zip(vertex.values.iter()) {
                optimal_parameters.insert(param.name(), *value);
            }

            let cent = centroid(&simplex);
            let size = simplex_size(&simplex, &cent);
            let converged = size < opt_config.tolerance;

            Ok(OptimizationResult {
                optimal_parameters,
                objective_value: vertex.record.objective_value,
                optimal_stats: vertex.record.stats.clone(),
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

    #[test]
    fn test_reflect() {
        let point = vec![0.0, 0.0];
        let centroid = vec![1.0, 1.0];

        let reflected = reflect(&point, &centroid, 1.0);
        assert!((reflected[0] - 2.0).abs() < 0.001);
        assert!((reflected[1] - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_clamp_to_bounds() {
        let mut values = vec![-5.0, 15.0, 5.0];
        let bounds = vec![(0.0, 10.0), (0.0, 10.0), (0.0, 10.0)];

        clamp_to_bounds(&mut values, &bounds);

        assert!((values[0] - 0.0).abs() < 0.001);
        assert!((values[1] - 10.0).abs() < 0.001);
        assert!((values[2] - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_centroid() {
        use crate::model::MonteCarloStats;

        let dummy_record = EvaluationRecord {
            parameter_values: vec![],
            objective_value: 0.0,
            constraints_satisfied: true,
            stats: MonteCarloStats {
                num_iterations: 0,
                success_rate: 0.0,
                mean_final_net_worth: 0.0,
                std_dev_final_net_worth: 0.0,
                min_final_net_worth: 0.0,
                max_final_net_worth: 0.0,
                percentile_values: vec![],
                converged: None,
                relative_standard_error: None,
            },
        };

        let simplex = vec![
            SimplexVertex {
                values: vec![0.0, 0.0],
                objective: 0.0,
                constraints_satisfied: true,
                record: dummy_record.clone(),
            },
            SimplexVertex {
                values: vec![2.0, 0.0],
                objective: 0.0,
                constraints_satisfied: true,
                record: dummy_record.clone(),
            },
            SimplexVertex {
                values: vec![1.0, 2.0], // This is the worst (last), excluded
                objective: -1.0,
                constraints_satisfied: true,
                record: dummy_record,
            },
        ];

        let cent = centroid(&simplex);
        // Centroid of (0,0) and (2,0) = (1, 0)
        assert!((cent[0] - 1.0).abs() < 0.001);
        assert!((cent[1] - 0.0).abs() < 0.001);
    }
}
