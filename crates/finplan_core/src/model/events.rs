//! Event system - triggers and effects
//!
//! Events are the mechanism for changing simulation state over time.
//! Each event has a trigger condition and a list of effects to apply when triggered.

use crate::model::AssetCoord;

use super::accounts::Account;
use super::ids::{AccountId, AssetId, EventId, ParameterId};

use serde::{Deserialize, Serialize};

/// How often a repeating event occurs
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Hash)]
pub enum RepeatInterval {
    Never,
    Weekly,
    BiWeekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl RepeatInterval {
    /// Fast date advancement without going through `jiff::Span`.
    #[must_use]
    #[inline]
    pub fn add_to_date(&self, date: jiff::civil::Date) -> jiff::civil::Date {
        use crate::date_math::{add_days, days_in_month};
        match self {
            RepeatInterval::Never => date,
            RepeatInterval::Weekly => add_days(date, 7),
            RepeatInterval::BiWeekly => add_days(date, 14),
            RepeatInterval::Monthly => TriggerOffset::Months(1).add_to_date(date),
            RepeatInterval::Quarterly => TriggerOffset::Months(3).add_to_date(date),
            RepeatInterval::Yearly => {
                let new_year = date.year() + 1;
                let max_day = days_in_month(new_year, date.month());
                let new_day = date.day().min(max_day);
                jiff::civil::date(new_year, date.month(), new_day)
            }
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

/// Expression-backed amount used by every effect.
pub use crate::expression::TransferAmount;

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

    /// Penalty-aware: avoids early withdrawal penalties
    /// Before age 59.5: Taxable → `TaxFree` → `TaxDeferred` (avoid 10% penalty)
    /// After age 59.5: Falls back to `TaxEfficientEarly` behavior
    PenaltyAware,

    /// Bracket filling: from age 59.5, draw `TaxDeferred` first but only until
    /// the year's ordinary income reaches the top of the `ceiling_rate`
    /// bracket, then continue as `PenaltyAware`. Spends the low brackets on
    /// pre-tax money that would otherwise be taxed higher later (RMDs).
    /// Before 59.5 it is `PenaltyAware`: that income would carry the penalty.
    BracketFilling {
        /// The highest marginal rate to fill to, e.g. 0.12.
        ceiling_rate: f64,
    },
}

impl WithdrawalOrder {
    /// The ceiling a `BracketFilling` order uses when none is given.
    pub const DEFAULT_BRACKET_CEILING: f64 = 0.12;
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
    /// - For `AssetSale`: Gross proceeds; capital gains taxes deducted
    Gross,

    /// The amount specified is what should be RECEIVED after taxes.
    /// - For Income: Take-home pay; gross back-calculated for tax records
    /// - For `AssetSale`: Net proceeds; system sells enough to cover taxes
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
    /// Fast date addition that avoids expensive Span->DateArithmetic conversion.
    #[inline]
    #[must_use]
    pub fn add_to_date(&self, date: jiff::civil::Date) -> jiff::civil::Date {
        match self {
            TriggerOffset::Days(d) => {
                // Direct Rata Die arithmetic — avoids all jiff Span/Duration overhead
                crate::date_math::add_days(date, *d)
            }
            TriggerOffset::Months(m) => {
                // Manual month arithmetic
                let total_months = i32::from(date.year()) * 12 + i32::from(date.month()) - 1 + *m;
                let new_year = total_months.div_euclid(12) as i16;
                let new_month = (total_months.rem_euclid(12) + 1) as i8;
                let max_day = crate::date_math::days_in_month(new_year, new_month);
                let new_day = date.day().min(max_day);
                jiff::civil::date(new_year, new_month, new_day)
            }
            TriggerOffset::Years(y) => {
                let new_year = (i32::from(date.year()) + *y) as i16;
                let max_day = crate::date_math::days_in_month(new_year, date.month());
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
    #[must_use]
    pub fn value(&self) -> f64 {
        match self {
            BalanceThreshold::GreaterThanOrEqual(v) | BalanceThreshold::LessThanOrEqual(v) => *v,
        }
    }

    #[must_use]
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

    /// Date supplied by a named parameter; bound before the run begins.
    DateParameter(ParameterId),

    /// Trigger at a specific age (requires `birth_date` in `SimulationParameters`)
    Age { years: u8, months: Option<u8> },

    /// Calendar age supplied by a named parameter; requires `birth_date`.
    AgeParameter(ParameterId),

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
        /// Optional: maximum number of times this event can trigger
        /// After reaching this count, the event stops repeating (equivalent to `StopRepeating`)
        #[serde(default)]
        max_occurrences: Option<u32>,
    },

    // TODO: Add account limits triggers

    // === Manual/Simulation Control ===
    /// Never triggers automatically; can only be triggered by `TriggerEvent` effect
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Combines `AssetSale` + Income in a single operation
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

    // === Stochastic Effects ===
    /// Randomly execute one of two effects based on a probability threshold
    /// Useful for modeling uncertain events like job loss, medical expenses, inheritance, etc.
    ///
    /// The probability is checked against the simulation's RNG:
    /// - If random value < probability: execute `on_true`
    /// - Otherwise: execute `on_false` (if provided)
    ///
    /// Each Monte Carlo iteration will get different random outcomes based on its seed,
    /// making this suitable for modeling uncertainty in financial plans.
    Random {
        /// Probability threshold (0.0 to 1.0). E.g., 0.1 = 10% chance of `on_true`
        probability: f64,
        /// Effect to execute if random check passes
        on_true: Box<EventEffect>,
        /// Optional effect to execute if random check fails
        #[serde(default)]
        on_false: Option<Box<EventEffect>>,
    },

    // === Real Estate ===
    /// Buy a property: its price lands on the Property account (and in its
    /// cost basis), the cash side leaves `from` as a transfer rather than an
    /// expense, and — when financed — the loan is drawn for the rest and
    /// starts amortizing a month later.
    BuyProperty {
        /// The Property account the home is held in.
        property: AccountId,
        /// Purchase price.
        price: TransferAmount,
        /// Cash account paying the down payment, or the whole price when
        /// there is no financing.
        from: AccountId,
        #[serde(default)]
        financing: Option<Financing>,
    },

    /// Sell a property: proceeds net of selling costs and capital-gains tax
    /// (after any exclusion) land in `to`, and — when named — the loan is
    /// paid off out of them first.
    SellProperty {
        /// The Property account being sold.
        property: AccountId,
        /// Cash account receiving the proceeds.
        to: AccountId,
        /// Agent fees and closing costs, as a share of the sale price.
        #[serde(default)]
        selling_cost_rate: f64,
        /// Gain excluded from tax — $250k single or $500k joint for a primary
        /// residence held two of the last five years, 0 otherwise.
        #[serde(default)]
        gain_exclusion: f64,
        /// Loan paid off from the proceeds.
        #[serde(default)]
        payoff: Option<AccountId>,
    },

    // === Market Events ===
    /// A one-time crash: the price of every *market* asset drops by `drop`
    /// (a fraction, `0.3` = −30%) at the moment the event fires, and keeps
    /// compounding from the lower level afterwards.
    ///
    /// "Market" assets are the ones held as lots in investment accounts (and
    /// any registered asset not backing a property). Cash balances, property
    /// values and liability principals are left alone: a stock crash does not
    /// mark down a checking account, a house, or a mortgage. Cost bases are
    /// untouched, so a later sale realizes the loss.
    MarketShock {
        /// Fraction of value lost, in `(0, 1)`. Clamped to `[0, 1]`.
        drop: f64,
    },

    // === Equity Compensation ===
    /// RSU vesting: shares vest and are deposited to an investment account.
    /// The FMV at vesting is taxed as ordinary income.
    /// Optionally sells a portion of shares to cover taxes (sell-to-cover).
    RsuVesting {
        /// Investment account where vested shares are deposited
        to: AccountId,
        /// Asset representing the company stock
        asset: AssetCoord,
        /// Number of shares vesting
        units: f64,
        /// If true, automatically sell shares to cover income taxes (sell-to-cover)
        #[serde(default)]
        sell_to_cover: bool,
        /// Lot method for sell-to-cover liquidation
        #[serde(default)]
        lot_method: LotMethod,
    },
}

/// How a `BuyProperty` is financed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Financing {
    /// The Liability account drawn for `price - down_payment`. Its interest
    /// rate is the loan's rate.
    pub loan: AccountId,
    /// Cash put down; the loan covers the rest of the price.
    pub down_payment: TransferAmount,
    /// Months to amortize over — 360 for a 30-year mortgage.
    pub term_months: u32,
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
