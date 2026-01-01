//! Cash flow definitions
//!
//! Cash flows represent regular income or expenses that affect asset balances.

use crate::ids::{AccountId, AssetId, CashFlowId, EventId};
use jiff::ToSpan;
use serde::{Deserialize, Serialize};

/// How often a cash flow repeats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepeatInterval {
    Never,
    Weekly,
    BiWeekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl RepeatInterval {
    /// Convert to a jiff::Span for date arithmetic
    pub fn span(&self) -> jiff::Span {
        match self {
            RepeatInterval::Never => 0.days(),
            RepeatInterval::Weekly => 1.week(),
            RepeatInterval::BiWeekly => 2.weeks(),
            RepeatInterval::Monthly => 1.month(),
            RepeatInterval::Quarterly => 3.months(),
            RepeatInterval::Yearly => 1.year(),
        }
    }
}

/// Direction of a CashFlow - either income (money entering) or expense (money leaving)
///
/// Internal transfers between assets should use EventEffect::TransferAsset instead
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CashFlowDirection {
    /// Income: money flows from external source into an asset
    Income {
        target_account_id: AccountId,
        target_asset_id: AssetId,
    },
    /// Expense: money flows from an asset to external destination
    Expense {
        source_account_id: AccountId,
        source_asset_id: AssetId,
    },
}

/// How a limit resets
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LimitPeriod {
    /// Resets every calendar year
    Yearly,
    /// Never resets
    Lifetime,
}

/// Limits on a cash flow (e.g., IRS contribution limits)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlowLimits {
    pub limit: f64,
    pub limit_period: LimitPeriod,
}

/// Current runtime state of a CashFlow
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum CashFlowState {
    /// Not yet started (created via config, waiting for activation)
    #[default]
    Pending,
    /// Actively generating cash flow events
    Active,
    /// Temporarily paused (can be resumed)
    Paused,
    /// Permanently stopped
    Terminated,
}

/// A recurring or one-time money flow into or out of an asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlow {
    pub cash_flow_id: CashFlowId,
    pub amount: f64,
    pub repeats: RepeatInterval,
    pub cash_flow_limits: Option<CashFlowLimits>,
    pub adjust_for_inflation: bool,
    /// Direction of money flow (income or expense)
    /// For internal transfers, use Events with TransferAsset effect
    pub direction: CashFlowDirection,
    /// Initial state when loaded (runtime state tracked in SimulationState)
    #[serde(default)]
    pub state: CashFlowState,
}

impl CashFlow {
    /// Calculate annualized amount for income calculations
    pub fn annualized_amount(&self) -> f64 {
        match self.repeats {
            RepeatInterval::Never => self.amount,
            RepeatInterval::Weekly => self.amount * 52.0,
            RepeatInterval::BiWeekly => self.amount * 26.0,
            RepeatInterval::Monthly => self.amount * 12.0,
            RepeatInterval::Quarterly => self.amount * 4.0,
            RepeatInterval::Yearly => self.amount,
        }
    }
}

/// A specific fixed date or reference to a named event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Timepoint {
    Immediate,
    /// A specific fixed date (ad-hoc)
    Date(jiff::civil::Date),
    /// Reference to a named event in SimulationParameters
    Event(EventId),
    Never,
}
