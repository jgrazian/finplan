//! Optimization configuration types
//!
//! Defines the objectives, parameters, constraints, and algorithms available
//! for optimization of financial planning scenarios.

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::model::{AccountId, EventId};

/// What the optimization is trying to achieve
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptimizationObjective {
    /// Maximize total wealth at a specific date
    MaximizeWealthAt { date: Date },

    /// Maximize total wealth when a retirement event triggers
    MaximizeWealthAtRetirement { retirement_event_id: EventId },

    /// Maximize total wealth at the end of the simulation (death/end date)
    MaximizeWealthAtDeath,

    /// Find the maximum sustainable withdrawal that maintains a target success rate
    MaximizeSustainableWithdrawal {
        withdrawal_event_id: EventId,
        target_success_rate: f64,
    },

    /// Minimize total lifetime tax burden
    MinimizeLifetimeTax,
}

/// A parameter that can be optimized
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptimizableParameter {
    /// Optimize the retirement age (modifies an Age trigger on an event)
    RetirementAge {
        event_id: EventId,
        min_age: u8,
        max_age: u8,
    },

    /// Optimize a contribution rate (modifies TransferAmount::Fixed in event effects)
    ContributionRate {
        event_id: EventId,
        min_amount: f64,
        max_amount: f64,
    },

    /// Optimize a withdrawal amount (modifies TransferAmount::Fixed in event effects)
    WithdrawalAmount {
        event_id: EventId,
        min_amount: f64,
        max_amount: f64,
    },

    /// Optimize asset allocation (stock vs bond percentage)
    AssetAllocation {
        account_id: AccountId,
        min_stock_pct: f64,
        max_stock_pct: f64,
    },
}

impl OptimizableParameter {
    /// Returns the (min, max) bounds for this parameter
    pub fn bounds(&self) -> (f64, f64) {
        match self {
            OptimizableParameter::RetirementAge {
                min_age, max_age, ..
            } => (*min_age as f64, *max_age as f64),
            OptimizableParameter::ContributionRate {
                min_amount,
                max_amount,
                ..
            } => (*min_amount, *max_amount),
            OptimizableParameter::WithdrawalAmount {
                min_amount,
                max_amount,
                ..
            } => (*min_amount, *max_amount),
            OptimizableParameter::AssetAllocation {
                min_stock_pct,
                max_stock_pct,
                ..
            } => (*min_stock_pct, *max_stock_pct),
        }
    }

    /// Returns a display name for this parameter
    pub fn name(&self) -> String {
        match self {
            OptimizableParameter::RetirementAge { event_id, .. } => {
                format!("RetirementAge(event_{})", event_id.0)
            }
            OptimizableParameter::ContributionRate { event_id, .. } => {
                format!("ContributionRate(event_{})", event_id.0)
            }
            OptimizableParameter::WithdrawalAmount { event_id, .. } => {
                format!("WithdrawalAmount(event_{})", event_id.0)
            }
            OptimizableParameter::AssetAllocation { account_id, .. } => {
                format!("AssetAllocation(account_{})", account_id.0)
            }
        }
    }
}

/// Constraints that must be satisfied for a solution to be feasible
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OptimizationConstraints {
    /// Minimum acceptable success rate (e.g., 0.95 for 95%)
    pub min_success_rate: Option<f64>,

    /// Minimum acceptable final net worth
    pub min_final_net_worth: Option<f64>,

    /// Maximum withdrawal rate as a percentage of portfolio
    pub max_withdrawal_rate: Option<f64>,
}

/// Algorithm to use for optimization
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum OptimizationAlgorithm {
    /// Binary search - efficient for single-parameter optimization
    BinarySearch,

    /// Grid search - exhaustive search over parameter space
    GridSearch { grid_size: usize },

    /// Nelder-Mead simplex - good for multi-parameter continuous optimization
    NelderMead,

    /// Automatically select the best algorithm based on parameter count
    #[default]
    Auto,
}

/// Complete configuration for an optimization run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationConfig {
    /// What we're trying to optimize
    pub objective: OptimizationObjective,

    /// Parameters to optimize
    pub parameters: Vec<OptimizableParameter>,

    /// Constraints that must be satisfied
    pub constraints: OptimizationConstraints,

    /// Algorithm to use
    pub algorithm: OptimizationAlgorithm,

    /// Number of Monte Carlo iterations for each evaluation
    #[serde(default = "default_monte_carlo_iterations")]
    pub monte_carlo_iterations: usize,

    /// Maximum optimization iterations
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,

    /// Convergence tolerance (relative improvement threshold)
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
}

fn default_monte_carlo_iterations() -> usize {
    500
}

fn default_max_iterations() -> usize {
    100
}

fn default_tolerance() -> f64 {
    0.001
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            objective: OptimizationObjective::MaximizeWealthAtDeath,
            parameters: Vec::new(),
            constraints: OptimizationConstraints::default(),
            algorithm: OptimizationAlgorithm::Auto,
            monte_carlo_iterations: default_monte_carlo_iterations(),
            max_iterations: default_max_iterations(),
            tolerance: default_tolerance(),
        }
    }
}
