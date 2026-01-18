//! Simulation results and snapshots
//!
//! Contains the output types from running simulations, including
//! account snapshots and the immutable ledger of state changes.

use std::collections::HashMap;

use crate::model::accounts::AccountFlavor;

use super::ids::{AccountId, AssetId, EventId};
use super::state_event::{LedgerEntry, StateEvent};
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
    pub flavor: AccountFlavor,
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
    pub dates: Vec<jiff::civil::Date>,
    /// Starting state of all accounts
    pub accounts: Vec<AccountSnapshot>,
    /// Tax summaries per year
    pub yearly_taxes: Vec<TaxSummary>,

    /// Immutable ledger of all state changes in chronological order
    pub ledger: Vec<LedgerEntry>,

    /// Final account balances at end of simulation (computed from Market prices)
    pub final_balances: HashMap<AccountId, f64>,

    /// Final asset balances at end of simulation (account_id, asset_id) -> value
    pub final_asset_balances: HashMap<(AccountId, AssetId), f64>,

    /// Net worth at end of each year (year -> net_worth)
    pub yearly_net_worth: HashMap<i16, f64>,
}

impl SimulationResult {
    /// Get the final balance for a specific account
    /// Uses pre-computed final balances from the simulation
    pub fn final_account_balance(&self, account_id: AccountId) -> f64 {
        self.final_balances.get(&account_id).copied().unwrap_or(0.0)
    }

    /// Get the final balance for a specific asset
    /// Uses pre-computed final asset balances from the simulation
    pub fn final_asset_balance(&self, account_id: AccountId, asset_id: AssetId) -> f64 {
        self.final_asset_balances
            .get(&(account_id, asset_id))
            .copied()
            .unwrap_or(0.0)
    }

    /// Check if an event was triggered at any point
    pub fn event_was_triggered(&self, event_id: EventId) -> bool {
        self.ledger
            .iter()
            .any(|entry| matches!(&entry.event, StateEvent::EventTriggered { event_id: eid } if *eid == event_id))
    }

    /// Get the date when an event was first triggered
    pub fn event_trigger_date(&self, event_id: EventId) -> Option<jiff::civil::Date> {
        self.ledger.iter().find_map(|entry| {
            if let StateEvent::EventTriggered { event_id: eid } = &entry.event
                && *eid == event_id
            {
                return Some(entry.date);
            }
            None
        })
    }

    // === Helper methods to filter ledger entries by type ===

    /// Get all cash appreciation entries
    pub fn cash_appreciation_entries(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger
            .iter()
            .filter(|e| matches!(e.event, StateEvent::CashAppreciation { .. }))
    }

    /// Get all cash credit entries
    pub fn cash_credit_entries(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger
            .iter()
            .filter(|e| matches!(e.event, StateEvent::CashCredit { .. }))
    }

    /// Get all cash debit entries
    pub fn cash_debit_entries(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger
            .iter()
            .filter(|e| matches!(e.event, StateEvent::CashDebit { .. }))
    }

    /// Get all asset purchase entries
    pub fn asset_purchase_entries(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger
            .iter()
            .filter(|e| matches!(e.event, StateEvent::AssetPurchase { .. }))
    }

    /// Get all asset sale entries
    pub fn asset_sale_entries(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger
            .iter()
            .filter(|e| matches!(e.event, StateEvent::AssetSale { .. }))
    }

    /// Get all event triggered entries
    pub fn event_triggered_entries(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger
            .iter()
            .filter(|e| matches!(e.event, StateEvent::EventTriggered { .. }))
    }

    /// Get all tax-related entries
    pub fn tax_entries(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger.iter().filter(|e| e.event.is_tax_event())
    }

    /// Get all RMD withdrawal entries
    pub fn rmd_entries(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger
            .iter()
            .filter(|e| matches!(e.event, StateEvent::RmdWithdrawal { .. }))
    }

    /// Get all entries for a specific account
    pub fn entries_for_account(&self, account_id: AccountId) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger
            .iter()
            .filter(move |e| e.event.account_id() == Some(account_id))
    }

    /// Get all entries for a specific user-defined event
    pub fn entries_for_event(&self, event_id: EventId) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger
            .iter()
            .filter(move |e| e.source_event == Some(event_id))
    }

    /// Get all time-related entries (advances and year rollovers)
    pub fn time_entries(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.ledger.iter().filter(|e| e.event.is_time_event())
    }
}

/// Results from Monte Carlo simulation (multiple runs)
#[derive(Debug, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub iterations: Vec<SimulationResult>,
}
