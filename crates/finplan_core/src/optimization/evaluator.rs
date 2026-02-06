//! Parameter application and objective evaluation
//!
//! Provides functions to apply parameter values to a simulation configuration
//! and evaluate the objective function using Monte Carlo simulation.

use crate::config::SimulationConfig;
use crate::error::MarketError;
use crate::model::{
    EventEffect, MonteCarloConfig, MonteCarloStats, MonteCarloSummary, TransferAmount,
};
use crate::simulation::monte_carlo_simulate_with_config;

use super::config::{
    OptimizableParameter, OptimizationConfig, OptimizationConstraints, OptimizationObjective,
};
use super::result::EvaluationRecord;

/// Apply parameter values to a simulation configuration
///
/// Returns `None` if the configuration cannot be modified (e.g., event not found)
#[must_use]
pub fn apply_parameters(
    base_config: &SimulationConfig,
    parameters: &[OptimizableParameter],
    values: &[f64],
) -> Option<SimulationConfig> {
    if parameters.len() != values.len() {
        return None;
    }

    let mut config = base_config.clone();

    for (param, value) in parameters.iter().zip(values.iter()) {
        match param {
            OptimizableParameter::RetirementAge { event_id, .. } => {
                // Use the existing with_retirement_age helper
                config = config.with_retirement_age(*event_id, *value as u8)?;
            }
            OptimizableParameter::ContributionRate { event_id, .. } => {
                // Find the event and modify TransferAmount::Fixed in its effects
                let event = config.events.iter_mut().find(|e| e.event_id == *event_id)?;
                modify_fixed_amount_in_effects(&mut event.effects, *value);
            }
            OptimizableParameter::WithdrawalAmount { event_id, .. } => {
                // Find the event and modify TransferAmount::Fixed in its effects
                let event = config.events.iter_mut().find(|e| e.event_id == *event_id)?;
                modify_fixed_amount_in_effects(&mut event.effects, *value);
            }
            OptimizableParameter::AssetAllocation { account_id, .. } => {
                // Find the account and adjust asset allocation
                // The stock_pct value represents what percentage of the portfolio
                // should be in the first asset (stocks) vs cash
                let stock_pct = *value;

                let account = config
                    .accounts
                    .iter_mut()
                    .find(|a| a.account_id == *account_id)?;

                if let crate::model::AccountFlavor::Investment(ref mut inv) = account.flavor {
                    // Calculate total value (positions + cash)
                    // Use cost_basis as proxy for value since we don't have market data here
                    let total_positions_value: f64 =
                        inv.positions.iter().map(|p| p.cost_basis).sum();
                    let total_value = total_positions_value + inv.cash.value;

                    if total_value > 0.0 && !inv.positions.is_empty() {
                        // Target: stock_pct in positions, (1-stock_pct) in cash
                        let target_stock_value = total_value * stock_pct;
                        let target_cash_value = total_value * (1.0 - stock_pct);

                        // Scale all positions proportionally to hit target stock value
                        if total_positions_value > 0.0 {
                            let scale_factor = target_stock_value / total_positions_value;
                            for position in &mut inv.positions {
                                position.units *= scale_factor;
                                position.cost_basis *= scale_factor;
                            }
                        }

                        // Set cash to target cash value
                        inv.cash.value = target_cash_value;
                    }
                }
            }
        }
    }

    Some(config)
}

/// Modify `TransferAmount::Fixed` values in event effects
fn modify_fixed_amount_in_effects(effects: &mut [EventEffect], new_amount: f64) {
    for effect in effects {
        match effect {
            EventEffect::Income { amount, .. }
            | EventEffect::Expense { amount, .. }
            | EventEffect::AssetPurchase { amount, .. }
            | EventEffect::AssetSale { amount, .. }
            | EventEffect::Sweep { amount, .. }
            | EventEffect::AdjustBalance { amount, .. }
            | EventEffect::CashTransfer { amount, .. } => {
                modify_transfer_amount(amount, new_amount);
            }
            _ => {}
        }
    }
}

/// Recursively modify `TransferAmount::Fixed` values
fn modify_transfer_amount(amount: &mut TransferAmount, new_amount: f64) {
    match amount {
        TransferAmount::Fixed(val) => {
            *val = new_amount;
        }
        TransferAmount::Min(a, b)
        | TransferAmount::Max(a, b)
        | TransferAmount::Sub(a, b)
        | TransferAmount::Add(a, b)
        | TransferAmount::Mul(a, b) => {
            // Only modify the first Fixed we find in compound amounts
            if matches!(**a, TransferAmount::Fixed(_)) {
                modify_transfer_amount(a, new_amount);
            } else if matches!(**b, TransferAmount::Fixed(_)) {
                modify_transfer_amount(b, new_amount);
            }
        }
        _ => {}
    }
}

/// Evaluate parameters and return a full evaluation record
pub fn evaluate(
    base_config: &SimulationConfig,
    opt_config: &OptimizationConfig,
    values: &[f64],
) -> Result<EvaluationRecord, MarketError> {
    // Apply parameters to get modified config
    let config = apply_parameters(base_config, &opt_config.parameters, values).ok_or({
        MarketError::InvalidDistributionParameters {
            profile_type: "optimization",
            mean: 0.0,
            std_dev: 0.0,
            reason: "failed to apply parameters to configuration",
        }
    })?;

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
        parameter_values: values.to_vec(),
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
