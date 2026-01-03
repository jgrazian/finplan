//! Transaction records for history tracking
//!
//! These records capture every mutation to simulation state, allowing
//! full replay and analysis of what happened during a simulation.
//!
//! All records are stored in a single unified `Record` type with a `RecordKind`
//! enum to distinguish between different transaction types. This design:
//! - Maintains natural chronological ordering in a single Vec
//! - Makes it easy to filter by type using pattern matching
//! - Is extensible - new record types just add enum variants

use super::events::{LotMethod, WithdrawalAmountMode};
use super::ids::{AccountId, AssetId, EventId};
use serde::{Deserialize, Serialize};

/// A single record entry representing any transaction/event in the simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub date: jiff::civil::Date,
    pub kind: RecordKind,
}

impl Record {
    pub fn new(date: jiff::civil::Date, kind: RecordKind) -> Self {
        Self { date, kind }
    }

    /// Create a return record
    pub fn investment_return(
        date: jiff::civil::Date,
        account_id: AccountId,
        asset_id: AssetId,
        balance_before: f64,
        return_rate: f64,
        return_amount: f64,
    ) -> Self {
        Self::new(
            date,
            RecordKind::Return {
                account_id,
                asset_id,
                balance_before,
                return_rate,
                return_amount,
            },
        )
    }

    /// Create an event record
    pub fn event(date: jiff::civil::Date, event_id: EventId) -> Self {
        Self::new(date, RecordKind::Event { event_id })
    }
}

/// What triggered a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionSource {
    /// User-defined event
    Event(EventId),
    /// Sweep withdrawal
    Sweep {
        event_id: EventId,
        target_amount: f64,
        amount_mode: WithdrawalAmountMode,
    },
    /// Required Minimum Distribution
    Rmd {
        event_id: EventId,
        age: u8,
        prior_year_balance: f64,
        irs_divisor: f64,
        required_amount: f64,
    },
}

/// Tax details for taxable transactions (liquidations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxInfo {
    pub cost_basis: f64,
    pub short_term_gain: f64,
    pub long_term_gain: f64,
    pub federal_tax: f64,
    pub state_tax: f64,
    pub lot_method: LotMethod,
}

/// The kind of transaction recorded
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RecordKind {
    /// External income received
    Income {
        to_account_id: AccountId,
        to_asset_id: AssetId,
        amount: f64,
        event_id: EventId,
    },
    /// External expense paid
    Expense {
        from_account_id: AccountId,
        from_asset_id: AssetId,
        amount: f64,
        event_id: EventId,
    },
    /// Movement between accounts (with optional tax implications)
    Transfer {
        from_account_id: AccountId,
        from_asset_id: AssetId,
        to_account_id: AccountId,
        to_asset_id: AssetId,
        gross_amount: f64,
        net_amount: f64,
        tax_info: Option<Box<TaxInfo>>, // Some = liquidation, None = simple transfer
        source: Box<TransactionSource>,
    },
    /// Investment return applied
    Return {
        account_id: AccountId,
        asset_id: AssetId,
        balance_before: f64,
        return_rate: f64,
        return_amount: f64,
    },
    /// Event triggered
    Event { event_id: EventId },
}
