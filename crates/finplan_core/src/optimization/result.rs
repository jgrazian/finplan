//! Optimization result types
//!
//! Contains types for tracking optimization progress and final results.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::MonteCarloStats;

/// A single evaluation during optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationRecord {
    /// The parameter values that were evaluated
    pub parameter_values: Vec<f64>,

    /// The objective function value
    pub objective_value: f64,

    /// Whether all constraints were satisfied
    pub constraints_satisfied: bool,

    /// Full Monte Carlo statistics from this evaluation
    pub stats: MonteCarloStats,
}

/// History of evaluations during optimization
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConvergenceHistory {
    /// All evaluations performed during optimization
    pub evaluations: Vec<EvaluationRecord>,

    /// Best objective value found at each iteration (monotonically improving)
    pub best_values: Vec<f64>,
}

impl ConvergenceHistory {
    /// Create a new empty convergence history
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new evaluation
    pub fn record(&mut self, record: EvaluationRecord) {
        let current_best = self
            .best_values
            .last()
            .copied()
            .unwrap_or(f64::NEG_INFINITY);

        // Only update best if constraints are satisfied and objective improved
        let new_best = if record.constraints_satisfied && record.objective_value > current_best {
            record.objective_value
        } else {
            current_best
        };

        self.best_values.push(new_best);
        self.evaluations.push(record);
    }

    /// Get the number of evaluations performed
    #[must_use]
    pub fn num_evaluations(&self) -> usize {
        self.evaluations.len()
    }

    /// Get the best evaluation (highest objective with satisfied constraints)
    #[must_use]
    pub fn best_evaluation(&self) -> Option<&EvaluationRecord> {
        self.evaluations
            .iter()
            .filter(|e| e.constraints_satisfied)
            .max_by(|a, b| {
                a.objective_value
                    .partial_cmp(&b.objective_value)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

/// Reason why optimization terminated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminationReason {
    /// Optimization converged to a solution
    Converged,

    /// Maximum iterations reached without convergence
    MaxIterationsReached,

    /// No feasible solution found (all evaluated points violated constraints)
    NoFeasibleSolution,

    /// User cancelled the optimization
    UserCancelled,
}

/// Final result from an optimization run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    /// The optimal parameter values found (parameter name -> value)
    pub optimal_parameters: HashMap<String, f64>,

    /// The objective function value at the optimal point
    pub objective_value: f64,

    /// Full Monte Carlo statistics at the optimal point
    pub optimal_stats: MonteCarloStats,

    /// Whether the optimization converged
    pub converged: bool,

    /// Why the optimization terminated
    pub termination_reason: TerminationReason,

    /// Number of iterations performed
    pub iterations: usize,

    /// Total number of simulations run (iterations * `monte_carlo_iterations`)
    pub total_simulations: usize,

    /// Full convergence history
    pub history: ConvergenceHistory,
}

impl OptimizationResult {
    /// Create a result for when no feasible solution was found
    #[must_use]
    pub fn no_feasible_solution(history: ConvergenceHistory) -> Self {
        Self {
            optimal_parameters: HashMap::new(),
            objective_value: f64::NEG_INFINITY,
            optimal_stats: MonteCarloStats {
                num_iterations: 0,
                success_rate: 0.0,
                mean_final_net_worth: 0.0,
                std_dev_final_net_worth: 0.0,
                min_final_net_worth: 0.0,
                max_final_net_worth: 0.0,
                percentile_values: Vec::new(),
                converged: None,
                convergence_metric: None,
                convergence_value: None,
            },
            converged: false,
            termination_reason: TerminationReason::NoFeasibleSolution,
            iterations: history.num_evaluations(),
            total_simulations: 0,
            history,
        }
    }
}
