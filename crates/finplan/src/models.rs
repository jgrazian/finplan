use crate::profiles::{InflationProfile, ReturnProfile};
use jiff::ToSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountType {
    Taxable,
    TaxDeferred, // 401k, Traditional IRA
    TaxFree,     // Roth IRA
    Liability,   // Mortgages, loans
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub account_id: u64,
    pub name: String,
    pub initial_balance: f64,
    pub account_type: AccountType,
    pub return_profile: ReturnProfile,
    pub cash_flows: Vec<CashFlow>,
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
    Event(String),
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlow {
    pub cash_flow_id: u64,
    pub description: Option<String>,
    pub amount: f64,
    pub start: Timepoint,
    pub end: Timepoint,
    pub repeats: RepeatInterval,
    pub cash_flow_limits: Option<CashFlowLimits>,
    pub adjust_for_inflation: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CashFlowEvent {
    pub cash_flow_id: u64,
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
    pub name: String,
    pub trigger: EventTrigger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventTrigger {
    Date(jiff::civil::Date),
    AccountBalance {
        account_id: u64,
        threshold: f64,
        above: bool, // true = trigger when balance > threshold, false = balance < threshold
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationParameters {
    pub start_date: Option<jiff::civil::Date>,
    pub duration_years: usize,
    pub inflation_profile: InflationProfile,
    pub events: Vec<Event>,
    pub accounts: Vec<Account>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimulationResult {
    pub yearly_inflation: Vec<f64>,
    pub account_histories: Vec<AccountHistory>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountHistory {
    pub account_id: u64,
    pub yearly_returns: Vec<f64>,
    pub values: Vec<AccountSnapshot>,
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
