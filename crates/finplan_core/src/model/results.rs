//! Simulation results and snapshots
//!
//! Contains the output types from running simulations, including
//! account snapshots and the immutable ledger of state changes.

use crate::model::{AccountSnapshot, AccountSnapshotFlavor};

use super::ids::{AccountId, AssetId, EventId};
use super::state_event::{LedgerEntry, StateEvent};
use super::tax_config::TaxSummary;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WealthSnapshot {
    pub date: jiff::civil::Date,
    pub accounts: Vec<AccountSnapshot>,
}

/// Yearly summary of cash flows by category
///
/// This pre-aggregates cash flows by their semantic purpose, so consumers
/// don't need to trace ledger entries back to their source events.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct YearlyCashFlowSummary {
    pub year: i16,
    /// True income (salary, dividends, rental income, etc.)
    pub income: f64,
    /// True expenses (bills, purchases, etc.)
    pub expenses: f64,
    /// Contributions to investment accounts (401k, IRA deposits)
    pub contributions: f64,
    /// Withdrawals from investments (Sweep, liquidations)
    pub withdrawals: f64,
    /// Interest/appreciation on cash balances
    pub appreciation: f64,
    /// Net cash flow (income - expenses + appreciation)
    pub net_cash_flow: f64,
}

/// Complete results from a single simulation run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    /// Starting state of all accounts
    pub wealth_snapshots: Vec<WealthSnapshot>,
    /// Tax summaries per year
    pub yearly_taxes: Vec<TaxSummary>,
    /// Cash flow summaries per year
    #[serde(default)]
    pub yearly_cash_flows: Vec<YearlyCashFlowSummary>,
    /// Immutable ledger of all state changes in chronological order
    pub ledger: Vec<LedgerEntry>,
}

impl SimulationResult {
    /// Get the final balance for a specific account
    /// Uses pre-computed final balances from the simulation
    pub fn final_account_balance(&self, account_id: AccountId) -> Option<f64> {
        self.wealth_snapshots.last().and_then(|snapshot| {
            snapshot.accounts.iter().find_map(|acc_snap| {
                if acc_snap.account_id == account_id {
                    Some(acc_snap.total_value())
                } else {
                    None
                }
            })
        })
    }

    /// Get the final balance for a specific asset
    /// Uses pre-computed final asset balances from the simulation
    pub fn final_asset_balance(&self, account_id: AccountId, asset_id: AssetId) -> Option<f64> {
        self.wealth_snapshots.last().and_then(|snapshot| {
            snapshot.accounts.iter().find_map(|acc_snap| {
                if acc_snap.account_id != account_id {
                    return None;
                }

                if let AccountSnapshotFlavor::Investment { assets, .. } = &acc_snap.flavor {
                    assets.get(&asset_id).copied()
                } else {
                    None
                }
            })
        })
    }

    pub fn yearly_net_worth(&self) -> Vec<(jiff::civil::Date, f64)> {
        self.wealth_snapshots
            .iter()
            // Get only year-end snapshots (December 31)
            .filter(|snap| snap.date.month() == 12 && snap.date.day() == snap.date.days_in_month())
            .map(|snapshot| {
                let total = snapshot
                    .accounts
                    .iter()
                    .map(|acc_snap| acc_snap.total_value())
                    .sum();
                (snapshot.date, total)
            })
            .collect()
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
/// DEPRECATED: Use MonteCarloSummary with monte_carlo_simulate_with_config for memory efficiency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub iterations: Vec<SimulationResult>,
}

// ============================================================================
// Memory-efficient Monte Carlo types
// ============================================================================

/// Configuration for Monte Carlo simulation
#[derive(Debug, Clone)]
pub struct MonteCarloConfig {
    /// Number of iterations to run
    pub iterations: usize,
    /// Percentiles to keep (e.g., [0.05, 0.50, 0.95])
    /// Sorted ascending internally
    pub percentiles: Vec<f64>,
    /// Whether to compute mean values across all iterations
    pub compute_mean: bool,
}

impl Default for MonteCarloConfig {
    fn default() -> Self {
        Self {
            iterations: 1000,
            percentiles: vec![0.05, 0.50, 0.95],
            compute_mean: true,
        }
    }
}

/// Aggregate statistics from Monte Carlo simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloStats {
    pub num_iterations: usize,
    /// Fraction of runs with positive final net worth
    pub success_rate: f64,
    pub mean_final_net_worth: f64,
    pub std_dev_final_net_worth: f64,
    pub min_final_net_worth: f64,
    pub max_final_net_worth: f64,
    /// Final net worth at each requested percentile
    pub percentile_values: Vec<(f64, f64)>, // (percentile, value)
}

/// Accumulator for computing mean wealth snapshots across iterations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeanAccumulator {
    /// For each snapshot index, for each account index: sum of total values
    pub account_sums: Vec<Vec<f64>>,
    /// Template snapshot dates (from first iteration)
    pub dates: Vec<jiff::civil::Date>,
    /// Template account IDs (from first iteration)
    pub account_ids: Vec<Vec<AccountId>>,
    /// Number of iterations accumulated
    pub count: usize,
}

impl SnapshotMeanAccumulator {
    /// Create a new accumulator using the first result as a template
    pub fn new(template: &SimulationResult) -> Self {
        let dates: Vec<_> = template.wealth_snapshots.iter().map(|s| s.date).collect();
        let account_ids: Vec<Vec<_>> = template
            .wealth_snapshots
            .iter()
            .map(|s| s.accounts.iter().map(|a| a.account_id).collect())
            .collect();
        let account_sums: Vec<Vec<_>> = template
            .wealth_snapshots
            .iter()
            .map(|s| vec![0.0; s.accounts.len()])
            .collect();

        Self {
            account_sums,
            dates,
            account_ids,
            count: 0,
        }
    }

    /// Add a result to the accumulator
    pub fn accumulate(&mut self, result: &SimulationResult) {
        for (snap_idx, snapshot) in result.wealth_snapshots.iter().enumerate() {
            if let Some(sums) = self.account_sums.get_mut(snap_idx) {
                for (acc_idx, acc) in snapshot.accounts.iter().enumerate() {
                    if let Some(sum) = sums.get_mut(acc_idx) {
                        *sum += acc.total_value();
                    }
                }
            }
        }
        self.count += 1;
    }

    /// Build the mean wealth snapshots
    pub fn build_mean_snapshots(&self) -> Vec<WealthSnapshot> {
        let n = self.count as f64;
        self.dates
            .iter()
            .zip(self.account_sums.iter())
            .zip(self.account_ids.iter())
            .map(|((date, sums), ids)| {
                let accounts = sums
                    .iter()
                    .zip(ids.iter())
                    .map(|(sum, id)| AccountSnapshot {
                        account_id: *id,
                        // Store as Bank with averaged total value
                        flavor: AccountSnapshotFlavor::Bank(sum / n),
                    })
                    .collect();
                WealthSnapshot {
                    date: *date,
                    accounts,
                }
            })
            .collect()
    }
}

/// Accumulator for computing mean tax summaries across iterations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxMeanAccumulator {
    /// For each tax year: (year, sum_ordinary, sum_cap_gains, sum_tax_free, sum_federal, sum_state, sum_total, sum_early_penalties)
    pub sums: Vec<(i16, f64, f64, f64, f64, f64, f64, f64)>,
    pub count: usize,
}

impl TaxMeanAccumulator {
    /// Create a new accumulator using the first result as a template
    pub fn new(template: &SimulationResult) -> Self {
        let sums = template
            .yearly_taxes
            .iter()
            .map(|t| (t.year, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0))
            .collect();
        Self { sums, count: 0 }
    }

    /// Add a result to the accumulator
    pub fn accumulate(&mut self, result: &SimulationResult) {
        for (idx, tax) in result.yearly_taxes.iter().enumerate() {
            if let Some(sums) = self.sums.get_mut(idx) {
                sums.1 += tax.ordinary_income;
                sums.2 += tax.capital_gains;
                sums.3 += tax.tax_free_withdrawals;
                sums.4 += tax.federal_tax;
                sums.5 += tax.state_tax;
                sums.6 += tax.total_tax;
                sums.7 += tax.early_withdrawal_penalties;
            }
        }
        self.count += 1;
    }

    /// Build the mean tax summaries
    pub fn build_mean_taxes(&self) -> Vec<TaxSummary> {
        let n = self.count as f64;
        self.sums
            .iter()
            .map(|(year, ord, cap, tf, fed, state, total, early_penalties)| TaxSummary {
                year: *year,
                ordinary_income: ord / n,
                capital_gains: cap / n,
                tax_free_withdrawals: tf / n,
                federal_tax: fed / n,
                state_tax: state / n,
                total_tax: total / n,
                early_withdrawal_penalties: early_penalties / n,
            })
            .collect()
    }
}

/// Accumulator for computing mean cash flow summaries across iterations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlowMeanAccumulator {
    /// For each year: (year, income, expenses, contributions, withdrawals, appreciation, net)
    pub sums: Vec<(i16, f64, f64, f64, f64, f64, f64)>,
    pub count: usize,
}

impl CashFlowMeanAccumulator {
    /// Create a new accumulator using the first result as a template
    pub fn new(template: &SimulationResult) -> Self {
        let sums = template
            .yearly_cash_flows
            .iter()
            .map(|cf| (cf.year, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0))
            .collect();
        Self { sums, count: 0 }
    }

    /// Add a result to the accumulator
    pub fn accumulate(&mut self, result: &SimulationResult) {
        for (idx, cf) in result.yearly_cash_flows.iter().enumerate() {
            if let Some(sums) = self.sums.get_mut(idx) {
                sums.1 += cf.income;
                sums.2 += cf.expenses;
                sums.3 += cf.contributions;
                sums.4 += cf.withdrawals;
                sums.5 += cf.appreciation;
                sums.6 += cf.net_cash_flow;
            }
        }
        self.count += 1;
    }

    /// Build the mean cash flow summaries
    pub fn build_mean_cash_flows(&self) -> Vec<YearlyCashFlowSummary> {
        let n = self.count as f64;
        self.sums
            .iter()
            .map(|(year, inc, exp, cont, wd, appr, net)| YearlyCashFlowSummary {
                year: *year,
                income: inc / n,
                expenses: exp / n,
                contributions: cont / n,
                withdrawals: wd / n,
                appreciation: appr / n,
                net_cash_flow: net / n,
            })
            .collect()
    }
}

/// Accumulators for computing mean values (used to build synthetic mean result)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeanAccumulators {
    pub snapshots: SnapshotMeanAccumulator,
    pub taxes: TaxMeanAccumulator,
    pub cash_flows: CashFlowMeanAccumulator,
}

impl MeanAccumulators {
    pub fn new(template: &SimulationResult) -> Self {
        Self {
            snapshots: SnapshotMeanAccumulator::new(template),
            taxes: TaxMeanAccumulator::new(template),
            cash_flows: CashFlowMeanAccumulator::new(template),
        }
    }

    pub fn accumulate(&mut self, result: &SimulationResult) {
        self.snapshots.accumulate(result);
        self.taxes.accumulate(result);
        self.cash_flows.accumulate(result);
    }

    /// Build a synthetic SimulationResult with mean values
    pub fn build_mean_result(&self) -> SimulationResult {
        SimulationResult {
            wealth_snapshots: self.snapshots.build_mean_snapshots(),
            yearly_taxes: self.taxes.build_mean_taxes(),
            yearly_cash_flows: self.cash_flows.build_mean_cash_flows(),
            ledger: Vec::new(), // No meaningful ledger for averaged results
        }
    }
}

/// Memory-efficient Monte Carlo results
/// Contains only the requested percentile runs and mean accumulators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloSummary {
    /// Aggregate statistics
    pub stats: MonteCarloStats,
    /// Selected percentile runs: (percentile, result)
    pub percentile_runs: Vec<(f64, SimulationResult)>,
    /// Accumulators for computing mean (if requested)
    pub mean_accumulators: Option<MeanAccumulators>,
}

impl MonteCarloSummary {
    /// Get the result for a specific percentile (exact match)
    pub fn get_percentile(&self, percentile: f64) -> Option<&SimulationResult> {
        self.percentile_runs
            .iter()
            .find(|(p, _)| (*p - percentile).abs() < 0.001)
            .map(|(_, r)| r)
    }

    /// Get the mean result (built from accumulators)
    pub fn get_mean_result(&self) -> Option<SimulationResult> {
        self.mean_accumulators.as_ref().map(|acc| acc.build_mean_result())
    }
}

/// Helper function to calculate final net worth from a SimulationResult
pub fn final_net_worth(result: &SimulationResult) -> f64 {
    result.wealth_snapshots.last().map_or(0.0, |snap| {
        snap.accounts.iter().map(|acc| acc.total_value()).sum()
    })
}
