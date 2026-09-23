//! Optimization configuration types
//!
//! Defines the objectives, parameters, constraints, and algorithms available
//! for optimization of financial planning scenarios.

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::model::{CalendarAge, EventId, ParameterId, ParameterValue};

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

/// A registry parameter to optimize, with inclusive bounds of the same type.
///
/// Money and Rate are continuous. Date is searched in whole days, and Age in
/// whole months. Both bounds must match the configured parameter's type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptimizableParameter {
    pub parameter_id: ParameterId,
    pub min_value: ParameterValue,
    pub max_value: ParameterValue,
}

impl OptimizableParameter {
    /// Internal search coordinates: numeric values, days since the lower Date
    /// bound, or total months of Age. Invalid bound types produce NaN bounds.
    #[must_use]
    pub fn bounds(&self) -> (f64, f64) {
        match (self.min_value, self.max_value) {
            (ParameterValue::Money(min), ParameterValue::Money(max))
            | (ParameterValue::Rate(min), ParameterValue::Rate(max)) => (min, max),
            (ParameterValue::Date(min), ParameterValue::Date(max)) => {
                (0.0, f64::from((max - min).get_days()))
            }
            (ParameterValue::Age(min), ParameterValue::Age(max)) => {
                (age_months(min), age_months(max))
            }
            _ => (f64::NAN, f64::NAN),
        }
    }

    #[must_use]
    pub fn is_discrete(&self) -> bool {
        matches!(
            self.min_value,
            ParameterValue::Date(_) | ParameterValue::Age(_)
        )
    }

    /// Decode a search coordinate, rounding calendar values to the nearest day
    /// or month. Reject invalid bounds and candidates outside the search range.
    #[must_use]
    pub fn value_at(&self, coordinate: f64) -> Option<ParameterValue> {
        let (min, max) = self.bounds();
        if !self.min_value.is_valid()
            || !self.max_value.is_valid()
            || !min.is_finite()
            || !max.is_finite()
            || min > max
            || !coordinate.is_finite()
            || coordinate < min
            || coordinate > max
        {
            return None;
        }
        Some(match self.min_value {
            ParameterValue::Money(_) => ParameterValue::Money(coordinate),
            ParameterValue::Rate(_) => ParameterValue::Rate(coordinate),
            ParameterValue::Date(date) => ParameterValue::Date(
                date.checked_add(jiff::Span::new().days(coordinate.round() as i64))
                    .ok()?,
            ),
            ParameterValue::Age(_) => {
                let months = coordinate.round() as u16;
                ParameterValue::Age(CalendarAge::new((months / 12) as u8, (months % 12) as u8))
            }
        })
    }
}

fn age_months(age: CalendarAge) -> f64 {
    f64::from(age.years) * 12.0 + f64::from(age.months)
}

/// Check targets before any algorithm starts or candidate is applied.
pub(super) fn validate_parameters(
    config: &crate::config::SimulationConfig,
    parameters: &[OptimizableParameter],
) -> Result<(), crate::error::SimulationError> {
    use crate::error::SimulationError;
    let mut seen = std::collections::HashSet::new();
    for parameter in parameters {
        let invalid = |reason| {
            SimulationError::Config(format!("parameter {}: {reason}", parameter.parameter_id.0))
        };
        if !seen.insert(parameter.parameter_id) {
            return Err(invalid("duplicate optimization target"));
        }
        let current = config
            .parameters
            .get(&parameter.parameter_id)
            .ok_or_else(|| invalid("optimization target does not exist"))?;
        if !current.is_valid()
            || std::mem::discriminant(current) != std::mem::discriminant(&parameter.min_value)
        {
            return Err(invalid("optimization bounds must match the parameter type"));
        }
        let (min, max) = parameter.bounds();
        if parameter.value_at(min).is_none()
            || parameter.value_at(max).is_none()
            || !(max - min).is_finite()
        {
            return Err(invalid("invalid optimization bounds"));
        }
    }
    Ok(())
}

pub(super) fn validate_optimization(
    config: &crate::config::SimulationConfig,
    optimization: &OptimizationConfig,
) -> Result<(), crate::error::SimulationError> {
    if optimization.parameters.is_empty() {
        return Err(crate::error::SimulationError::Config(
            "no parameters to optimize".into(),
        ));
    }
    if !optimization.tolerance.is_finite() || optimization.tolerance <= 0.0 {
        return Err(crate::error::SimulationError::Config(
            "optimization tolerance must be positive and finite".into(),
        ));
    }
    validate_parameters(config, &optimization.parameters)
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

    /// Grid search - sample each dimension, rounding/deduplicating calendar values
    GridSearch { grid_size: usize },

    /// Nelder-Mead simplex - good for multi-parameter continuous optimization
    NelderMead,

    /// Grid search for Date/Age targets; otherwise select by parameter count
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
