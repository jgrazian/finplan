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

use super::ids::{AccountId, AssetId, CashFlowId, EventId, SpendingTargetId};
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

    /// Create a cash flow record
    pub fn cash_flow(
        date: jiff::civil::Date,
        cash_flow_id: CashFlowId,
        account_id: AccountId,
        asset_id: AssetId,
        amount: f64,
    ) -> Self {
        Self::new(
            date,
            RecordKind::CashFlow {
                cash_flow_id,
                account_id,
                asset_id,
                amount,
            },
        )
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
        triggered_by: Option<EventId>,
    ) -> Self {
        Self::new(
            date,
            RecordKind::Transfer {
                from_account_id,
                from_asset_id,
                to_account_id,
                to_asset_id,
                amount,
                triggered_by,
            },
        )
    }

    /// Create an event record
    pub fn event(date: jiff::civil::Date, event_id: EventId) -> Self {
        Self::new(date, RecordKind::Event { event_id })
    }

    /// Create a withdrawal record
    #[allow(clippy::too_many_arguments)]
    pub fn withdrawal(
        date: jiff::civil::Date,
        spending_target_id: SpendingTargetId,
        account_id: AccountId,
        asset_id: AssetId,
        gross_amount: f64,
        federal_tax: f64,
        state_tax: f64,
        net_amount: f64,
    ) -> Self {
        Self::new(
            date,
            RecordKind::Withdrawal {
                spending_target_id,
                account_id,
                asset_id,
                gross_amount,
                federal_tax,
                state_tax,
                net_amount,
            },
        )
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
        spending_target_id: SpendingTargetId,
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
                spending_target_id,
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
        realized_gain: f64,
        federal_tax: f64,
        state_tax: f64,
        net_amount: f64,
        triggered_by: Option<EventId>,
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
                realized_gain,
                federal_tax,
                state_tax,
                net_amount,
                triggered_by,
            },
        )
    }
}

/// The kind of transaction recorded
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RecordKind {
    /// CashFlow execution (income or expense only, not internal transfers)
    CashFlow {
        cash_flow_id: CashFlowId,
        /// The account affected (target for income, source for expense)
        account_id: AccountId,
        /// The asset affected
        asset_id: AssetId,
        /// Positive for deposits (income), negative for withdrawals (expenses)
        amount: f64,
    },

    /// Investment return applied to an asset
    Return {
        account_id: AccountId,
        asset_id: AssetId,
        /// Balance before return was applied
        balance_before: f64,
        /// The return rate applied (can be negative for losses/debt interest)
        return_rate: f64,
        /// The dollar amount of return (balance_before * return_rate)
        return_amount: f64,
    },

    /// Transfer between assets (triggered by EventEffect::TransferAsset)
    Transfer {
        from_account_id: AccountId,
        from_asset_id: AssetId,
        to_account_id: AccountId,
        to_asset_id: AssetId,
        /// Amount transferred (always positive)
        amount: f64,
        /// The event that triggered this transfer, if any
        triggered_by: Option<EventId>,
    },

    /// Event being triggered
    Event { event_id: EventId },

    /// SpendingTarget withdrawal
    Withdrawal {
        spending_target_id: SpendingTargetId,
        account_id: AccountId,
        asset_id: AssetId,
        gross_amount: f64,
        /// Federal income tax on this withdrawal
        federal_tax: f64,
        /// State tax (income + capital gains) on this withdrawal
        state_tax: f64,
        net_amount: f64,
    },

    /// Required Minimum Distribution calculation and withdrawal
    Rmd {
        account_id: AccountId,
        age: u8,
        prior_year_balance: f64,
        irs_divisor: f64,
        required_amount: f64,
        /// Actual amount withdrawn (may differ from required if account balance is lower)
        actual_withdrawn: f64,
        spending_target_id: SpendingTargetId,
    },

    /// Liquidation: selling assets from one account to fund another (with tax implications)
    /// Used for cash sweeps, rebalancing with tax consequences, etc.
    Liquidation {
        /// Source account where assets are being sold
        from_account_id: AccountId,
        from_asset_id: AssetId,
        /// Destination account receiving net proceeds
        to_account_id: AccountId,
        to_asset_id: AssetId,
        /// Market value of assets sold
        gross_amount: f64,
        /// Original purchase price (for capital gains calculation)
        cost_basis: f64,
        /// Realized gain/loss (gross_amount - cost_basis)
        realized_gain: f64,
        /// Federal tax on the sale
        federal_tax: f64,
        /// State tax on the sale
        state_tax: f64,
        /// Net amount arriving at destination (gross - taxes)
        net_amount: f64,
        /// The event that triggered this liquidation, if any
        triggered_by: Option<EventId>,
    },
}
