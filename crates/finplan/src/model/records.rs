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

    /// Create a transfer record
    pub fn transfer(
        date: jiff::civil::Date,
        from_account_id: AccountId,
        from_asset_id: AssetId,
        to_account_id: AccountId,
        to_asset_id: AssetId,
        amount: f64,
        event_id: EventId,
    ) -> Self {
        Self::new(
            date,
            RecordKind::Transfer {
                from_account_id,
                from_asset_id,
                to_account_id,
                to_asset_id,
                amount,
                event_id,
            },
        )
    }

    /// Create an event record
    pub fn event(date: jiff::civil::Date, event_id: EventId) -> Self {
        Self::new(date, RecordKind::Event { event_id })
    }

    /// Create an RMD record
    #[allow(clippy::too_many_arguments)]
    pub fn rmd(
        date: jiff::civil::Date,
        account_id: AccountId,
        age: u8,
        prior_year_balance: f64,
        irs_divisor: f64,
        required_amount: f64,
        actual_withdrawn: f64,
    ) -> Self {
        Self::new(
            date,
            RecordKind::Rmd {
                account_id,
                age,
                prior_year_balance,
                irs_divisor,
                required_amount,
                actual_withdrawn,
            },
        )
    }

    /// Create a liquidation record (selling assets to fund another account)
    #[allow(clippy::too_many_arguments)]
    pub fn liquidation(
        date: jiff::civil::Date,
        from_account_id: AccountId,
        from_asset_id: AssetId,
        to_account_id: AccountId,
        to_asset_id: AssetId,
        gross_amount: f64,
        cost_basis: f64,
        short_term_gain: f64,
        long_term_gain: f64,
        federal_tax: f64,
        state_tax: f64,
        net_proceeds: f64,
        lot_method: LotMethod,
        event_id: EventId,
    ) -> Self {
        Self::new(
            date,
            RecordKind::Liquidation {
                from_account_id,
                from_asset_id,
                to_account_id,
                to_asset_id,
                gross_amount,
                cost_basis,
                short_term_gain,
                long_term_gain,
                federal_tax,
                state_tax,
                net_proceeds,
                lot_method,
                event_id,
            },
        )
    }
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

    /// Internal transfer between assets (no tax implications tracked here)
    Transfer {
        from_account_id: AccountId,
        from_asset_id: AssetId,
        to_account_id: AccountId,
        to_asset_id: AssetId,
        amount: f64,
        event_id: EventId,
    },

    /// Liquidation with capital gains
    Liquidation {
        from_account_id: AccountId,
        from_asset_id: AssetId,
        to_account_id: AccountId,
        to_asset_id: AssetId,
        gross_amount: f64,
        cost_basis: f64,
        short_term_gain: f64,
        long_term_gain: f64,
        federal_tax: f64,
        state_tax: f64,
        net_proceeds: f64,
        lot_method: LotMethod,
        event_id: EventId,
    },

    /// Sweep withdrawal (may include multiple liquidations)
    Sweep {
        to_account_id: AccountId,
        to_asset_id: AssetId,
        target_amount: f64,
        actual_gross: f64,
        actual_net: f64,
        amount_mode: WithdrawalAmountMode,
        event_id: EventId,
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

    /// RMD withdrawal
    Rmd {
        account_id: AccountId,
        age: u8,
        prior_year_balance: f64,
        irs_divisor: f64,
        required_amount: f64,
        actual_withdrawn: f64,
    },
}
