//! State events - the immutable ledger of simulation state changes
//!
//! Every mutation to simulation state is represented as a StateEvent.
//! These form an immutable ledger that can be used to:
//! - Replay the simulation
//! - Audit what happened and when
//! - Debug complex scenarios
//! - Export to external systems

use super::accounts::Account;
use super::ids::{AccountId, AssetId, EventId};
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// A ledger entry recording a state change with its context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// When this event occurred
    pub date: Date,
    /// What triggered this event (if from a user-defined event)
    pub source_event: Option<EventId>,
    /// The state change
    pub event: StateEvent,
}

impl LedgerEntry {
    pub fn new(date: Date, event: StateEvent) -> Self {
        Self {
            date,
            source_event: None,
            event,
        }
    }

    pub fn with_source(date: Date, source_event: EventId, event: StateEvent) -> Self {
        Self {
            date,
            source_event: Some(source_event),
            event,
        }
    }
}

/// All possible state mutations in the simulation
///
/// This enum represents every way the simulation state can change.
/// Each variant is a complete description of the mutation, allowing
/// for replay and auditing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StateEvent {
    // === Time Management ===
    /// Advance simulation time to a new date
    TimeAdvance {
        from_date: Date,
        to_date: Date,
        days_elapsed: i32,
    },

    // === Account Management ===
    /// Create a new account
    CreateAccount(Account),

    /// Delete an account
    DeleteAccount(AccountId),

    // === Cash Operations ===
    /// Add cash to an account (income, transfer in, etc.)
    CashCredit {
        to: AccountId,
        amount: f64,
    },

    /// Remove cash from an account (expense, transfer out, etc.)
    CashDebit {
        from: AccountId,
        amount: f64,
    },

    /// Cash appreciation from interest/returns (HYSA, money market, etc.)
    CashAppreciation {
        account_id: AccountId,
        previous_value: f64,
        new_value: f64,
        return_rate: f64,
        days: i32,
    },

    // === Asset Operations ===
    /// Add units to an asset position (purchase)
    AssetPurchase {
        account_id: AccountId,
        asset_id: AssetId,
        units: f64,
        cost_basis: f64,
        price_per_unit: f64,
    },

    /// Remove units from an asset position (sale)
    AssetSale {
        account_id: AccountId,
        asset_id: AssetId,
        lot_date: Date,
        units: f64,
        cost_basis: f64,
        proceeds: f64,
        short_term_gain: f64,
        long_term_gain: f64,
    },

    // === Tax Events ===
    /// Ordinary income tax incurred
    IncomeTax {
        gross_amount: f64,
        federal_tax: f64,
        state_tax: f64,
    },

    /// Short-term capital gains tax incurred
    ShortTermCapitalGainsTax {
        gross_gain: f64,
        federal_tax: f64,
        state_tax: f64,
    },

    /// Long-term capital gains tax incurred
    LongTermCapitalGainsTax {
        gross_gain: f64,
        federal_tax: f64,
        state_tax: f64,
    },

    // === Event Management ===
    /// A user-defined event was triggered
    EventTriggered {
        event_id: EventId,
    },

    /// A repeating event was paused
    EventPaused {
        event_id: EventId,
    },

    /// A paused event was resumed
    EventResumed {
        event_id: EventId,
    },

    /// An event was permanently terminated
    EventTerminated {
        event_id: EventId,
    },

    // === Year-End Events ===
    /// Year rollover for tax purposes
    YearRollover {
        from_year: i16,
        to_year: i16,
    },

    /// RMD calculation and withdrawal
    RmdWithdrawal {
        account_id: AccountId,
        age: u8,
        prior_year_balance: f64,
        divisor: f64,
        required_amount: f64,
        actual_amount: f64,
    },
}

impl StateEvent {
    /// Check if this is a time-related event
    pub fn is_time_event(&self) -> bool {
        matches!(self, StateEvent::TimeAdvance { .. } | StateEvent::YearRollover { .. })
    }

    /// Check if this is a cash operation
    pub fn is_cash_event(&self) -> bool {
        matches!(
            self,
            StateEvent::CashCredit { .. }
                | StateEvent::CashDebit { .. }
                | StateEvent::CashAppreciation { .. }
        )
    }

    /// Check if this is an asset operation
    pub fn is_asset_event(&self) -> bool {
        matches!(
            self,
            StateEvent::AssetPurchase { .. } | StateEvent::AssetSale { .. }
        )
    }

    /// Check if this is a tax event
    pub fn is_tax_event(&self) -> bool {
        matches!(
            self,
            StateEvent::IncomeTax { .. }
                | StateEvent::ShortTermCapitalGainsTax { .. }
                | StateEvent::LongTermCapitalGainsTax { .. }
        )
    }

    /// Check if this is an event management event
    pub fn is_event_management(&self) -> bool {
        matches!(
            self,
            StateEvent::EventTriggered { .. }
                | StateEvent::EventPaused { .. }
                | StateEvent::EventResumed { .. }
                | StateEvent::EventTerminated { .. }
        )
    }

    /// Get the account ID if this event affects a specific account
    pub fn account_id(&self) -> Option<AccountId> {
        match self {
            StateEvent::CreateAccount(account) => Some(account.account_id),
            StateEvent::DeleteAccount(id) => Some(*id),
            StateEvent::CashCredit { to, .. } => Some(*to),
            StateEvent::CashDebit { from, .. } => Some(*from),
            StateEvent::CashAppreciation { account_id, .. } => Some(*account_id),
            StateEvent::AssetPurchase { account_id, .. } => Some(*account_id),
            StateEvent::AssetSale { account_id, .. } => Some(*account_id),
            StateEvent::RmdWithdrawal { account_id, .. } => Some(*account_id),
            _ => None,
        }
    }

    /// Get the event ID if this is related to a user-defined event
    pub fn event_id(&self) -> Option<EventId> {
        match self {
            StateEvent::EventTriggered { event_id } => Some(*event_id),
            StateEvent::EventPaused { event_id } => Some(*event_id),
            StateEvent::EventResumed { event_id } => Some(*event_id),
            StateEvent::EventTerminated { event_id } => Some(*event_id),
            _ => None,
        }
    }
}
