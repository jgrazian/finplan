//! Spending targets and withdrawal strategies
//!
//! SpendingTargets represent required withdrawal amounts for retirement spending.
//! The simulation will pull from accounts to meet these targets using the
//! specified withdrawal strategy.

use super::cash_flows::RepeatInterval;
use super::ids::{AccountId, SpendingTargetId};
use serde::{Deserialize, Serialize};

/// Strategy for withdrawing funds from multiple accounts to meet a spending target
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum WithdrawalStrategy {
    /// Withdraw from accounts in the specified order until target is met.
    /// Skips Illiquid accounts automatically.
    Sequential { order: Vec<AccountId> },
    /// Withdraw proportionally from all liquid accounts based on their balances
    ProRata,
    /// Withdraw in tax-optimized order:
    /// 1. Taxable (only gains taxed at capital gains rate)
    /// 2. TaxDeferred (ordinary income)
    /// 3. TaxFree (no tax)
    #[default]
    TaxOptimized,
}

/// Current runtime state of a SpendingTarget
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum SpendingTargetState {
    /// Not yet started (created via config, waiting for activation)
    #[default]
    Pending,
    /// Actively generating withdrawal events
    Active,
    /// Temporarily paused (can be resumed)
    Paused,
    /// Permanently stopped
    Terminated,
}

/// A spending target represents a required withdrawal amount
///
/// The simulation will pull from accounts to meet this target according to
/// the specified withdrawal strategy, accounting for taxes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendingTarget {
    pub spending_target_id: SpendingTargetId,
    /// The target amount (gross or net depending on net_amount_mode)
    pub amount: f64,
    /// If true, `amount` is the after-tax target; system will gross up for taxes.
    /// If false, `amount` is the pre-tax withdrawal amount.
    #[serde(default)]
    pub net_amount_mode: bool,
    /// How often to withdraw
    pub repeats: RepeatInterval,
    /// Whether to adjust the target amount for inflation over time
    #[serde(default)]
    pub adjust_for_inflation: bool,
    /// Strategy for selecting which accounts to withdraw from
    #[serde(default)]
    pub withdrawal_strategy: WithdrawalStrategy,
    /// Accounts to exclude from withdrawals (in addition to Illiquid accounts)
    #[serde(default)]
    pub exclude_accounts: Vec<AccountId>,
    /// Initial state when loaded (runtime state tracked in SimulationState)
    #[serde(default)]
    pub state: SpendingTargetState,
}
