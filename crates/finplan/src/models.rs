use crate::profiles::{InflationProfile, ReturnProfile};
use jiff::ToSpan;
use serde::{Deserialize, Serialize};

/// Unique identifier for an Account within a simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(pub u16);

/// Unique identifier for a Asset within a simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId(pub u16);

/// Unique identifier for a CashFlow within a simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CashFlowId(pub u16);

/// Unique identifier for a Event within a simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub u16);

/// Unique identifier for a SpendingTarget within a simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpendingTargetId(pub u16);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetClass {
    Investable,   // Stocks, bonds, mutual funds
    RealEstate,   // Property value
    Depreciating, // Cars, boats, equipment
    Liability,    // Loans, mortgages (value should be negative)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub asset_id: AssetId,
    pub asset_class: AssetClass,
    pub initial_value: f64,
    pub return_profile_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountType {
    Taxable,
    TaxDeferred, // 401k, Traditional IRA
    TaxFree,     // Roth IRA
    Illiquid,    // Real estate, vehicles - not liquid
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub assets: Vec<Asset>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Timepoint {
    Immediate,
    /// A specific fixed date (ad-hoc)
    Date(jiff::civil::Date),
    /// Reference to a named event in SimulationParameters
    Event(EventId),
    Never,
}

/// Specifies where money flows from or to
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CashFlowEndpoint {
    /// Income from outside the simulation / expenses leaving the simulation
    External,
    /// A specific asset within an account
    Asset {
        account_id: AccountId,
        asset_id: AssetId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlow {
    pub cash_flow_id: CashFlowId,
    pub name: Option<String>,
    pub amount: f64,
    pub repeats: RepeatInterval,
    pub cash_flow_limits: Option<CashFlowLimits>,
    pub adjust_for_inflation: bool,
    /// Where money comes from (External = income from outside the simulation)
    pub source: CashFlowEndpoint,
    /// Where money goes to (External = expense leaving the simulation)
    pub target: CashFlowEndpoint,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct CashFlowEvent {
    pub cash_flow_id: CashFlowId,
    pub amount: f64,
    pub date: jiff::civil::Date,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LimitPeriod {
    /// Resets every calendar
    Yearly,
    /// Never resets
    Lifetime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlowLimits {
    pub limit: f64,
    pub limit_period: LimitPeriod,
}

// ============================================================================
// Event System - Triggers and Effects
// ============================================================================

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

/// Time offset relative to another event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TriggerOffset {
    Days(i32),
    Months(i32),
    Years(i32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: EventId,
    /// Human-readable name for debugging/UI
    pub name: Option<String>,
    pub trigger: EventTrigger,
    /// Effects to apply when this event triggers (executed in order)
    #[serde(default)]
    pub effects: Vec<EventEffect>,
    /// If true, this event can only trigger once
    #[serde(default)]
    pub once: bool,
}

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
        threshold: f64,
        above: bool, // true = trigger when balance > threshold, false = balance < threshold
    },

    /// Trigger when a specific asset balance crosses threshold
    AssetBalance {
        account_id: AccountId,
        asset_id: AssetId,
        threshold: f64,
        above: bool,
    },

    /// Trigger when total net worth crosses threshold
    NetWorth { threshold: f64, above: bool },

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
}

// ============================================================================
// Tax Configuration
// ============================================================================

/// A single bracket in a progressive tax system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxBracket {
    /// Income threshold where this bracket begins
    pub threshold: f64,
    /// Marginal tax rate for income in this bracket (e.g., 0.22 for 22%)
    pub rate: f64,
}

/// Tax configuration for the simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxConfig {
    /// Federal income tax brackets (must be sorted by threshold ascending)
    pub federal_brackets: Vec<TaxBracket>,
    /// Flat state income tax rate (e.g., 0.05 for 5%)
    pub state_rate: f64,
    /// Long-term capital gains tax rate (e.g., 0.15 for 15%)
    pub capital_gains_rate: f64,
    /// Estimated percentage of taxable account withdrawals that are gains (0.0 to 1.0)
    /// Used as a simplification instead of full cost basis tracking
    pub taxable_gains_percentage: f64,
}

impl Default for TaxConfig {
    /// Returns a reasonable default based on 2024 US federal brackets (single filer)
    fn default() -> Self {
        Self {
            federal_brackets: vec![
                TaxBracket {
                    threshold: 0.0,
                    rate: 0.10,
                },
                TaxBracket {
                    threshold: 11_600.0,
                    rate: 0.12,
                },
                TaxBracket {
                    threshold: 47_150.0,
                    rate: 0.22,
                },
                TaxBracket {
                    threshold: 100_525.0,
                    rate: 0.24,
                },
                TaxBracket {
                    threshold: 191_950.0,
                    rate: 0.32,
                },
                TaxBracket {
                    threshold: 243_725.0,
                    rate: 0.35,
                },
                TaxBracket {
                    threshold: 609_350.0,
                    rate: 0.37,
                },
            ],
            state_rate: 0.05,
            capital_gains_rate: 0.15,
            taxable_gains_percentage: 0.50,
        }
    }
}

// ============================================================================
// Spending Targets & Withdrawal Strategies
// ============================================================================

/// Strategy for withdrawing funds from multiple accounts to meet a spending target
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum WithdrawalStrategy {
    /// Withdraw from accounts in the specified order until target is met
    /// Skips Illiquid accounts automatically
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

/// A spending target represents a required withdrawal amount
/// The simulation will pull from accounts to meet this target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendingTarget {
    pub spending_target_id: SpendingTargetId,
    pub name: Option<String>,
    /// The target amount (gross or net depending on net_amount_mode)
    pub amount: f64,
    /// If true, `amount` is the after-tax target; system will gross up for taxes
    /// If false, `amount` is the pre-tax withdrawal amount
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

// ============================================================================
// Tax Results & Tracking
// ============================================================================

/// Summary of taxes for a single year
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaxSummary {
    pub year: i16,
    /// Income from TaxDeferred account withdrawals (taxed as ordinary income)
    pub ordinary_income: f64,
    /// Realized capital gains from Taxable account withdrawals
    pub capital_gains: f64,
    /// Withdrawals from TaxFree accounts (not taxed)
    pub tax_free_withdrawals: f64,
    /// Total federal tax owed
    pub federal_tax: f64,
    /// Total state tax owed
    pub state_tax: f64,
    /// Total tax owed (federal + state + capital gains)
    pub total_tax: f64,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimulationParameters {
    pub start_date: Option<jiff::civil::Date>,
    #[serde(default = "default_duration_years")]
    pub duration_years: usize,
    /// Birth date for age-based triggers
    pub birth_date: Option<jiff::civil::Date>,
    #[serde(default)]
    pub inflation_profile: InflationProfile,
    #[serde(default)]
    pub return_profiles: Vec<ReturnProfile>,
    /// Events define triggers and their effects
    #[serde(default)]
    pub events: Vec<Event>,
    /// Initial accounts (more can be created via events)
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// Initial cash flows - typically start in Pending state
    /// Use events to activate them, or set state: Active for immediate
    #[serde(default)]
    pub cash_flows: Vec<CashFlow>,
    /// Spending targets for retirement withdrawals (more can be created via events)
    #[serde(default)]
    pub spending_targets: Vec<SpendingTarget>,
    /// Tax configuration (uses US 2024 defaults if not specified)
    #[serde(default)]
    pub tax_config: TaxConfig,
}

fn default_duration_years() -> usize {
    30
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimulationResult {
    pub yearly_inflation: Vec<f64>,
    pub dates: Vec<jiff::civil::Date>,
    pub return_profile_returns: Vec<Vec<f64>>,
    pub triggered_events: std::collections::HashMap<EventId, jiff::civil::Date>,
    pub account_histories: Vec<AccountHistory>,
    /// Tax summaries per year
    pub yearly_taxes: Vec<TaxSummary>,
    /// Detailed record of all withdrawals
    pub withdrawal_history: Vec<WithdrawalRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountHistory {
    pub account_id: AccountId,
    pub assets: Vec<AssetHistory>,
}

impl AccountHistory {
    pub fn values(&self, dates: &[jiff::civil::Date]) -> Vec<AccountSnapshot> {
        dates
            .iter()
            .enumerate()
            .map(|(i, date)| {
                let balance = self.assets.iter().map(|a| a.values[i]).sum();
                AccountSnapshot {
                    date: *date,
                    balance,
                }
            })
            .collect()
    }

    pub fn current_balance(&self) -> f64 {
        self.assets
            .iter()
            .map(|a| a.values.last().copied().unwrap_or(0.0))
            .sum()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetHistory {
    pub asset_id: AssetId,
    pub return_profile_index: usize,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub date: jiff::civil::Date,
    pub balance: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub iterations: Vec<SimulationResult>,
}
