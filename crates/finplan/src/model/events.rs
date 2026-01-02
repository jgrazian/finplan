//! Event system - triggers and effects
//!
//! Events are the mechanism for changing simulation state over time.
//! Each event has a trigger condition and a list of effects to apply when triggered.

use super::accounts::Account;
use super::cash_flows::{CashFlow, RepeatInterval};
use super::ids::{AccountId, AssetId, CashFlowId, EventId, SpendingTargetId};
use super::spending::SpendingTarget;
use serde::{Deserialize, Serialize};

/// Time offset relative to another event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TriggerOffset {
    Days(i32),
    Months(i32),
    Years(i32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BalanceThreshold {
    GreaterThanOrEqual(f64),
    LessThanOrEqual(f64),
}

impl BalanceThreshold {
    pub fn value(&self) -> f64 {
        match self {
            BalanceThreshold::GreaterThanOrEqual(v) => *v,
            BalanceThreshold::LessThanOrEqual(v) => *v,
        }
    }

    pub fn evaluate(&self, balance: f64) -> bool {
        match self {
            BalanceThreshold::GreaterThanOrEqual(v) => balance >= *v,
            BalanceThreshold::LessThanOrEqual(v) => balance <= *v,
        }
    }
}

/// Conditions that can trigger an event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventTrigger {
    // === Time-Based Triggers ===
    /// Trigger on a specific date
    Date(jiff::civil::Date),

    /// Trigger at a specific age (requires birth_date in SimulationParameters)
    Age { years: u8, months: Option<u8> },

    /// Trigger N days/months/years after another event
    RelativeToEvent {
        event_id: EventId,
        offset: TriggerOffset,
    },

    // === Balance-Based Triggers ===
    /// Trigger when total account balance crosses threshold
    AccountBalance {
        account_id: AccountId,
        threshold: BalanceThreshold,
    },

    /// Trigger when a specific asset balance crosses threshold
    AssetBalance {
        account_id: AccountId,
        asset_id: AssetId,
        threshold: BalanceThreshold,
    },

    /// Trigger when total net worth crosses threshold
    NetWorth { threshold: BalanceThreshold },

    /// Trigger when an account is depleted (balance <= 0)
    AccountDepleted(AccountId),

    // === CashFlow-Based Triggers ===
    /// Trigger when a cash flow is terminated
    CashFlowEnded(CashFlowId),

    /// Trigger when total income (from External sources) drops below threshold
    TotalIncomeBelow(f64),

    // === Compound Triggers ===
    /// All conditions must be true
    And(Vec<EventTrigger>),

    /// Any condition can be true
    Or(Vec<EventTrigger>),

    // === Scheduled/Repeating Triggers ===
    /// Trigger on a repeating schedule (like a cron job)
    /// Useful for recurring transfers, rebalancing, etc.
    Repeating {
        interval: RepeatInterval,
        /// Optional: only start repeating after this condition is met
        #[serde(default)]
        start_condition: Option<Box<EventTrigger>>,
    },

    // === Manual/Simulation Control ===
    /// Never triggers automatically; can only be triggered by TriggerEvent effect
    Manual,
}

/// Actions that can occur when an event triggers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventEffect {
    // === Account Effects ===
    CreateAccount(Account),
    DeleteAccount(AccountId),

    // === CashFlow Effects ===
    CreateCashFlow(Box<CashFlow>),
    ActivateCashFlow(CashFlowId),
    PauseCashFlow(CashFlowId),
    ResumeCashFlow(CashFlowId),
    TerminateCashFlow(CashFlowId),
    ModifyCashFlow {
        cash_flow_id: CashFlowId,
        new_amount: Option<f64>,
        new_repeats: Option<RepeatInterval>,
    },

    // === SpendingTarget Effects ===
    CreateSpendingTarget(Box<SpendingTarget>),
    ActivateSpendingTarget(SpendingTargetId),
    PauseSpendingTarget(SpendingTargetId),
    ResumeSpendingTarget(SpendingTargetId),
    TerminateSpendingTarget(SpendingTargetId),
    ModifySpendingTarget {
        spending_target_id: SpendingTargetId,
        new_amount: Option<f64>,
    },

    // === Asset Effects ===
    TransferAsset {
        from_account: AccountId,
        to_account: AccountId,
        from_asset_id: AssetId,
        to_asset_id: AssetId,
        /// None = transfer entire balance
        amount: Option<f64>,
    },

    // === Event Chaining ===
    /// Trigger another event (for chaining effects)
    TriggerEvent(EventId),

    // === RMD Effects ===
    /// Create automatic Required Minimum Distribution withdrawal from tax-deferred account
    CreateRmdWithdrawal {
        account_id: AccountId,
        starting_age: u8,
    },

    // === Cash Management Effects ===
    /// Sweep funds to maintain minimum balance in target account
    /// Liquidates assets from funding sources (in order) to replenish target
    SweepToAccount {
        /// Account to keep funded (e.g., cash/checking account)
        target_account_id: AccountId,
        target_asset_id: AssetId,
        /// Balance to replenish to when triggered
        target_balance: f64,
        /// Accounts/assets to liquidate from (in priority order)
        /// Will sell from first source until exhausted, then move to next
        funding_sources: Vec<(AccountId, AssetId)>,
    },
}

/// An event with a trigger condition and effects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: EventId,
    pub trigger: EventTrigger,
    /// Effects to apply when this event triggers (executed in order)
    #[serde(default)]
    pub effects: Vec<EventEffect>,
    /// If true, this event can only trigger once
    #[serde(default)]
    pub once: bool,
}
