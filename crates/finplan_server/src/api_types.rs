//! REST API-friendly types for simulation configuration
//!
//! These types use names instead of IDs and are designed for JSON serialization.
//! The server converts these to SimulationConfig using SimulationBuilder.
//!
//! TypeScript types are automatically generated using ts-rs.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// Re-export types from finplan that are already serializable
use finplan::model::{
    ContributionLimit, IncomeType, InflationProfile, LotMethod, RepeatInterval, ReturnProfile,
    TaxStatus, WithdrawalOrder,
};

/// API request to create a simulation (name-based, no IDs required)
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SimulationRequest {
    /// Simulation name
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    // === Timeline ===
    /// Start date (YYYY-MM-DD)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    /// Number of years to simulate
    pub duration_years: usize,
    /// Birth date for age-based triggers (YYYY-MM-DD)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub birth_date: Option<String>,

    // === World Assumptions ===
    /// Named return profiles (referenced by name in assets)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub return_profiles: Vec<NamedReturnProfileDef>,
    /// Inflation model
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inflation_profile: Option<InflationProfile>,
    /// Tax configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tax_config: Option<TaxConfigDef>,

    // === Portfolio ===
    /// Account definitions (name-based)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<AccountDef>,
    /// Asset definitions (name-based)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<AssetDef>,
    /// Initial positions (references accounts/assets by name)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub positions: Vec<PositionDef>,

    // === Events ===
    /// Event definitions (name-based references)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<EventDef>,
}

/// Tax configuration definition (simplified wrapper)
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaxConfigDef {
    /// Federal standard deduction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standard_deduction: Option<f64>,
    /// Capital gains rate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capital_gains_rate: Option<f64>,
}

/// Named return profile definition
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NamedReturnProfileDef {
    pub name: String,
    pub profile: ReturnProfile,
}

/// Account definition using preset types or custom configuration
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AccountDef {
    /// Unique name for this account
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Account type (determines tax treatment)
    pub account_type: AccountTypeDef,
    /// Initial cash balance
    #[serde(default)]
    pub cash: f64,
    /// Return profile for cash (name reference or inline)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cash_return_profile: Option<ReturnProfileRef>,
}

/// Account type presets (maps to AccountBuilder methods)
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type")]
pub enum AccountTypeDef {
    /// Bank account (checking/savings)
    Bank,
    /// Taxable brokerage
    TaxableBrokerage,
    /// Traditional 401(k)
    Traditional401k {
        #[serde(skip_serializing_if = "Option::is_none")]
        contribution_limit: Option<ContributionLimit>,
    },
    /// Roth 401(k)
    Roth401k {
        #[serde(skip_serializing_if = "Option::is_none")]
        contribution_limit: Option<ContributionLimit>,
    },
    /// Traditional IRA
    TraditionalIra {
        #[serde(skip_serializing_if = "Option::is_none")]
        contribution_limit: Option<ContributionLimit>,
    },
    /// Roth IRA
    RothIra {
        #[serde(skip_serializing_if = "Option::is_none")]
        contribution_limit: Option<ContributionLimit>,
    },
    /// HSA
    Hsa {
        #[serde(skip_serializing_if = "Option::is_none")]
        contribution_limit: Option<ContributionLimit>,
    },
    /// Custom account
    Custom {
        tax_status: TaxStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        contribution_limit: Option<ContributionLimit>,
    },
}

/// Asset definition
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssetDef {
    /// Unique name for this asset
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Initial price per unit
    pub price: f64,
    /// Return profile (name reference or inline definition)
    pub return_profile: ReturnProfileRef,
}

/// Reference to a return profile (by name or inline)
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(untagged)]
pub enum ReturnProfileRef {
    /// Reference existing named profile
    Named(String),
    /// Inline profile definition
    Inline(ReturnProfile),
}

/// Position (asset holding in an account)
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PositionDef {
    /// Account name (must match an AccountDef.name)
    pub account: String,
    /// Asset name (must match an AssetDef.name)
    pub asset: String,
    /// Number of units
    pub units: f64,
    /// Cost basis (total, not per unit)
    pub cost_basis: f64,
    /// Purchase date (YYYY-MM-DD), defaults to start_date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purchase_date: Option<String>,
}

/// Event definition
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventDef {
    /// Event name
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// When the event triggers
    pub trigger: TriggerDef,
    /// What happens when triggered
    pub effects: Vec<EffectDef>,
    /// Whether this event fires only once
    #[serde(default)]
    pub once: bool,
}

/// Event trigger definition
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type")]
pub enum TriggerDef {
    /// Triggers immediately at simulation start
    Immediate,
    /// Triggers on a specific date
    Date { date: String },
    /// Triggers at a specific age (years and optionally months)
    Age {
        years: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        months: Option<u8>,
    },
    /// Triggers on a schedule
    Repeating {
        interval: RepeatInterval,
        #[serde(skip_serializing_if = "Option::is_none")]
        start: Option<Box<TriggerStartDef>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        end: Option<Box<TriggerEndDef>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type")]
pub enum TriggerStartDef {
    Date { date: String },
    Age { years: u8, months: Option<u8> },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type")]
pub enum TriggerEndDef {
    Date { date: String },
    Age { years: u8, months: Option<u8> },
    Never,
}

/// Event effect definition
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type")]
pub enum EffectDef {
    /// Income deposited to an account
    Income {
        amount: f64,
        to_account: String,
        #[serde(default = "default_income_type")]
        income_type: IncomeType,
        #[serde(default)]
        gross: bool,
        #[serde(default)]
        adjust_for_inflation: bool,
    },
    /// Expense withdrawn from account(s)
    Expense {
        amount: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        from_account: Option<String>,
        #[serde(default)]
        adjust_for_inflation: bool,
    },
    /// Purchase an asset
    AssetPurchase {
        amount: f64,
        account: String,
        asset: String,
        #[serde(default)]
        adjust_for_inflation: bool,
    },
    /// Sell an asset or withdraw from portfolio
    Withdrawal {
        amount: AmountDef,
        to_account: String,
        #[serde(default)]
        source: WithdrawalSourceDef,
        #[serde(default)]
        gross: bool,
        #[serde(default = "default_lot_method")]
        lot_method: LotMethod,
    },
}

fn default_income_type() -> IncomeType {
    IncomeType::Taxable
}

fn default_lot_method() -> LotMethod {
    LotMethod::Fifo
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type")]
pub enum AmountDef {
    Fixed { value: f64 },
    Percent { value: f64 },
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type")]
pub enum WithdrawalSourceDef {
    /// Use a strategy to select accounts
    Strategy {
        #[serde(default)]
        order: WithdrawalOrder,
        #[serde(default)]
        exclude: Vec<String>,
    },
    /// Specific order of accounts to withdraw from
    AccountOrder { accounts: Vec<String> },
    /// Single specific asset
    Asset { account: String, asset: String },
}

impl Default for WithdrawalSourceDef {
    fn default() -> Self {
        WithdrawalSourceDef::Strategy {
            order: WithdrawalOrder::TaxEfficientEarly,
            exclude: Vec::new(),
        }
    }
}
