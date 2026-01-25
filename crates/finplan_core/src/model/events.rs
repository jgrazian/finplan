//! Event system - triggers and effects
//!
//! Events are the mechanism for changing simulation state over time.
//! Each event has a trigger condition and a list of effects to apply when triggered.

use crate::model::AssetCoord;

use super::accounts::Account;
use super::ids::{AccountId, AssetId, EventId};

use jiff::ToSpan;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts")]
use ts_rs::TS;

/// How often a repeating event occurs
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Hash)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
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
        asset_coord: AssetCoord,
    },

    /// Reference total account balance (sum of all assets)
    AccountTotalBalance {
        account_id: AccountId,
    },

    AccountCashBalance {
        account_id: AccountId,
    },

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
    Cash {
        account_id: AccountId,
    },
    /// Specific asset within an account
    Asset {
        asset_coord: AssetCoord,
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
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
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
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

    /// Penalty-aware: avoids early withdrawal penalties
    /// Before age 59.5: Taxable → TaxFree → TaxDeferred (avoid 10% penalty)
    /// After age 59.5: Falls back to TaxEfficientEarly behavior
    PenaltyAware,
}

/// Source configuration for Sweep withdrawals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WithdrawalSources {
    /// Withdraw from a single specific asset
    /// Use this for simple single-source liquidations
    SingleAsset(AssetCoord),
    SingleAccount(AccountId),

    /// Use a pre-defined withdrawal order strategy
    /// Automatically selects from all non-excluded liquid accounts
    Strategy {
        order: WithdrawalOrder,
        /// Accounts to exclude from automatic selection
        #[serde(default)]
        exclude_accounts: Vec<AccountId>,
    },

    /// Explicitly specify accounts/assets in priority order
    Custom(Vec<AssetCoord>),
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
pub enum AmountMode {
    /// The amount specified is BEFORE taxes are applied.
    /// - For Income: Full salary; income taxes deducted from deposit
    /// - For AssetSale: Gross proceeds; capital gains taxes deducted
    Gross,

    /// The amount specified is what should be RECEIVED after taxes.
    /// - For Income: Take-home pay; gross back-calculated for tax records
    /// - For AssetSale: Net proceeds; system sells enough to cover taxes
    #[default]
    Net,
}

/// Time offset relative to another event
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub enum TriggerOffset {
    Days(i32),
    Months(i32),
    Years(i32),
}

impl TriggerOffset {
    /// Convert to a jiff::Span for date arithmetic.
    /// This is relatively expensive - prefer using add_to_date() instead.
    #[inline]
    pub fn to_span(&self) -> jiff::Span {
        use jiff::ToSpan;
        match self {
            TriggerOffset::Days(d) => (*d as i64).days(),
            TriggerOffset::Months(m) => (*m as i64).months(),
            TriggerOffset::Years(y) => (*y as i64).years(),
        }
    }

    /// Fast date addition that avoids expensive Span->DateArithmetic conversion.
    #[inline]
    pub fn add_to_date(&self, date: jiff::civil::Date) -> jiff::civil::Date {
        match self {
            TriggerOffset::Days(d) => {
                // Direct day addition via SignedDuration avoids Span overhead
                let duration = jiff::SignedDuration::from_hours(*d as i64 * 24);
                date.checked_add(duration).unwrap_or(date)
            }
            TriggerOffset::Months(m) => {
                // Manual month arithmetic
                let total_months = date.year() as i32 * 12 + date.month() as i32 - 1 + *m;
                let new_year = total_months.div_euclid(12) as i16;
                let new_month = (total_months.rem_euclid(12) + 1) as i8;
                let max_day = jiff::civil::date(new_year, new_month, 1).days_in_month() as i8;
                let new_day = date.day().min(max_day);
                jiff::civil::date(new_year, new_month, new_day)
            }
            TriggerOffset::Years(y) => {
                let new_year = (date.year() as i32 + *y) as i16;
                let max_day = jiff::civil::date(new_year, date.month(), 1).days_in_month() as i8;
                let new_day = date.day().min(max_day);
                jiff::civil::date(new_year, date.month(), new_day)
            }
        }
    }
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
        asset_coord: AssetCoord,
        threshold: BalanceThreshold,
    },

    /// Trigger when total net worth crosses threshold
    NetWorth { threshold: BalanceThreshold },

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

    // TODO: Add account limits triggers

    // === Manual/Simulation Control ===
    /// Never triggers automatically; can only be triggered by TriggerEvent effect
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum IncomeType {
    Taxable,
    TaxFree,
}

/// Actions that can occur when an event triggers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventEffect {
    // === Account Management ===
    CreateAccount(Account),
    DeleteAccount(AccountId),

    Income {
        to: AccountId,
        amount: TransferAmount,
        amount_mode: AmountMode,
        income_type: IncomeType,
    },

    Expense {
        from: AccountId,
        amount: TransferAmount,
    },

    /// Buy asset with cash (within same or different account)
    AssetPurchase {
        from: AccountId,
        to: AssetCoord,
        amount: TransferAmount,
    },

    /// Liquidate assets into the source account's cash balance
    /// Handles capital gains, lot tracking, and tax calculation automatically
    /// Cash proceeds remain in the source account - use Income/Expense to move money
    AssetSale {
        /// Source account to liquidate from
        from: AccountId,
        /// Specific asset to liquidate, or None to liquidate all assets in account
        asset_id: Option<AssetId>,
        /// Amount to liquidate at current market prices
        amount: TransferAmount,
        /// Gross = liquidate target amount gross (net varies by taxes)
        /// Net = liquidate enough gross to achieve target net after taxes
        #[serde(default)]
        amount_mode: AmountMode,
        #[serde(default)]
        lot_method: LotMethod,
    },

    /// Sweep: Liquidate assets and transfer to another account
    /// Combines AssetSale + Income in a single operation
    /// Common use case for RMDs and rebalancing between accounts
    Sweep {
        /// Source(s) to liquidate from
        #[serde(default)]
        sources: WithdrawalSources,
        /// Destination account for cash proceeds
        to: AccountId,
        /// Amount to liquidate/transfer
        amount: TransferAmount,
        /// Gross = liquidate target amount gross
        /// Net = liquidate enough to achieve target net after taxes
        #[serde(default)]
        amount_mode: AmountMode,
        #[serde(default)]
        lot_method: LotMethod,
        /// Tax treatment for the transfer (e.g., Taxable for RMDs)
        income_type: IncomeType,
    },

    // === Balance & Transfer Operations ===
    /// Adjust an account's balance directly
    /// For liabilities: positive = increase debt, negative = decrease debt
    /// For cash accounts: positive = add cash, negative = remove cash
    AdjustBalance {
        /// The account to modify
        account: AccountId,
        /// Amount to add (negative to subtract)
        amount: TransferAmount,
    },

    /// Transfer cash between accounts
    /// Debits from source, credits to destination
    /// If destination is a liability, reduces the principal instead
    CashTransfer {
        /// Source cash account
        from: AccountId,
        /// Destination account (cash or liability)
        to: AccountId,
        /// Amount to transfer
        amount: TransferAmount,
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
    /// Apply RMD withdrawals to all eligible tax-deferred accounts
    /// Uses the IRS Uniform Lifetime Table to calculate required amounts
    /// Only processes accounts where the person has reached RMD age (typically 73)
    /// Proceeds are deposited to the specified destination account/asset
    ApplyRmd {
        destination: AccountId,
        lot_method: LotMethod,
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
