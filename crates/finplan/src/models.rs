use crate::profiles::{InflationProfile, ReturnProfile};
use jiff::ToSpan;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// Direction of a CashFlow - either income (money entering) or expense (money leaving)
/// Internal transfers between assets should use EventEffect::TransferAsset instead
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CashFlowDirection {
    /// Income: money flows from External into an Asset
    Income {
        target_account_id: AccountId,
        target_asset_id: AssetId,
    },
    /// Expense: money flows from an Asset to External
    Expense {
        source_account_id: AccountId,
        source_asset_id: AssetId,
    },
}

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

/// Record of a CashFlow execution (income or expense only)
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
    /// Starting state of all accounts (replay from transaction logs to get future values)
    pub accounts: Vec<AccountSnapshot>,
    /// Tax summaries per year
    pub yearly_taxes: Vec<TaxSummary>,

    // === Transaction Logs ===
    /// Record of all event triggers in chronological order (for replay)
    pub event_history: Vec<EventRecord>,
    /// Record of all CashFlow executions (income deposits, expense withdrawals)
    pub cash_flow_history: Vec<CashFlowRecord>,
    /// Record of all investment returns applied to assets
    pub return_history: Vec<ReturnRecord>,
    /// Record of all transfers between accounts/assets
    pub transfer_history: Vec<TransferRecord>,
    /// Record of all SpendingTarget withdrawals
    pub withdrawal_history: Vec<WithdrawalRecord>,
}

/// Snapshot of an account's starting state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub assets: Vec<AssetSnapshot>,
}

impl AccountSnapshot {
    /// Get starting balance (sum of all asset initial values)
    pub fn starting_balance(&self) -> f64 {
        self.assets.iter().map(|a| a.starting_value).sum()
    }
}

/// Snapshot of an asset's starting state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetSnapshot {
    pub asset_id: AssetId,
    pub return_profile_index: usize,
    pub starting_value: f64,
}

impl SimulationResult {
    /// Calculate the final balance for a specific account by replaying transaction logs
    pub fn final_account_balance(&self, account_id: AccountId) -> f64 {
        // Start with initial values
        let account = self.accounts.iter().find(|a| a.account_id == account_id);
        let mut balance: f64 = account.map(|a| a.starting_balance()).unwrap_or(0.0);

        // Add cash flows (income positive, expenses negative via amount field)
        for cf in &self.cash_flow_history {
            if cf.account_id == account_id {
                balance += cf.amount;
            }
        }

        // Add returns
        for ret in &self.return_history {
            if ret.account_id == account_id {
                balance += ret.return_amount;
            }
        }

        // Apply transfers (subtract outgoing, add incoming)
        for transfer in &self.transfer_history {
            if transfer.from_account_id == account_id {
                balance -= transfer.amount;
            }
            if transfer.to_account_id == account_id {
                balance += transfer.amount;
            }
        }

        // Subtract spending target withdrawals
        for withdrawal in &self.withdrawal_history {
            if withdrawal.account_id == account_id {
                balance -= withdrawal.gross_amount;
            }
        }

        balance
    }

    /// Calculate the final balance for a specific asset by replaying transaction logs
    pub fn final_asset_balance(&self, account_id: AccountId, asset_id: AssetId) -> f64 {
        // Start with initial value
        let initial = self
            .accounts
            .iter()
            .find(|a| a.account_id == account_id)
            .and_then(|a| a.assets.iter().find(|asset| asset.asset_id == asset_id))
            .map(|a| a.starting_value)
            .unwrap_or(0.0);

        let mut balance = initial;

        // Add cash flows
        for cf in &self.cash_flow_history {
            if cf.account_id == account_id && cf.asset_id == asset_id {
                balance += cf.amount;
            }
        }

        // Add returns
        for ret in &self.return_history {
            if ret.account_id == account_id && ret.asset_id == asset_id {
                balance += ret.return_amount;
            }
        }

        // Apply transfers
        for transfer in &self.transfer_history {
            if transfer.from_account_id == account_id && transfer.from_asset_id == asset_id {
                balance -= transfer.amount;
            }
            if transfer.to_account_id == account_id && transfer.to_asset_id == asset_id {
                balance += transfer.amount;
            }
        }

        // Subtract spending target withdrawals
        for withdrawal in &self.withdrawal_history {
            if withdrawal.account_id == account_id && withdrawal.asset_id == asset_id {
                balance -= withdrawal.gross_amount;
            }
        }

        balance
    }

    /// Check if an event was triggered at any point
    pub fn event_was_triggered(&self, event_id: EventId) -> bool {
        self.event_history.iter().any(|e| e.event_id == event_id)
    }

    /// Get the date when an event was first triggered
    pub fn event_trigger_date(&self, event_id: EventId) -> Option<jiff::civil::Date> {
        self.event_history
            .iter()
            .find(|e| e.event_id == event_id)
            .map(|e| e.date)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub iterations: Vec<SimulationResult>,
}

// ============================================================================
// Simulation Metadata
// ============================================================================

/// Metadata entry for any simulation entity
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EntityMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Holds human-readable names and descriptions for simulation entities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimulationMetadata {
    pub accounts: HashMap<AccountId, EntityMetadata>,
    pub assets: HashMap<AssetId, EntityMetadata>,
    pub cash_flows: HashMap<CashFlowId, EntityMetadata>,
    pub events: HashMap<EventId, EntityMetadata>,
    pub spending_targets: HashMap<SpendingTargetId, EntityMetadata>,
}

impl SimulationMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_account_name(&self, id: AccountId) -> Option<&str> {
        self.accounts.get(&id)?.name.as_deref()
    }

    pub fn get_asset_name(&self, id: AssetId) -> Option<&str> {
        self.assets.get(&id)?.name.as_deref()
    }

    pub fn get_cash_flow_name(&self, id: CashFlowId) -> Option<&str> {
        self.cash_flows.get(&id)?.name.as_deref()
    }

    pub fn get_event_name(&self, id: EventId) -> Option<&str> {
        self.events.get(&id)?.name.as_deref()
    }

    pub fn get_spending_target_name(&self, id: SpendingTargetId) -> Option<&str> {
        self.spending_targets.get(&id)?.name.as_deref()
    }
}

// ============================================================================
// Descriptor Structs (for Builder API)
// ============================================================================

/// Descriptor for creating an account (without ID)
#[derive(Debug, Clone)]
pub struct AccountDescriptor {
    pub account_type: AccountType,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl AccountDescriptor {
    pub fn new(account_type: AccountType) -> Self {
        Self {
            account_type,
            name: None,
            description: None,
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Descriptor for creating an asset (without ID)
#[derive(Debug, Clone)]
pub struct AssetDescriptor {
    pub asset_class: AssetClass,
    pub initial_value: f64,
    pub return_profile_index: usize,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl AssetDescriptor {
    pub fn new(asset_class: AssetClass, initial_value: f64, return_profile_index: usize) -> Self {
        Self {
            asset_class,
            initial_value,
            return_profile_index,
            name: None,
            description: None,
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Descriptor for creating a cash flow (without ID)
#[derive(Debug, Clone)]
pub struct CashFlowDescriptor {
    pub amount: f64,
    pub repeats: RepeatInterval,
    pub direction: CashFlowDirection,
    pub adjust_for_inflation: bool,
    pub state: CashFlowState,
    pub limits: Option<CashFlowLimits>,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl CashFlowDescriptor {
    pub fn new(amount: f64, repeats: RepeatInterval, direction: CashFlowDirection) -> Self {
        Self {
            amount,
            repeats,
            direction,
            adjust_for_inflation: false,
            state: CashFlowState::Pending,
            limits: None,
            name: None,
            description: None,
        }
    }

    /// Create an income CashFlow (External → Asset)
    pub fn income(
        amount: f64,
        repeats: RepeatInterval,
        target_account_id: AccountId,
        target_asset_id: AssetId,
    ) -> Self {
        Self::new(
            amount,
            repeats,
            CashFlowDirection::Income {
                target_account_id,
                target_asset_id,
            },
        )
    }

    /// Create an expense CashFlow (Asset → External)
    pub fn expense(
        amount: f64,
        repeats: RepeatInterval,
        source_account_id: AccountId,
        source_asset_id: AssetId,
    ) -> Self {
        Self::new(
            amount,
            repeats,
            CashFlowDirection::Expense {
                source_account_id,
                source_asset_id,
            },
        )
    }

    pub fn adjust_for_inflation(mut self, adjust: bool) -> Self {
        self.adjust_for_inflation = adjust;
        self
    }

    pub fn state(mut self, state: CashFlowState) -> Self {
        self.state = state;
        self
    }

    pub fn limits(mut self, limits: CashFlowLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Descriptor for creating an event (without ID)
#[derive(Debug, Clone)]
pub struct EventDescriptor {
    pub trigger: EventTrigger,
    pub effects: Vec<EventEffect>,
    pub once: bool,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl EventDescriptor {
    pub fn new(trigger: EventTrigger, effects: Vec<EventEffect>) -> Self {
        Self {
            trigger,
            effects,
            once: false,
            name: None,
            description: None,
        }
    }

    pub fn once(mut self) -> Self {
        self.once = true;
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Descriptor for creating a spending target (without ID)
#[derive(Debug, Clone)]
pub struct SpendingTargetDescriptor {
    pub amount: f64,
    pub repeats: RepeatInterval,
    pub withdrawal_strategy: WithdrawalStrategy,
    pub adjust_for_inflation: bool,
    pub net_amount_mode: bool,
    pub state: SpendingTargetState,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl SpendingTargetDescriptor {
    pub fn new(amount: f64, repeats: RepeatInterval) -> Self {
        Self {
            amount,
            repeats,
            withdrawal_strategy: WithdrawalStrategy::default(),
            adjust_for_inflation: false,
            net_amount_mode: false,
            state: SpendingTargetState::Pending,
            name: None,
            description: None,
        }
    }

    pub fn withdrawal_strategy(mut self, strategy: WithdrawalStrategy) -> Self {
        self.withdrawal_strategy = strategy;
        self
    }

    pub fn adjust_for_inflation(mut self, adjust: bool) -> Self {
        self.adjust_for_inflation = adjust;
        self
    }

    pub fn net_amount_mode(mut self, net: bool) -> Self {
        self.net_amount_mode = net;
        self
    }

    pub fn state(mut self, state: SpendingTargetState) -> Self {
        self.state = state;
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

// ============================================================================
// Simulation Builder
// ============================================================================

/// Builder for creating simulations with automatic ID assignment and metadata tracking
pub struct SimulationBuilder {
    params: SimulationParameters,
    metadata: SimulationMetadata,
    next_account_id: u16,
    next_asset_id: u16,
    next_cash_flow_id: u16,
    next_event_id: u16,
    next_spending_target_id: u16,
}

impl Default for SimulationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationBuilder {
    pub fn new() -> Self {
        Self {
            params: SimulationParameters::default(),
            metadata: SimulationMetadata::new(),
            next_account_id: 0,
            next_asset_id: 0,
            next_cash_flow_id: 0,
            next_event_id: 0,
            next_spending_target_id: 0,
        }
    }

    /// Set the simulation start date
    pub fn start_date(mut self, date: jiff::civil::Date) -> Self {
        self.params.start_date = Some(date);
        self
    }

    /// Set the simulation duration in years
    pub fn duration_years(mut self, years: usize) -> Self {
        self.params.duration_years = years;
        self
    }

    /// Set the birth date for age-based triggers
    pub fn birth_date(mut self, date: jiff::civil::Date) -> Self {
        self.params.birth_date = Some(date);
        self
    }

    /// Set the inflation profile
    pub fn inflation_profile(mut self, profile: InflationProfile) -> Self {
        self.params.inflation_profile = profile;
        self
    }

    /// Add a return profile
    pub fn add_return_profile(mut self, profile: ReturnProfile) -> Self {
        self.params.return_profiles.push(profile);
        self
    }

    /// Set the tax configuration
    pub fn tax_config(mut self, config: TaxConfig) -> Self {
        self.params.tax_config = config;
        self
    }

    /// Add an account using a descriptor
    pub fn add_account(mut self, descriptor: AccountDescriptor) -> (Self, AccountId) {
        let account_id = AccountId(self.next_account_id);
        self.next_account_id += 1;

        let account = Account {
            account_id,
            account_type: descriptor.account_type,
            assets: Vec::new(),
        };

        self.params.accounts.push(account);
        self.metadata.accounts.insert(
            account_id,
            EntityMetadata {
                name: descriptor.name,
                description: descriptor.description,
            },
        );

        (self, account_id)
    }

    /// Add an asset to an existing account using a descriptor
    pub fn add_asset(
        mut self,
        account_id: AccountId,
        descriptor: AssetDescriptor,
    ) -> (Self, AssetId) {
        let asset_id = AssetId(self.next_asset_id);
        self.next_asset_id += 1;

        let asset = Asset {
            asset_id,
            asset_class: descriptor.asset_class,
            initial_value: descriptor.initial_value,
            return_profile_index: descriptor.return_profile_index,
        };

        // Find the account and add the asset
        if let Some(account) = self
            .params
            .accounts
            .iter_mut()
            .find(|a| a.account_id == account_id)
        {
            account.assets.push(asset);
        }

        self.metadata.assets.insert(
            asset_id,
            EntityMetadata {
                name: descriptor.name,
                description: descriptor.description,
            },
        );

        (self, asset_id)
    }

    /// Add a cash flow using a descriptor
    pub fn add_cash_flow(mut self, descriptor: CashFlowDescriptor) -> (Self, CashFlowId) {
        let cash_flow_id = CashFlowId(self.next_cash_flow_id);
        self.next_cash_flow_id += 1;

        let cash_flow = CashFlow {
            cash_flow_id,
            amount: descriptor.amount,
            repeats: descriptor.repeats,
            cash_flow_limits: descriptor.limits,
            adjust_for_inflation: descriptor.adjust_for_inflation,
            direction: descriptor.direction,
            state: descriptor.state,
        };

        self.params.cash_flows.push(cash_flow);
        self.metadata.cash_flows.insert(
            cash_flow_id,
            EntityMetadata {
                name: descriptor.name,
                description: descriptor.description,
            },
        );

        (self, cash_flow_id)
    }

    /// Add an event using a descriptor
    pub fn add_event(mut self, descriptor: EventDescriptor) -> (Self, EventId) {
        let event_id = EventId(self.next_event_id);
        self.next_event_id += 1;

        let event = Event {
            event_id,
            trigger: descriptor.trigger,
            effects: descriptor.effects,
            once: descriptor.once,
        };

        self.params.events.push(event);
        self.metadata.events.insert(
            event_id,
            EntityMetadata {
                name: descriptor.name,
                description: descriptor.description,
            },
        );

        (self, event_id)
    }

    /// Add a spending target using a descriptor
    pub fn add_spending_target(
        mut self,
        descriptor: SpendingTargetDescriptor,
    ) -> (Self, SpendingTargetId) {
        let spending_target_id = SpendingTargetId(self.next_spending_target_id);
        self.next_spending_target_id += 1;

        let spending_target = SpendingTarget {
            spending_target_id,
            amount: descriptor.amount,
            repeats: descriptor.repeats,
            net_amount_mode: descriptor.net_amount_mode,
            adjust_for_inflation: descriptor.adjust_for_inflation,
            withdrawal_strategy: descriptor.withdrawal_strategy,
            exclude_accounts: Vec::new(),
            state: descriptor.state,
        };

        self.params.spending_targets.push(spending_target);
        self.metadata.spending_targets.insert(
            spending_target_id,
            EntityMetadata {
                name: descriptor.name,
                description: descriptor.description,
            },
        );

        (self, spending_target_id)
    }

    /// Build and return the simulation parameters and metadata
    pub fn build(self) -> (SimulationParameters, SimulationMetadata) {
        (self.params, self.metadata)
    }
}
