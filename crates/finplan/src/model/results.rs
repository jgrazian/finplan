//! Simulation results and snapshots
//!
//! Contains the output types from running simulations, including
//! account snapshots and transaction histories.

use super::accounts::AccountType;
use super::ids::{AccountId, AssetId, EventId};
use super::records::{Record, RecordKind};
use super::tax_config::TaxSummary;
use serde::{Deserialize, Serialize};

/// Snapshot of an asset's starting state
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AssetSnapshot {
    pub asset_id: AssetId,
    pub return_profile_index: usize,
    pub starting_value: f64,
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

/// Complete results from a single simulation run
#[derive(Debug, Serialize, Deserialize)]
pub struct SimulationResult {
    pub yearly_inflation: Vec<f64>,
    pub dates: Vec<jiff::civil::Date>,
    pub return_profile_returns: Vec<Vec<f64>>,
    /// Starting state of all accounts (replay from transaction logs to get future values)
    pub accounts: Vec<AccountSnapshot>,
    /// Tax summaries per year
    pub yearly_taxes: Vec<TaxSummary>,

    /// Unified transaction log in chronological order
    pub records: Vec<Record>,
}

impl SimulationResult {
    /// Calculate the final balance for a specific account by replaying transaction logs
    pub fn final_account_balance(&self, account_id: AccountId) -> f64 {
        // Start with initial values
        let account = self.accounts.iter().find(|a| a.account_id == account_id);
        let mut balance: f64 = account.map(|a| a.starting_balance()).unwrap_or(0.0);

        for record in &self.records {
            match &record.kind {
                RecordKind::Return {
                    account_id: acc_id,
                    return_amount,
                    ..
                } if *acc_id == account_id => {
                    balance += return_amount;
                }
                RecordKind::Transfer {
                    from_account_id,
                    to_account_id,
                    amount,
                    ..
                } => {
                    if *from_account_id == account_id {
                        balance -= amount;
                    }
                    if *to_account_id == account_id {
                        balance += amount;
                    }
                }
                RecordKind::Liquidation {
                    from_account_id,
                    to_account_id,
                    gross_amount,
                    net_proceeds,
                    ..
                } => {
                    // Source account loses gross amount
                    if *from_account_id == account_id {
                        balance -= gross_amount;
                    }
                    // Target account gains net proceeds (after taxes)
                    if *to_account_id == account_id {
                        balance += net_proceeds;
                    }
                }
                _ => {}
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

        for record in &self.records {
            match &record.kind {
                RecordKind::Return {
                    account_id: acc_id,
                    asset_id: ass_id,
                    return_amount,
                    ..
                } if *acc_id == account_id && *ass_id == asset_id => {
                    balance += return_amount;
                }
                RecordKind::Transfer {
                    from_account_id,
                    from_asset_id,
                    to_account_id,
                    to_asset_id,
                    amount,
                    ..
                } => {
                    if *from_account_id == account_id && *from_asset_id == asset_id {
                        balance -= amount;
                    }
                    if *to_account_id == account_id && *to_asset_id == asset_id {
                        balance += amount;
                    }
                }
                RecordKind::Liquidation {
                    from_account_id,
                    from_asset_id,
                    to_account_id,
                    to_asset_id,
                    gross_amount,
                    net_proceeds,
                    ..
                } => {
                    // Source asset loses gross amount
                    if *from_account_id == account_id && *from_asset_id == asset_id {
                        balance -= gross_amount;
                    }
                    // Target asset gains net proceeds (after taxes)
                    if *to_account_id == account_id && *to_asset_id == asset_id {
                        balance += net_proceeds;
                    }
                }
                _ => {}
            }
        }

        balance
    }

    /// Check if an event was triggered at any point
    pub fn event_was_triggered(&self, event_id: EventId) -> bool {
        self.records
            .iter()
            .any(|r| matches!(&r.kind, RecordKind::Event { event_id: eid } if *eid == event_id))
    }

    /// Get the date when an event was first triggered
    pub fn event_trigger_date(&self, event_id: EventId) -> Option<jiff::civil::Date> {
        self.records.iter().find_map(|r| {
            if let RecordKind::Event { event_id: eid } = &r.kind
                && *eid == event_id
            {
                return Some(r.date);
            }
            None
        })
    }

    // === Helper methods to filter records by type ===

    /// Get all return records
    pub fn return_records(&self) -> impl Iterator<Item = &Record> {
        self.records
            .iter()
            .filter(|r| matches!(r.kind, RecordKind::Return { .. }))
    }

    /// Get all transfer records
    pub fn transfer_records(&self) -> impl Iterator<Item = &Record> {
        self.records
            .iter()
            .filter(|r| matches!(r.kind, RecordKind::Transfer { .. }))
    }

    /// Get all event records
    pub fn event_records(&self) -> impl Iterator<Item = &Record> {
        self.records
            .iter()
            .filter(|r| matches!(r.kind, RecordKind::Event { .. }))
    }

    /// Get all RMD records
    pub fn rmd_records(&self) -> impl Iterator<Item = &Record> {
        self.records
            .iter()
            .filter(|r| matches!(r.kind, RecordKind::Rmd { .. }))
    }

    /// Get all liquidation records
    pub fn liquidation_records(&self) -> impl Iterator<Item = &Record> {
        self.records
            .iter()
            .filter(|r| matches!(r.kind, RecordKind::Liquidation { .. }))
    }
}

/// Results from Monte Carlo simulation (multiple runs)
#[derive(Debug, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub iterations: Vec<SimulationResult>,
}
