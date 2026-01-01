//! Simulation results and snapshots
//!
//! Contains the output types from running simulations, including
//! account snapshots and transaction histories.

use crate::accounts::AccountType;
use crate::ids::{AccountId, AssetId, EventId};
use crate::records::{
    CashFlowRecord, EventRecord, ReturnRecord, RmdRecord, TransferRecord, WithdrawalRecord,
};
use crate::tax_config::TaxSummary;
use serde::{Deserialize, Serialize};

/// Snapshot of an asset's starting state
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub rmd_history: Vec<RmdRecord>,
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

/// Results from Monte Carlo simulation (multiple runs)
#[derive(Debug, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub iterations: Vec<SimulationResult>,
}
