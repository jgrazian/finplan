//! Transaction records for history tracking
//!
//! These records capture every mutation to simulation state, allowing
//! full replay and analysis of what happened during a simulation.

use crate::ids::{AccountId, AssetId, CashFlowId, EventId, SpendingTargetId};
use serde::{Deserialize, Serialize};

/// Record of a CashFlow execution (income or expense only)
///
/// Internal transfers are recorded as TransferRecord instead
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlowRecord {
    pub date: jiff::civil::Date,
    pub cash_flow_id: CashFlowId,
    /// The account affected (target for income, source for expense)
    pub account_id: AccountId,
    /// The asset affected
    pub asset_id: AssetId,
    /// Positive for deposits (income), negative for withdrawals (expenses)
    pub amount: f64,
}

/// Record of investment return applied to an asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnRecord {
    pub date: jiff::civil::Date,
    pub account_id: AccountId,
    pub asset_id: AssetId,
    /// Balance before return was applied
    pub balance_before: f64,
    /// The return rate applied (can be negative for losses/debt interest)
    pub return_rate: f64,
    /// The dollar amount of return (balance_before * return_rate)
    pub return_amount: f64,
}

/// Record of a transfer between assets (triggered by EventEffect::TransferAsset)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRecord {
    pub date: jiff::civil::Date,
    pub from_account_id: AccountId,
    pub from_asset_id: AssetId,
    pub to_account_id: AccountId,
    pub to_asset_id: AssetId,
    /// Amount transferred (always positive)
    pub amount: f64,
}

/// Record of an event being triggered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub date: jiff::civil::Date,
    pub event_id: EventId,
}

/// Record of a single withdrawal event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalRecord {
    pub date: jiff::civil::Date,
    pub spending_target_id: SpendingTargetId,
    pub account_id: AccountId,
    pub asset_id: AssetId,
    pub gross_amount: f64,
    pub tax_amount: f64,
    pub net_amount: f64,
}

/// Record of a Required Minimum Distribution withdrawal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RmdRecord {
    pub date: jiff::civil::Date,
    pub account_id: AccountId,
    pub age: u8,
    pub prior_year_balance: f64,
    pub irs_divisor: f64,
    pub required_amount: f64,
    pub spending_target_id: SpendingTargetId,
}
