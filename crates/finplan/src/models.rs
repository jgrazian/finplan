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
    pub return_profile: ReturnProfile,
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
    pub amount: f64,
    pub start: Timepoint,
    pub end: Timepoint,
    pub repeats: RepeatInterval,
    pub cash_flow_limits: Option<CashFlowLimits>,
    pub adjust_for_inflation: bool,
    /// Where money comes from (None = External for backward compatibility)
    pub source: CashFlowEndpoint,
    /// Where money goes to
    pub target: CashFlowEndpoint,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CashFlowEvent {
    pub cash_flow_id: CashFlowId,
    pub amount: f64,
    pub date: jiff::civil::Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: EventId,
    pub trigger: EventTrigger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventTrigger {
    Date(jiff::civil::Date),
    TotalAccountBalance {
        account_id: AccountId,
        threshold: f64,
        above: bool, // true = trigger when balance > threshold, false = balance < threshold
    },
    AssetBalance {
        account_id: AccountId,
        asset_id: AssetId,
        threshold: f64,
        above: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationParameters {
    pub start_date: Option<jiff::civil::Date>,
    pub duration_years: usize,
    pub inflation_profile: InflationProfile,
    pub events: Vec<Event>,
    pub accounts: Vec<Account>,
    pub cash_flows: Vec<CashFlow>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimulationResult {
    pub yearly_inflation: Vec<f64>,
    pub account_histories: Vec<AccountHistory>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountHistory {
    pub account_id: AccountId,
    pub assets: Vec<AssetHistory>,
    pub dates: Vec<jiff::civil::Date>,
}

impl AccountHistory {
    pub fn values(&self) -> Vec<AccountSnapshot> {
        self.dates
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
    pub yearly_returns: Vec<f64>,
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
