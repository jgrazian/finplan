//! Event system - triggers and effects
//!
//! Events are the mechanism for changing simulation state over time.
//! Each event has a trigger condition and a list of effects to apply when triggered.

use super::accounts::Account;
use super::ids::{AccountId, AssetId, EventId};

use jiff::ToSpan;
use serde::{Deserialize, Serialize};

/// How often a repeating event occurs
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

/// How a limit resets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LimitPeriod {
    /// Resets every calendar year
    Yearly,
    /// Never resets
    Lifetime,
}

/// Specifies how much to transfer - supports both simple cases and complex calculations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransferAmount {
    // === Simple Cases (90% of use) ===
    /// Fixed dollar amount
    Fixed(f64),

    /// Transfer entire source balance
    SourceBalance,

    /// Transfer enough to zero out target balance (for debt payoff)
    /// Calculates: -1 * target_balance (turns negative debt to zero)
    ZeroTargetBalance,

    /// Transfer enough to bring target to specified balance
    /// Calculates: max(0, target_balance - current_target_balance)
    TargetToBalance(f64),

    // === Balance References ===
    /// Reference a specific asset's balance
    AssetBalance {
        account_id: AccountId,
        asset_id: AssetId,
    },

    /// Reference total account balance (sum of all assets)
    AccountBalance { account_id: AccountId },

    // === Arithmetic Operations (for complex cases) ===
    /// Minimum of two amounts
    Min(Box<TransferAmount>, Box<TransferAmount>),

    /// Maximum of two amounts
    Max(Box<TransferAmount>, Box<TransferAmount>),

    /// Subtract: left - right
    Sub(Box<TransferAmount>, Box<TransferAmount>),

    /// Add: left + right
    Add(Box<TransferAmount>, Box<TransferAmount>),

    /// Multiply: left * right
    Mul(Box<TransferAmount>, Box<TransferAmount>),
}

impl TransferAmount {
    /// Transfer the lesser of a fixed amount or available balance
    pub fn up_to(amount: f64) -> Self {
        TransferAmount::Min(
            Box::new(TransferAmount::Fixed(amount)),
            Box::new(TransferAmount::SourceBalance),
        )
    }

    /// Transfer all balance above a reserve amount
    pub fn excess_above(reserve: f64) -> Self {
        TransferAmount::Max(
            Box::new(TransferAmount::Fixed(0.0)),
            Box::new(TransferAmount::Sub(
                Box::new(TransferAmount::SourceBalance),
                Box::new(TransferAmount::Fixed(reserve)),
            )),
        )
    }
}

/// Source or destination for a transfer
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransferEndpoint {
    /// External world (income source or expense destination)
    /// No cost basis tracking, no capital gains
    External,

    /// Specific asset within an account
    Asset {
        account_id: AccountId,
        asset_id: AssetId,
    },
}

/// Limits on cumulative transfer amounts (e.g., IRS contribution limits)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FlowLimits {
    /// Maximum cumulative amount
    pub limit: f64,
    /// How often the limit resets
    pub period: LimitPeriod,
}

/// Method for selecting which lots to sell (affects capital gains calculation)
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum LotMethod {
    /// First-in, first-out (default, most common)
    #[default]
    Fifo,
    /// Last-in, first-out
    Lifo,
    /// Sell highest cost lots first (minimize realized gains)
    HighestCost,
    /// Sell lowest cost lots first (realize gains in low-income years)
    LowestCost,
    /// Average cost basis (common for mutual funds)
    AverageCost,
}

/// Pre-defined withdrawal order strategies
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum WithdrawalOrder {
    /// Taxable accounts first, then tax-deferred, then tax-free
    /// Minimizes taxes in early retirement, preserves tax-advantaged growth
    #[default]
    TaxEfficientEarly,

    /// Tax-deferred first, then taxable, then tax-free
    /// Good for filling lower tax brackets in early retirement
    TaxDeferredFirst,

    /// Tax-free first, then taxable, then tax-deferred
    /// Rarely optimal, but available
    TaxFreeFirst,

    /// Pro-rata from all accounts proportionally
    /// Maintains consistent tax treatment over time
    ProRata,
}

/// Source configuration for Sweep withdrawals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WithdrawalSources {
    /// Use a pre-defined withdrawal order strategy
    /// Automatically selects from all non-excluded liquid accounts
    Strategy {
        order: WithdrawalOrder,
        /// Accounts to exclude from automatic selection
        #[serde(default)]
        exclude_accounts: Vec<AccountId>,
    },

    /// Explicitly specify accounts/assets in priority order
    Custom(Vec<(AccountId, AssetId)>),
}

impl Default for WithdrawalSources {
    fn default() -> Self {
        WithdrawalSources::Strategy {
            order: WithdrawalOrder::TaxEfficientEarly,
            exclude_accounts: vec![],
        }
    }
}

/// How to interpret the withdrawal amount
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum WithdrawalAmountMode {
    /// Amount is gross (before taxes)
    /// Withdraw exactly this amount, taxes come out of it
    #[default]
    Gross,

    /// Amount is net (after taxes)
    /// Gross up withdrawal to cover taxes, so you receive this amount
    Net,
}

/// Time offset relative to another event
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TriggerOffset {
    Days(i32),
    Months(i32),
    Years(i32),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
        /// Optional: stop repeating when this condition is met
        #[serde(default)]
        end_condition: Option<Box<EventTrigger>>,
    },

    // === Manual/Simulation Control ===
    /// Never triggers automatically; can only be triggered by TriggerEvent effect
    Manual,
}

/// Actions that can occur when an event triggers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventEffect {
    // === Account Management ===
    CreateAccount(Account),
    DeleteAccount(AccountId),

    // === Money Movement ===
    /// Transfer money between endpoints (external or assets)
    /// Tax implications are automatic based on account types
    Transfer {
        from: TransferEndpoint,
        to: TransferEndpoint,
        amount: TransferAmount,
        #[serde(default)]
        adjust_for_inflation: bool,
        #[serde(default)]
        limits: Option<FlowLimits>,
    },

    /// Explicitly liquidate assets with capital gains handling
    Liquidate {
        from_account: AccountId,
        from_asset: AssetId,
        to_account: AccountId,
        to_asset: AssetId,
        amount: TransferAmount,
        #[serde(default)]
        lot_method: LotMethod,
    },

    /// Multi-source sweep with withdrawal strategy
    /// Replaces SpendingTarget functionality
    Sweep {
        to_account: AccountId,
        to_asset: AssetId,
        target: TransferAmount,
        #[serde(default)]
        sources: WithdrawalSources,
        #[serde(default)]
        amount_mode: WithdrawalAmountMode,
        #[serde(default)]
        lot_method: LotMethod,
    },

    // === Event Control ===
    /// Trigger another event immediately
    TriggerEvent(EventId),
    /// Pause a repeating event
    PauseEvent(EventId),
    /// Resume a paused event  
    ResumeEvent(EventId),
    /// Terminate an event permanently
    TerminateEvent(EventId),

    // === RMD (Required Minimum Distributions) ===
    /// Set up automatic RMD withdrawals from tax-deferred account
    CreateRmdWithdrawal {
        account_id: AccountId,
        starting_age: u8,
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
