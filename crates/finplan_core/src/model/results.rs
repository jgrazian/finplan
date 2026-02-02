//! Simulation results and snapshots
//!
//! Contains the output types from running simulations, including
//! account snapshots and the immutable ledger of state changes.

use crate::model::{AccountSnapshot, AccountSnapshotFlavor};

use super::ids::{AccountId, AssetId, EventId};
use super::state_event::{LedgerEntry, StateEvent};
use super::tax_config::TaxSummary;
use serde::{Deserialize, Serialize};

/// A warning generated during simulation execution
///
/// Warnings are non-fatal issues that occurred during simulation but did not
/// prevent the simulation from completing. Consumers should inspect warnings
/// to understand if any events were skipped or other issues occurred.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimulationWarning {
    /// Date when the warning occurred
    pub date: jiff::civil::Date,
    /// Event that was being processed when the warning occurred (if any)
    pub event_id: Option<EventId>,
    /// Human-readable description of the warning
    pub message: String,
    /// Category of the warning
    pub kind: WarningKind,
}

/// Categories of simulation warnings
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum WarningKind {
    /// An effect was skipped due to an error during application
    #[default]
    EffectSkipped,
    /// An effect evaluation failed
    EvaluationFailed,
    /// Iteration limit was hit, indicating a possible infinite loop
    IterationLimitHit,
}

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
    /// Non-fatal warnings encountered during simulation
    #[serde(default)]
    pub warnings: Vec<SimulationWarning>,
    /// Cumulative inflation factors for each year (index 0 = 1.0 for today's dollars)
    /// Used to convert nominal values to real (inflation-adjusted) values
    #[serde(default)]
    pub cumulative_inflation: Vec<f64>,
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

/// Metric to use for convergence checking
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConvergenceMetric {
    /// Track relative standard error of the mean (SEM / |mean|).
    /// Note: For skewed distributions (common with compound returns), the mean
    /// can drift upward as more extreme positive outcomes are sampled.
    Mean,
    /// Track stability of the median (P50). More robust for skewed distributions.
    #[default]
    Median,
    /// Track stability of the success rate (% runs with positive final net worth).
    /// Often the most meaningful metric for retirement planning.
    SuccessRate,
    /// Track stability of P5, P50, and P95 percentiles simultaneously.
    /// Converges when all three percentiles are stable.
    Percentiles,
}

impl ConvergenceMetric {
    /// Get display name for the metric
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Mean => "Mean",
            Self::Median => "Median",
            Self::SuccessRate => "Success Rate",
            Self::Percentiles => "Percentiles",
        }
    }

    /// Get short name for compact display
    pub fn short_name(&self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Median => "p50",
            Self::SuccessRate => "success",
            Self::Percentiles => "pctl",
        }
    }
}

/// Configuration for convergence-based stopping
#[derive(Debug, Clone)]
pub struct ConvergenceConfig {
    /// The metric to track for convergence
    pub metric: ConvergenceMetric,
    /// Stop when the metric changes by less than this threshold between batches.
    /// For Mean: relative standard error (SEM / |mean|)
    /// For Median/SuccessRate/Percentiles: relative change between batches
    pub relative_threshold: f64,
    /// Maximum iterations (cap to prevent infinite runs)
    pub max_iterations: usize,
}

impl Default for ConvergenceConfig {
    fn default() -> Self {
        Self {
            metric: ConvergenceMetric::default(),
            relative_threshold: 0.01, // 1% precision
            max_iterations: 10_000,
        }
    }
}

/// Configuration for Monte Carlo simulation
#[derive(Debug, Clone)]
pub struct MonteCarloConfig {
    /// Number of iterations to run (fixed mode), or minimum iterations before
    /// checking convergence (convergence mode)
    pub iterations: usize,
    /// Percentiles to keep (e.g., [0.05, 0.50, 0.95])
    /// Sorted ascending internally
    pub percentiles: Vec<f64>,
    /// Whether to compute mean values across all iterations
    pub compute_mean: bool,
    /// If set, use convergence-based stopping instead of fixed iterations.
    /// The simulation will run at least `iterations` iterations, then continue
    /// until convergence is achieved or `max_iterations` is reached.
    pub convergence: Option<ConvergenceConfig>,
    /// Number of simulations to run per batch. Each batch runs sequentially
    /// within a single thread. Defaults to 100.
    pub batch_size: usize,
    /// Number of batches to run in parallel per round. Higher values use more
    /// parallelism but may increase memory usage. Defaults to 4.
    pub parallel_batches: usize,
}

impl Default for MonteCarloConfig {
    fn default() -> Self {
        Self {
            iterations: 1000,
            percentiles: vec![0.05, 0.50, 0.95],
            compute_mean: true,
            convergence: None,
            batch_size: 100,
            parallel_batches: 4,
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
    /// If convergence mode was used, indicates whether convergence was achieved.
    /// None if fixed iteration mode was used.
    #[serde(default)]
    pub converged: Option<bool>,
    /// The convergence metric used (if any)
    #[serde(default)]
    pub convergence_metric: Option<ConvergenceMetric>,
    /// The final convergence metric value when simulation stopped.
    /// For Mean: relative standard error (SEM / |mean|)
    /// For Median/SuccessRate/Percentiles: relative change from previous batch
    #[serde(default)]
    pub convergence_value: Option<f64>,
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

/// Running sums for tax accumulation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TaxSums {
    year: i16,
    ordinary_income: f64,
    capital_gains: f64,
    tax_free_withdrawals: f64,
    federal_tax: f64,
    state_tax: f64,
    total_tax: f64,
    early_withdrawal_penalties: f64,
}

/// Accumulator for computing mean tax summaries across iterations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxMeanAccumulator {
    sums: Vec<TaxSums>,
    pub count: usize,
}

impl TaxMeanAccumulator {
    /// Create a new accumulator using the first result as a template
    pub fn new(template: &SimulationResult) -> Self {
        let sums = template
            .yearly_taxes
            .iter()
            .map(|t| TaxSums {
                year: t.year,
                ..Default::default()
            })
            .collect();
        Self { sums, count: 0 }
    }

    /// Add a result to the accumulator
    pub fn accumulate(&mut self, result: &SimulationResult) {
        for (idx, tax) in result.yearly_taxes.iter().enumerate() {
            if let Some(sums) = self.sums.get_mut(idx) {
                sums.ordinary_income += tax.ordinary_income;
                sums.capital_gains += tax.capital_gains;
                sums.tax_free_withdrawals += tax.tax_free_withdrawals;
                sums.federal_tax += tax.federal_tax;
                sums.state_tax += tax.state_tax;
                sums.total_tax += tax.total_tax;
                sums.early_withdrawal_penalties += tax.early_withdrawal_penalties;
            }
        }
        self.count += 1;
    }

    /// Build the mean tax summaries
    pub fn build_mean_taxes(&self) -> Vec<TaxSummary> {
        let n = self.count as f64;
        self.sums
            .iter()
            .map(|sums| TaxSummary {
                year: sums.year,
                ordinary_income: sums.ordinary_income / n,
                capital_gains: sums.capital_gains / n,
                tax_free_withdrawals: sums.tax_free_withdrawals / n,
                federal_tax: sums.federal_tax / n,
                state_tax: sums.state_tax / n,
                total_tax: sums.total_tax / n,
                early_withdrawal_penalties: sums.early_withdrawal_penalties / n,
            })
            .collect()
    }
}

/// Running sums for cash flow accumulation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CashFlowSums {
    year: i16,
    income: f64,
    expenses: f64,
    contributions: f64,
    withdrawals: f64,
    appreciation: f64,
    net_cash_flow: f64,
}

/// Accumulator for computing mean cash flow summaries across iterations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlowMeanAccumulator {
    sums: Vec<CashFlowSums>,
    pub count: usize,
}

impl CashFlowMeanAccumulator {
    /// Create a new accumulator using the first result as a template
    pub fn new(template: &SimulationResult) -> Self {
        let sums = template
            .yearly_cash_flows
            .iter()
            .map(|cf| CashFlowSums {
                year: cf.year,
                ..Default::default()
            })
            .collect();
        Self { sums, count: 0 }
    }

    /// Add a result to the accumulator
    pub fn accumulate(&mut self, result: &SimulationResult) {
        for (idx, cf) in result.yearly_cash_flows.iter().enumerate() {
            if let Some(sums) = self.sums.get_mut(idx) {
                sums.income += cf.income;
                sums.expenses += cf.expenses;
                sums.contributions += cf.contributions;
                sums.withdrawals += cf.withdrawals;
                sums.appreciation += cf.appreciation;
                sums.net_cash_flow += cf.net_cash_flow;
            }
        }
        self.count += 1;
    }

    /// Build the mean cash flow summaries
    pub fn build_mean_cash_flows(&self) -> Vec<YearlyCashFlowSummary> {
        let n = self.count as f64;
        self.sums
            .iter()
            .map(|sums| YearlyCashFlowSummary {
                year: sums.year,
                income: sums.income / n,
                expenses: sums.expenses / n,
                contributions: sums.contributions / n,
                withdrawals: sums.withdrawals / n,
                appreciation: sums.appreciation / n,
                net_cash_flow: sums.net_cash_flow / n,
            })
            .collect()
    }
}

/// Accumulator for computing mean inflation factors across iterations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InflationMeanAccumulator {
    /// Sum of cumulative inflation factors for each year index
    sums: Vec<f64>,
    /// Number of iterations accumulated
    pub count: usize,
}

impl InflationMeanAccumulator {
    /// Create a new accumulator using the first result as a template
    pub fn new(template: &SimulationResult) -> Self {
        Self {
            sums: vec![0.0; template.cumulative_inflation.len()],
            count: 0,
        }
    }

    /// Add a result to the accumulator
    pub fn accumulate(&mut self, result: &SimulationResult) {
        for (idx, &factor) in result.cumulative_inflation.iter().enumerate() {
            if let Some(sum) = self.sums.get_mut(idx) {
                *sum += factor;
            }
        }
        self.count += 1;
    }

    /// Build the mean inflation factors
    pub fn build_mean_inflation(&self) -> Vec<f64> {
        if self.count == 0 {
            return self.sums.clone();
        }
        let n = self.count as f64;
        self.sums.iter().map(|sum| sum / n).collect()
    }
}

/// Accumulators for computing mean values (used to build synthetic mean result)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeanAccumulators {
    pub snapshots: SnapshotMeanAccumulator,
    pub taxes: TaxMeanAccumulator,
    pub cash_flows: CashFlowMeanAccumulator,
    pub inflation: InflationMeanAccumulator,
}

impl MeanAccumulators {
    pub fn new(template: &SimulationResult) -> Self {
        Self {
            snapshots: SnapshotMeanAccumulator::new(template),
            taxes: TaxMeanAccumulator::new(template),
            cash_flows: CashFlowMeanAccumulator::new(template),
            inflation: InflationMeanAccumulator::new(template),
        }
    }

    pub fn accumulate(&mut self, result: &SimulationResult) {
        self.snapshots.accumulate(result);
        self.taxes.accumulate(result);
        self.cash_flows.accumulate(result);
        self.inflation.accumulate(result);
    }

    /// Build a synthetic SimulationResult with mean values
    pub fn build_mean_result(&self) -> SimulationResult {
        SimulationResult {
            wealth_snapshots: self.snapshots.build_mean_snapshots(),
            yearly_taxes: self.taxes.build_mean_taxes(),
            yearly_cash_flows: self.cash_flows.build_mean_cash_flows(),
            ledger: Vec::new(),   // No meaningful ledger for averaged results
            warnings: Vec::new(), // No warnings for averaged results
            cumulative_inflation: self.inflation.build_mean_inflation(),
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
        self.mean_accumulators
            .as_ref()
            .map(|acc| acc.build_mean_result())
    }
}

/// Helper function to calculate final net worth from a SimulationResult
pub fn final_net_worth(result: &SimulationResult) -> f64 {
    result.wealth_snapshots.last().map_or(0.0, |snap| {
        snap.accounts.iter().map(|acc| acc.total_value()).sum()
    })
}

// ============================================================================
// Monte Carlo Progress Tracking
// ============================================================================

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Progress tracker for Monte Carlo simulations
///
/// This struct provides thread-safe progress tracking and cancellation
/// for Monte Carlo simulations running in the background.
///
/// # Example
/// ```ignore
/// let progress = MonteCarloProgress::new();
///
/// // In a separate thread, run the simulation
/// let result = monte_carlo_simulate_with_progress(&config, &mc_config, &progress);
///
/// // In the main thread, poll progress
/// loop {
///     let completed = progress.completed();
///     let total = mc_config.iterations;
///     println!("Progress: {}/{}", completed, total);
///     if completed >= total { break; }
///     std::thread::sleep(std::time::Duration::from_millis(100));
/// }
/// ```
#[derive(Debug, Clone)]
pub struct MonteCarloProgress {
    /// Count of completed iterations
    completed: Arc<AtomicUsize>,
    /// Flag to request cancellation
    cancel: Arc<AtomicBool>,
    /// Skip reset when reusing (for accumulating progress across multiple runs)
    skip_reset: bool,
}

impl Default for MonteCarloProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl MonteCarloProgress {
    /// Create a new progress tracker
    pub fn new() -> Self {
        Self {
            completed: Arc::new(AtomicUsize::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            skip_reset: false,
        }
    }

    /// Create from existing atomics (for interop with existing TUI code)
    pub fn from_atomics(completed: Arc<AtomicUsize>, cancel: Arc<AtomicBool>) -> Self {
        Self {
            completed,
            cancel,
            skip_reset: false,
        }
    }

    /// Create from existing atomics, skipping reset (for accumulating progress)
    pub fn from_atomics_accumulating(completed: Arc<AtomicUsize>, cancel: Arc<AtomicBool>) -> Self {
        Self {
            completed,
            cancel,
            skip_reset: true,
        }
    }

    /// Get the number of completed iterations
    pub fn completed(&self) -> usize {
        self.completed.load(Ordering::Relaxed)
    }

    /// Check if cancellation was requested
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Request cancellation of the simulation
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Reset the progress tracker for reuse
    pub fn reset(&self) {
        if self.skip_reset {
            return;
        }
        self.completed.store(0, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
    }

    /// Increment the completed count (called internally by simulation)
    pub(crate) fn increment(&self) {
        self.completed.fetch_add(1, Ordering::Relaxed);
    }
}
