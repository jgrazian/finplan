use std::sync::Mutex;

use rustc_hash::FxHashMap;

use crate::apply::{SimulationScratch, process_events_with_scratch};
use crate::config::SimulationConfig;
use crate::error::MarketError;
use crate::metrics::{InstrumentationConfig, SimulationMetrics};
use crate::model::{
    AccountFlavor, AccountId, CashFlowKind, ConvergenceMetric, EventTrigger, LedgerEntry,
    MeanAccumulators, MonteCarloConfig, MonteCarloProgress, MonteCarloResult, MonteCarloStats,
    MonteCarloSummary, SimulationResult, SimulationWarning, StateEvent, TaxStatus, WarningKind,
    YearlyCashFlowSummary, final_net_worth,
};
use crate::simulation_state::{SimulationState, cached_spans};
use rand::{RngCore, SeedableRng};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

// Re-export for backwards compatibility
pub use crate::model::n_day_rate;

/// Build yearly cash flow summaries from ledger entries.
/// Uses a Vec indexed by (year - min_year) for O(1) lookups instead of BTreeMap.
fn build_yearly_cash_flows(ledger: &[LedgerEntry]) -> Vec<YearlyCashFlowSummary> {
    if ledger.is_empty() {
        return Vec::new();
    }

    // Find year range from ledger (entries are chronological)
    let min_year = ledger.first().map(|e| e.date.year()).unwrap_or(2024);
    let max_year = ledger.last().map(|e| e.date.year()).unwrap_or(min_year);
    let num_years = (max_year - min_year + 1) as usize;

    // Pre-allocate Vec with default summaries
    let mut yearly: Vec<YearlyCashFlowSummary> = (0..num_years)
        .map(|i| YearlyCashFlowSummary {
            year: min_year + i as i16,
            ..Default::default()
        })
        .collect();

    for entry in ledger {
        let year_idx = (entry.date.year() - min_year) as usize;
        // Safety: year_idx is guaranteed to be in bounds since we calculated range from ledger
        let summary = &mut yearly[year_idx];

        match &entry.event {
            StateEvent::CashCredit { amount, kind, .. } => match kind {
                CashFlowKind::Income => summary.income += amount,
                CashFlowKind::LiquidationProceeds => summary.withdrawals += amount,
                CashFlowKind::Appreciation => summary.appreciation += amount,
                CashFlowKind::RmdWithdrawal => summary.withdrawals += amount,
                _ => {} // Other types don't affect these totals directly
            },
            StateEvent::CashDebit { amount, kind, .. } => match kind {
                CashFlowKind::Expense => summary.expenses += amount,
                CashFlowKind::Contribution => summary.contributions += amount,
                CashFlowKind::InvestmentPurchase => {} // Internal reallocation
                _ => {}
            },
            StateEvent::CashAppreciation {
                previous_value,
                new_value,
                ..
            } => {
                summary.appreciation += new_value - previous_value;
            }
            _ => {}
        }
    }

    // Calculate net cash flow for each year
    for summary in &mut yearly {
        summary.net_cash_flow = summary.income - summary.expenses + summary.appreciation;
    }

    yearly
}

pub fn simulate(params: &SimulationConfig, seed: u64) -> Result<SimulationResult, MarketError> {
    let mut scratch = SimulationScratch::new();
    simulate_with_scratch(params, seed, &mut scratch)
}

/// Simulate with a pre-allocated scratch buffer for reuse across Monte Carlo iterations.
/// This avoids allocation overhead when running many simulations.
pub fn simulate_with_scratch(
    params: &SimulationConfig,
    seed: u64,
    scratch: &mut SimulationScratch,
) -> Result<SimulationResult, MarketError> {
    const MAX_SAME_DATE_ITERATIONS: u64 = 1000;

    let mut state = SimulationState::from_parameters(params, seed)?;
    state.snapshot_wealth();

    while state.timeline.current_date < state.timeline.end_date {
        let mut something_happened = true;
        let mut iteration_count: u64 = 0;

        while something_happened {
            something_happened = false;
            iteration_count += 1;

            // Safety limit to prevent infinite loops (e.g., AccountBalance triggers with once: false
            // when sweep cannot fulfill the request due to depleted accounts)
            if iteration_count > MAX_SAME_DATE_ITERATIONS {
                state.warnings.push(SimulationWarning {
                    date: state.timeline.current_date,
                    event_id: None,
                    message: format!(
                        "iteration limit ({}) reached, possible infinite loop",
                        MAX_SAME_DATE_ITERATIONS
                    ),
                    kind: WarningKind::IterationLimitHit,
                });
                break;
            }

            // Process events - now handles ALL money movement
            process_events_with_scratch(&mut state, scratch);
            if !scratch.triggered.is_empty() {
                something_happened = true;
            }
        }

        advance_time(&mut state, params);
    }

    // Finalize last year's taxes
    state.snapshot_wealth();
    state.finalize_year_taxes();

    // Build yearly cash flow summaries from ledger
    let yearly_cash_flows = build_yearly_cash_flows(&state.history.ledger);

    // Extract cumulative inflation factors for real value calculations
    let cumulative_inflation = state.portfolio.market.get_cumulative_inflation_factors();

    Ok(SimulationResult {
        wealth_snapshots: std::mem::take(&mut state.portfolio.wealth_snapshots),
        yearly_taxes: std::mem::take(&mut state.taxes.yearly_taxes),
        yearly_cash_flows,
        ledger: std::mem::take(&mut state.history.ledger),
        warnings: std::mem::take(&mut state.warnings),
        cumulative_inflation,
    })
}

/// Instrumented simulation that collects metrics and enforces iteration limits
///
/// This function is useful for:
/// - Profiling simulation performance
/// - Detecting potential infinite loops (AccountBalance triggers with once: false)
/// - Debugging event-heavy simulations
///
/// Returns both the simulation result and collected metrics.
pub fn simulate_with_metrics(
    params: &SimulationConfig,
    seed: u64,
    config: &InstrumentationConfig,
) -> Result<(SimulationResult, SimulationMetrics), MarketError> {
    let mut state = SimulationState::from_parameters(params, seed)?;
    let mut metrics = SimulationMetrics::new();
    // Scratch buffer for simulation - reused across all process_events calls
    let mut scratch = SimulationScratch::new();

    state.snapshot_wealth();

    while state.timeline.current_date < state.timeline.end_date {
        let mut something_happened = true;
        let mut iteration_count: u64 = 0;

        while something_happened {
            something_happened = false;
            iteration_count += 1;

            // Safety limit check
            if iteration_count > config.max_same_date_iterations {
                if config.collect_metrics {
                    metrics.record_limit_hit(state.timeline.current_date);
                }
                // Add warning instead of eprintln - the caller can check SimulationResult.warnings
                state.warnings.push(SimulationWarning {
                    date: state.timeline.current_date,
                    event_id: None,
                    message: format!(
                        "iteration limit ({}) reached, possible infinite loop",
                        config.max_same_date_iterations
                    ),
                    kind: WarningKind::IterationLimitHit,
                });
                break;
            }

            // Process events - now handles ALL money movement
            process_events_with_scratch(&mut state, &mut scratch);
            if !scratch.triggered.is_empty() {
                something_happened = true;

                // Record event metrics
                if config.collect_metrics {
                    for event_id in &scratch.triggered {
                        metrics.record_event_triggered(*event_id);
                    }
                }
            }

            if config.collect_metrics {
                metrics.record_iteration(state.timeline.current_date, iteration_count);
            }
        }

        if config.collect_metrics {
            metrics.record_time_step();
        }

        advance_time(&mut state, params);
    }

    // Finalize last year's taxes
    state.snapshot_wealth();
    state.finalize_year_taxes();

    // Build yearly cash flow summaries from ledger
    let yearly_cash_flows = build_yearly_cash_flows(&state.history.ledger);

    // Extract cumulative inflation factors for real value calculations
    let cumulative_inflation = state.portfolio.market.get_cumulative_inflation_factors();

    let result = SimulationResult {
        wealth_snapshots: std::mem::take(&mut state.portfolio.wealth_snapshots),
        yearly_taxes: std::mem::take(&mut state.taxes.yearly_taxes),
        yearly_cash_flows,
        ledger: std::mem::take(&mut state.history.ledger),
        warnings: std::mem::take(&mut state.warnings),
        cumulative_inflation,
    };

    Ok((result, metrics))
}

fn advance_time(state: &mut SimulationState, _params: &SimulationConfig) {
    // Check for year rollover before advancing
    state.maybe_rollover_year();

    // Find next checkpoint
    let mut next_checkpoint = state.timeline.end_date;

    // Check event dates
    for event in state.event_state.iter_events() {
        // Skip if already triggered and once=true (unless Repeating)
        if event.once
            && state.event_state.is_triggered(event.event_id)
            && !matches!(event.trigger, EventTrigger::Repeating { .. })
        {
            continue;
        }

        if let EventTrigger::Date(d) = event.trigger
            && d > state.timeline.current_date
            && d < next_checkpoint
        {
            next_checkpoint = d;
        }

        // Also check relative events - use fast add_to_date
        if let EventTrigger::RelativeToEvent {
            event_id: ref_event_id,
            offset,
        } = &event.trigger
            && let Some(trigger_date) = state.event_state.triggered_date(*ref_event_id)
        {
            let d = offset.add_to_date(trigger_date);
            if d > state.timeline.current_date && d < next_checkpoint {
                next_checkpoint = d;
            }
        }
    }

    // Check repeating event scheduled dates
    for date in state.event_state.event_next_date.iter().flatten() {
        if *date > state.timeline.current_date && *date < next_checkpoint {
            next_checkpoint = *date;
        }
    }

    // Heartbeat - advance at least quarterly (use cached span)
    let heartbeat = state
        .timeline
        .current_date
        .saturating_add(*cached_spans::HEARTBEAT);
    if heartbeat < next_checkpoint {
        next_checkpoint = heartbeat;
    }

    // Ensure we capture December 31 for RMD year-end balance tracking
    let current_year = state.timeline.current_date.year();
    let dec_31 = jiff::civil::date(current_year, 12, 31);
    if state.timeline.current_date < dec_31 && dec_31 < next_checkpoint {
        next_checkpoint = dec_31;
    }

    // Apply interest/returns
    let days_passed = (next_checkpoint - state.timeline.current_date).get_days();
    if days_passed > 0 {
        // Calculate year index for rate lookup (years since simulation start)
        let year_index =
            (state.timeline.current_date.year() - state.timeline.start_date.year()) as usize;

        // Collect account IDs first to avoid borrow conflicts when mutating
        // Pre-allocate with known capacity
        let num_accounts = state.portfolio.accounts.len();
        let mut account_ids: Vec<AccountId> = Vec::with_capacity(num_accounts);
        account_ids.extend(state.portfolio.accounts.keys().copied());

        // Compound cash balances for all accounts and record appreciation events
        for account_id in account_ids {
            // Account should exist since we just collected the keys, but handle gracefully
            // in case it was somehow removed during iteration
            let Some(account) = state.portfolio.accounts.get_mut(&account_id) else {
                continue;
            };
            match &mut account.flavor {
                AccountFlavor::Bank(cash) => {
                    // Only compound positive cash balances (negative = overdraft, shouldn't grow)
                    if cash.value > 0.0 {
                        // Apply return profile to bank account cash
                        if let Some(multiplier) = state.portfolio.market.get_period_multiplier(
                            year_index,
                            days_passed as i64,
                            cash.return_profile_id,
                        ) {
                            let previous_value = cash.value;
                            cash.value *= multiplier;
                            let return_rate = multiplier - 1.0;

                            // Only record if there was actual appreciation
                            if state.collect_ledger && (cash.value - previous_value).abs() > 0.001 {
                                state.history.ledger.push(LedgerEntry::new(
                                    next_checkpoint,
                                    StateEvent::CashAppreciation {
                                        account_id,
                                        previous_value,
                                        new_value: cash.value,
                                        return_rate,
                                        days: days_passed,
                                    },
                                ));
                            }
                        }
                    }
                }
                AccountFlavor::Investment(inv) => {
                    // Only compound positive cash balances (negative = overdraft, shouldn't grow)
                    if inv.cash.value > 0.0 {
                        // Apply return profile to investment account cash (money market, etc.)
                        if let Some(multiplier) = state.portfolio.market.get_period_multiplier(
                            year_index,
                            days_passed as i64,
                            inv.cash.return_profile_id,
                        ) {
                            let previous_value = inv.cash.value;
                            inv.cash.value *= multiplier;
                            let return_rate = multiplier - 1.0;

                            // Only record if there was actual appreciation
                            if state.collect_ledger
                                && (inv.cash.value - previous_value).abs() > 0.001
                            {
                                state.history.ledger.push(LedgerEntry::new(
                                    next_checkpoint,
                                    StateEvent::CashAppreciation {
                                        account_id,
                                        previous_value,
                                        new_value: inv.cash.value,
                                        return_rate,
                                        days: days_passed,
                                    },
                                ));
                            }
                        }
                    }
                }
                AccountFlavor::Liability(loan) => {
                    // Apply interest to liability (debt grows over time)
                    if loan.interest_rate > 0.0 {
                        let previous_principal = loan.principal;
                        let multiplier =
                            (1.0 + loan.interest_rate).powf(days_passed as f64 / 365.0);
                        loan.principal *= multiplier;

                        // Only record if there was actual interest accrual
                        if state.collect_ledger
                            && (loan.principal - previous_principal).abs() > 0.001
                        {
                            state.history.ledger.push(LedgerEntry::new(
                                next_checkpoint,
                                StateEvent::LiabilityInterestAccrual {
                                    account_id,
                                    previous_principal,
                                    new_principal: loan.principal,
                                    interest_rate: loan.interest_rate,
                                    days: days_passed,
                                },
                            ));
                        }
                    }
                }
                AccountFlavor::Property(_) => {}
            }
        }

        // Record time advance event
        if state.collect_ledger {
            state.history.ledger.push(LedgerEntry::new(
                next_checkpoint,
                StateEvent::TimeAdvance {
                    from_date: state.timeline.current_date,
                    to_date: next_checkpoint,
                    days_elapsed: days_passed,
                },
            ));
        }
    }

    // Capture year-end balances for RMD calculations (December 31)
    if next_checkpoint == dec_31 {
        let year = next_checkpoint.year();
        let mut year_balances = FxHashMap::default();

        for (account_id, account) in &state.portfolio.accounts {
            if let AccountFlavor::Investment(inv) = &account.flavor
                && matches!(inv.tax_status, TaxStatus::TaxDeferred)
                && let Ok(balance) = state.account_balance(*account_id)
            {
                year_balances.insert(*account_id, balance);
            }
        }

        state
            .portfolio
            .year_end_balances
            .insert(year, year_balances);

        // Capture year-end net worth
        state.snapshot_wealth();
    }

    // Check if we're crossing a month boundary and reset monthly contributions
    let prev_month = state.timeline.current_date.month();
    let next_month = next_checkpoint.month();
    let prev_year = state.timeline.current_date.year();
    let next_year = next_checkpoint.year();

    if prev_month != next_month || prev_year != next_year {
        state.reset_monthly_contributions();
    }

    // Reset yearly contributions on year boundary
    if prev_year != next_year {
        state.portfolio.contributions_ytd.clear();
    }

    state.timeline.current_date = next_checkpoint;
}

/// DEPRECATED: Use monte_carlo_simulate_with_config for memory efficiency
pub fn monte_carlo_simulate(
    params: &SimulationConfig,
    num_iterations: usize,
) -> Result<MonteCarloResult, MarketError> {
    const MAX_BATCH_SIZE: usize = 100;
    let num_batches = num_iterations.div_ceil(MAX_BATCH_SIZE);

    // First validate by running one simulation to check for market errors
    // This prevents us from running many iterations only to fail at the end
    let _ = simulate(params, 0)?;

    let iterations: Vec<SimulationResult> = (0..num_batches)
        .into_par_iter()
        .flat_map(|i| {
            let mut rng = rand::rngs::SmallRng::seed_from_u64(i as u64);
            // Reuse scratch buffer across all iterations in this batch
            let mut scratch = SimulationScratch::new();

            let batch_size = if i == num_batches - 1 {
                num_iterations - i * MAX_BATCH_SIZE
            } else {
                MAX_BATCH_SIZE
            };

            (0..batch_size)
                .filter_map(|_| {
                    let seed = rng.next_u64();
                    // Skip failed simulations (should be rare after initial validation)
                    simulate_with_scratch(params, seed, &mut scratch).ok()
                })
                .collect::<Vec<_>>()
        })
        .collect();

    Ok(MonteCarloResult { iterations })
}

/// Online statistics accumulator for convergence checking
struct OnlineStats {
    count: usize,
    sum: f64,
    sum_sq: f64,
}

impl OnlineStats {
    fn new() -> Self {
        Self {
            count: 0,
            sum: 0.0,
            sum_sq: 0.0,
        }
    }

    fn add(&mut self, value: f64) {
        self.count += 1;
        self.sum += value;
        self.sum_sq += value * value;
    }

    fn merge(&mut self, other: &OnlineStats) {
        self.count += other.count;
        self.sum += other.sum;
        self.sum_sq += other.sum_sq;
    }

    fn mean(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f64
        }
    }

    fn variance(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            let mean = self.mean();
            (self.sum_sq / self.count as f64) - (mean * mean)
        }
    }

    fn std_dev(&self) -> f64 {
        self.variance().sqrt()
    }

    /// Calculate relative standard error: SEM / |mean|
    /// Returns None if mean is zero (to avoid division by zero)
    fn relative_standard_error(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        let mean = self.mean();
        if mean.abs() < f64::EPSILON {
            return None;
        }
        let sem = self.std_dev() / (self.count as f64).sqrt();
        Some(sem / mean.abs())
    }
}

/// Convergence tracker that supports multiple metrics
struct ConvergenceTracker {
    metric: ConvergenceMetric,
    threshold: f64,
    /// Previous values for stability checking (median, success_rate, p5, p50, p95)
    prev_median: Option<f64>,
    prev_success_rate: Option<f64>,
    prev_percentiles: Option<(f64, f64, f64)>, // (p5, p50, p95)
}

impl ConvergenceTracker {
    fn new(metric: ConvergenceMetric, threshold: f64) -> Self {
        Self {
            metric,
            threshold,
            prev_median: None,
            prev_success_rate: None,
            prev_percentiles: None,
        }
    }

    /// Check if convergence has been achieved based on current data
    /// Returns (converged, current_metric_value)
    fn check_convergence(
        &mut self,
        seed_results: &[(u64, f64)],
        online_stats: &OnlineStats,
    ) -> (bool, Option<f64>) {
        let n = seed_results.len();
        if n == 0 {
            return (false, None);
        }

        match self.metric {
            ConvergenceMetric::Mean => {
                // Use relative standard error for mean convergence
                if let Some(rse) = online_stats.relative_standard_error() {
                    (rse < self.threshold, Some(rse))
                } else {
                    (false, None)
                }
            }
            ConvergenceMetric::Median => {
                // Track P50 stability between batches
                let median_idx = (n as f64 * 0.5).floor() as usize;
                let median = seed_results
                    .get(median_idx.min(n - 1))
                    .map(|(_, v)| *v)
                    .unwrap_or(0.0);

                let relative_change = if let Some(prev) = self.prev_median {
                    if prev.abs() < f64::EPSILON {
                        if median.abs() < f64::EPSILON {
                            0.0
                        } else {
                            f64::INFINITY
                        }
                    } else {
                        ((median - prev) / prev).abs()
                    }
                } else {
                    f64::INFINITY
                };

                self.prev_median = Some(median);
                (relative_change < self.threshold, Some(relative_change))
            }
            ConvergenceMetric::SuccessRate => {
                // Track success rate stability
                let success_count = seed_results.iter().filter(|(_, v)| *v > 0.0).count();
                let success_rate = success_count as f64 / n as f64;

                let absolute_change = if let Some(prev) = self.prev_success_rate {
                    (success_rate - prev).abs()
                } else {
                    f64::INFINITY
                };

                self.prev_success_rate = Some(success_rate);
                // For success rate, use absolute change since it's already a percentage
                (absolute_change < self.threshold, Some(absolute_change))
            }
            ConvergenceMetric::Percentiles => {
                // Track P5/P50/P95 stability
                let p5_idx = (n as f64 * 0.05).floor() as usize;
                let p50_idx = (n as f64 * 0.50).floor() as usize;
                let p95_idx = (n as f64 * 0.95).floor() as usize;

                let p5 = seed_results
                    .get(p5_idx.min(n - 1))
                    .map(|(_, v)| *v)
                    .unwrap_or(0.0);
                let p50 = seed_results
                    .get(p50_idx.min(n - 1))
                    .map(|(_, v)| *v)
                    .unwrap_or(0.0);
                let p95 = seed_results
                    .get(p95_idx.min(n - 1))
                    .map(|(_, v)| *v)
                    .unwrap_or(0.0);

                let max_relative_change =
                    if let Some((prev_p5, prev_p50, prev_p95)) = self.prev_percentiles {
                        let rel_change = |curr: f64, prev: f64| {
                            if prev.abs() < f64::EPSILON {
                                if curr.abs() < f64::EPSILON {
                                    0.0
                                } else {
                                    f64::INFINITY
                                }
                            } else {
                                ((curr - prev) / prev).abs()
                            }
                        };

                        rel_change(p5, prev_p5)
                            .max(rel_change(p50, prev_p50))
                            .max(rel_change(p95, prev_p95))
                    } else {
                        f64::INFINITY
                    };

                self.prev_percentiles = Some((p5, p50, p95));
                (
                    max_relative_change < self.threshold,
                    Some(max_relative_change),
                )
            }
        }
    }
}

/// Memory-efficient Monte Carlo simulation
///
/// This function runs simulations in two phases:
/// 1. First pass: Run all iterations, keeping only (seed, final_net_worth) and accumulating mean sums
/// 2. Second pass: Re-run only the specific seeds needed for percentile runs
///
/// This approach uses O(N) memory for seeds/values instead of O(N * result_size)
///
/// # Convergence Mode
/// If `config.convergence` is set, the simulation will continue running batches until:
/// - The relative standard error (SEM / |mean|) falls below the threshold, OR
/// - The maximum number of iterations is reached
///
/// In this mode, `config.iterations` serves as the minimum number of iterations
/// before convergence checking begins.
pub fn monte_carlo_simulate_with_config(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
) -> Result<MonteCarloSummary, MarketError> {
    let batch_size = config.batch_size;
    let parallel_batches = config.parallel_batches;

    // First validate by running one simulation to check for market errors
    // This prevents us from running many iterations only to fail
    let _ = simulate(params, 0)?;

    // Create a config with ledger disabled for batch iterations
    // This saves CPU/memory since we only need final_net_worth during batches
    let mut batch_params = params.clone();
    batch_params.collect_ledger = false;

    // Determine iteration limits based on mode
    let min_iterations = config.iterations;
    let max_iterations = config
        .convergence
        .as_ref()
        .map(|c| c.max_iterations)
        .unwrap_or(config.iterations);

    // Set up convergence tracker if in convergence mode
    let mut convergence_tracker = config
        .convergence
        .as_ref()
        .map(|c| ConvergenceTracker::new(c.metric, c.relative_threshold));

    // Track all results and statistics
    let mut seed_results: Vec<(u64, f64)> = Vec::new();
    let mut online_stats = OnlineStats::new();
    let mut mean_accumulators: Option<MeanAccumulators> = None;
    // Use configured seed if provided, otherwise generate a random one
    let mut batch_seed: u64 = config.seed.unwrap_or_else(|| rand::rng().next_u64());
    let mut converged = false;
    let mut final_convergence_value: Option<f64> = None;

    // Run batches until we have enough iterations and (optionally) convergence
    loop {
        let current_count = seed_results.len();

        // Check if we've reached max iterations
        if current_count >= max_iterations {
            break;
        }

        // Calculate how many iterations to run in this round
        let remaining = max_iterations - current_count;
        let target_this_round = remaining.min(batch_size * parallel_batches);
        let num_batches = target_this_round.div_ceil(batch_size);

        // Run batches in parallel
        let mean_accumulator: Option<Mutex<Option<MeanAccumulators>>> = if config.compute_mean {
            Some(Mutex::new(mean_accumulators.take()))
        } else {
            None
        };

        // Run batches in parallel, collecting results and stats
        let batch_outputs: Vec<(Vec<(u64, f64)>, OnlineStats)> = (0..num_batches)
            .into_par_iter()
            .map(|local_batch_idx| {
                let mut rng =
                    rand::rngs::SmallRng::seed_from_u64(batch_seed + local_batch_idx as u64);
                let mut scratch = SimulationScratch::new();
                let mut local_stats = OnlineStats::new();
                let mut local_results = Vec::new();

                let this_batch_size = if local_batch_idx == num_batches - 1 {
                    target_this_round - local_batch_idx * batch_size
                } else {
                    batch_size
                };

                for _ in 0..this_batch_size {
                    let seed = rng.next_u64();
                    // Use batch_params with ledger disabled for efficiency
                    if let Ok(result) = simulate_with_scratch(&batch_params, seed, &mut scratch) {
                        let fnw = final_net_worth(&result);
                        local_stats.add(fnw);
                        local_results.push((seed, fnw));

                        if let Some(ref acc_mutex) = mean_accumulator {
                            let mut acc_guard = match acc_mutex.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            match acc_guard.as_mut() {
                                Some(acc) => acc.accumulate(&result),
                                None => {
                                    let mut new_acc = MeanAccumulators::new(&result);
                                    new_acc.accumulate(&result);
                                    *acc_guard = Some(new_acc);
                                }
                            }
                        }
                    }
                }

                (local_results, local_stats)
            })
            .collect();

        // Merge results and stats
        for (results, stats) in batch_outputs {
            seed_results.extend(results);
            online_stats.merge(&stats);
        }

        // Update batch seed for next round
        batch_seed += num_batches as u64;

        // Extract mean accumulators for next round or final use
        mean_accumulators = mean_accumulator.and_then(|m| match m.into_inner() {
            Ok(opt) => opt,
            Err(poisoned) => poisoned.into_inner(),
        });

        // Sort results for percentile-based convergence checks
        seed_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Check convergence if we have enough iterations
        if let Some(ref mut tracker) = convergence_tracker {
            if seed_results.len() >= min_iterations {
                let (is_converged, metric_value) =
                    tracker.check_convergence(&seed_results, &online_stats);
                final_convergence_value = metric_value;
                if is_converged {
                    converged = true;
                    break;
                }
            }
        } else if seed_results.len() >= config.iterations {
            // Fixed mode: stop after reaching target iterations
            break;
        }
    }

    // Final sort (may already be sorted from last iteration)
    seed_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Calculate final statistics
    let actual_iterations = seed_results.len();
    let final_values: Vec<f64> = seed_results.iter().map(|(_, v)| *v).collect();
    let mean_final_net_worth = online_stats.mean();
    let std_dev_final_net_worth = online_stats.std_dev();

    let min_final_net_worth = final_values.first().copied().unwrap_or(0.0);
    let max_final_net_worth = final_values.last().copied().unwrap_or(0.0);

    let success_count = final_values.iter().filter(|v| **v > 0.0).count();
    let success_rate = if actual_iterations > 0 {
        success_count as f64 / actual_iterations as f64
    } else {
        0.0
    };

    // Calculate percentile indices and values
    let mut percentile_values = Vec::new();
    let mut percentile_seeds = Vec::new();

    if actual_iterations > 0 {
        for &p in &config.percentiles {
            let idx = ((actual_iterations as f64 * p).floor() as usize).min(actual_iterations - 1);
            let (seed, value) = seed_results[idx];
            percentile_values.push((p, value));
            percentile_seeds.push((p, seed));
        }
    }

    // Phase 2: Re-run simulations for percentile seeds to get full results
    let percentile_runs: Vec<(f64, SimulationResult)> = percentile_seeds
        .into_iter()
        .filter_map(|(p, seed)| simulate(params, seed).ok().map(|result| (p, result)))
        .collect();

    // Build stats with convergence info
    let stats = MonteCarloStats {
        num_iterations: actual_iterations,
        success_rate,
        mean_final_net_worth,
        std_dev_final_net_worth,
        min_final_net_worth,
        max_final_net_worth,
        percentile_values,
        converged: config.convergence.as_ref().map(|_| converged),
        convergence_metric: config.convergence.as_ref().map(|c| c.metric),
        convergence_value: final_convergence_value,
    };

    Ok(MonteCarloSummary {
        stats,
        percentile_runs,
        mean_accumulators,
    })
}

/// Memory-efficient Monte Carlo simulation with progress tracking
///
/// This function is identical to `monte_carlo_simulate_with_config` but provides
/// real-time progress updates via a `MonteCarloProgress` struct. This allows
/// UI applications to display accurate progress bars during simulation.
///
/// # Progress Updates
/// The `progress.completed()` counter is incremented after each iteration completes.
/// The TUI can poll this value to update progress display.
///
/// # Cancellation
/// If `progress.is_cancelled()` returns true, the simulation will stop early
/// and return `Err(MarketError::Cancelled)`.
///
/// # Convergence Mode
/// If `config.convergence` is set, the simulation will continue running batches until:
/// - The relative standard error (SEM / |mean|) falls below the threshold, OR
/// - The maximum number of iterations is reached
///
/// In this mode, `config.iterations` serves as the minimum number of iterations
/// before convergence checking begins.
///
/// # Example
/// ```ignore
/// let progress = MonteCarloProgress::new();
/// let progress_clone = progress.clone();
///
/// // Run simulation in background thread
/// let handle = std::thread::spawn(move || {
///     monte_carlo_simulate_with_progress(&config, &mc_config, &progress_clone)
/// });
///
/// // Poll progress in main thread
/// while progress.completed() < mc_config.iterations {
///     println!("Progress: {}/{}", progress.completed(), mc_config.iterations);
///     std::thread::sleep(std::time::Duration::from_millis(100));
/// }
///
/// let result = handle.join().unwrap()?;
/// ```
pub fn monte_carlo_simulate_with_progress(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
    progress: &MonteCarloProgress,
) -> Result<MonteCarloSummary, MarketError> {
    let batch_size = config.batch_size;
    let parallel_batches = config.parallel_batches;

    // Reset progress for new simulation
    progress.reset();

    // Check for early cancellation
    if progress.is_cancelled() {
        return Err(MarketError::Cancelled);
    }

    // First validate by running one simulation to check for market errors
    let _ = simulate(params, 0)?;

    // Check for cancellation after validation
    if progress.is_cancelled() {
        return Err(MarketError::Cancelled);
    }

    // Create a config with ledger disabled for batch iterations
    // This saves CPU/memory since we only need final_net_worth during batches
    let mut batch_params = params.clone();
    batch_params.collect_ledger = false;

    // Determine iteration limits based on mode
    let min_iterations = config.iterations;
    let max_iterations = config
        .convergence
        .as_ref()
        .map(|c| c.max_iterations)
        .unwrap_or(config.iterations);

    // Set up convergence tracker if in convergence mode
    let mut convergence_tracker = config
        .convergence
        .as_ref()
        .map(|c| ConvergenceTracker::new(c.metric, c.relative_threshold));

    // Track all results and statistics
    let mut seed_results: Vec<(u64, f64)> = Vec::new();
    let mut online_stats = OnlineStats::new();
    let mut mean_accumulators: Option<MeanAccumulators> = None;
    // Use configured seed if provided, otherwise generate a random one
    let mut batch_seed: u64 = config.seed.unwrap_or_else(|| rand::rng().next_u64());
    let mut converged = false;
    let mut final_convergence_value: Option<f64> = None;

    // Track if any thread detected cancellation
    let cancelled = std::sync::atomic::AtomicBool::new(false);

    // Run batches until we have enough iterations and (optionally) convergence
    loop {
        let current_count = seed_results.len();

        // Check if we've reached max iterations
        if current_count >= max_iterations {
            break;
        }

        // Check for cancellation
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) || progress.is_cancelled() {
            return Err(MarketError::Cancelled);
        }

        // Calculate how many iterations to run in this round
        let remaining = max_iterations - current_count;
        let target_this_round = remaining.min(batch_size * parallel_batches);
        let num_batches = target_this_round.div_ceil(batch_size);

        // Run batches in parallel
        let mean_accumulator: Option<Mutex<Option<MeanAccumulators>>> = if config.compute_mean {
            Some(Mutex::new(mean_accumulators.take()))
        } else {
            None
        };

        // Run batches in parallel
        let batch_outputs: Vec<(Vec<(u64, f64)>, OnlineStats)> = (0..num_batches)
            .into_par_iter()
            .map(|local_batch_idx| {
                // Check cancellation at batch start
                if cancelled.load(std::sync::atomic::Ordering::Relaxed) || progress.is_cancelled() {
                    cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                    return (Vec::new(), OnlineStats::new());
                }

                let mut rng =
                    rand::rngs::SmallRng::seed_from_u64(batch_seed + local_batch_idx as u64);
                let mut scratch = SimulationScratch::new();
                let mut local_stats = OnlineStats::new();
                let mut local_results = Vec::new();

                let this_batch_size = if local_batch_idx == num_batches - 1 {
                    target_this_round - local_batch_idx * batch_size
                } else {
                    batch_size
                };

                for _ in 0..this_batch_size {
                    // Check cancellation periodically within batch
                    if progress.is_cancelled() {
                        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }

                    let seed = rng.next_u64();
                    // Use batch_params with ledger disabled for efficiency
                    if let Ok(result) = simulate_with_scratch(&batch_params, seed, &mut scratch) {
                        let fnw = final_net_worth(&result);
                        local_stats.add(fnw);
                        local_results.push((seed, fnw));

                        if let Some(ref acc_mutex) = mean_accumulator {
                            let mut acc_guard = match acc_mutex.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            match acc_guard.as_mut() {
                                Some(acc) => acc.accumulate(&result),
                                None => {
                                    let mut new_acc = MeanAccumulators::new(&result);
                                    new_acc.accumulate(&result);
                                    *acc_guard = Some(new_acc);
                                }
                            }
                        }

                        // Update progress counter
                        progress.increment();
                    }
                }

                (local_results, local_stats)
            })
            .collect();

        // Merge results and stats
        for (results, stats) in batch_outputs {
            seed_results.extend(results);
            online_stats.merge(&stats);
        }

        // Update batch seed for next round
        batch_seed += num_batches as u64;

        // Extract mean accumulators for next round or final use
        mean_accumulators = mean_accumulator.and_then(|m| match m.into_inner() {
            Ok(opt) => opt,
            Err(poisoned) => poisoned.into_inner(),
        });

        // Check for cancellation after batch
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) || progress.is_cancelled() {
            return Err(MarketError::Cancelled);
        }

        // Sort results for percentile-based convergence checks
        seed_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Check convergence if we have enough iterations
        if let Some(ref mut tracker) = convergence_tracker {
            if seed_results.len() >= min_iterations {
                let (is_converged, metric_value) =
                    tracker.check_convergence(&seed_results, &online_stats);
                final_convergence_value = metric_value;
                if is_converged {
                    converged = true;
                    break;
                }
            }
        } else if seed_results.len() >= config.iterations {
            // Fixed mode: stop after reaching target iterations
            break;
        }
    }

    // Final sort (may already be sorted from last iteration)
    seed_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Calculate final statistics
    let actual_iterations = seed_results.len();
    let final_values: Vec<f64> = seed_results.iter().map(|(_, v)| *v).collect();
    let mean_final_net_worth = online_stats.mean();
    let std_dev_final_net_worth = online_stats.std_dev();

    let min_final_net_worth = final_values.first().copied().unwrap_or(0.0);
    let max_final_net_worth = final_values.last().copied().unwrap_or(0.0);

    let success_count = final_values.iter().filter(|v| **v > 0.0).count();
    let success_rate = if actual_iterations > 0 {
        success_count as f64 / actual_iterations as f64
    } else {
        0.0
    };

    // Calculate percentile indices and values
    let mut percentile_values = Vec::new();
    let mut percentile_seeds = Vec::new();

    if actual_iterations > 0 {
        for &p in &config.percentiles {
            let idx = ((actual_iterations as f64 * p).floor() as usize).min(actual_iterations - 1);
            let (seed, value) = seed_results[idx];
            percentile_values.push((p, value));
            percentile_seeds.push((p, seed));
        }
    }

    // Phase 2: Re-run simulations for percentile seeds to get full results
    let percentile_runs: Vec<(f64, SimulationResult)> = percentile_seeds
        .into_iter()
        .filter_map(|(p, seed)| simulate(params, seed).ok().map(|result| (p, result)))
        .collect();

    // Build stats with convergence info
    let stats = MonteCarloStats {
        num_iterations: actual_iterations,
        success_rate,
        mean_final_net_worth,
        std_dev_final_net_worth,
        min_final_net_worth,
        max_final_net_worth,
        percentile_values,
        converged: config.convergence.as_ref().map(|_| converged),
        convergence_metric: config.convergence.as_ref().map(|c| c.metric),
        convergence_value: final_convergence_value,
    };

    Ok(MonteCarloSummary {
        stats,
        percentile_runs,
        mean_accumulators,
    })
}

/// Memory-efficient Monte Carlo simulation that returns only stats and percentile seeds.
///
/// This function is similar to `monte_carlo_simulate_with_progress` but skips Phase 2
/// (re-running simulations for percentile results). Instead, it returns the seeds
/// for each percentile, allowing the caller to re-run specific simulations on demand.
///
/// This is ideal for sweep analysis where storing full results for every grid point
/// would consume excessive memory.
///
/// # Returns
/// A tuple of (MonteCarloStats, Vec<(f64, u64)>) where the second element contains
/// (percentile, seed) pairs for on-demand reconstruction.
pub fn monte_carlo_stats_only(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
    progress: &MonteCarloProgress,
) -> Result<(MonteCarloStats, Vec<(f64, u64)>), MarketError> {
    let batch_size = config.batch_size;
    let parallel_batches = config.parallel_batches;

    // Reset progress for new simulation
    progress.reset();

    // Check for early cancellation
    if progress.is_cancelled() {
        return Err(MarketError::Cancelled);
    }

    // First validate by running one simulation to check for market errors
    let _ = simulate(params, 0)?;

    // Check for cancellation after validation
    if progress.is_cancelled() {
        return Err(MarketError::Cancelled);
    }

    // Create a config with ledger disabled for batch iterations
    let mut batch_params = params.clone();
    batch_params.collect_ledger = false;

    // Determine iteration limits based on mode
    let min_iterations = config.iterations;
    let max_iterations = config
        .convergence
        .as_ref()
        .map(|c| c.max_iterations)
        .unwrap_or(config.iterations);

    // Set up convergence tracker if in convergence mode
    let mut convergence_tracker = config
        .convergence
        .as_ref()
        .map(|c| ConvergenceTracker::new(c.metric, c.relative_threshold));

    // Track all results and statistics
    let mut seed_results: Vec<(u64, f64)> = Vec::new();
    let mut online_stats = OnlineStats::new();
    // Use configured seed if provided, otherwise generate a random one
    let mut batch_seed: u64 = config.seed.unwrap_or_else(|| rand::rng().next_u64());
    let mut converged = false;
    let mut final_convergence_value: Option<f64> = None;

    // Track if any thread detected cancellation
    let cancelled = std::sync::atomic::AtomicBool::new(false);

    // Run batches until we have enough iterations and (optionally) convergence
    loop {
        let current_count = seed_results.len();

        // Check if we've reached max iterations
        if current_count >= max_iterations {
            break;
        }

        // Check for cancellation
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) || progress.is_cancelled() {
            return Err(MarketError::Cancelled);
        }

        // Calculate how many iterations to run in this round
        let remaining = max_iterations - current_count;
        let target_this_round = remaining.min(batch_size * parallel_batches);
        let num_batches = target_this_round.div_ceil(batch_size);

        // Run batches in parallel
        let batch_outputs: Vec<(Vec<(u64, f64)>, OnlineStats)> = (0..num_batches)
            .into_par_iter()
            .map(|local_batch_idx| {
                // Check cancellation at batch start
                if cancelled.load(std::sync::atomic::Ordering::Relaxed) || progress.is_cancelled() {
                    cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                    return (Vec::new(), OnlineStats::new());
                }

                let mut rng =
                    rand::rngs::SmallRng::seed_from_u64(batch_seed + local_batch_idx as u64);
                let mut scratch = SimulationScratch::new();
                let mut local_stats = OnlineStats::new();
                let mut local_results = Vec::new();

                let this_batch_size = if local_batch_idx == num_batches - 1 {
                    target_this_round - local_batch_idx * batch_size
                } else {
                    batch_size
                };

                for _ in 0..this_batch_size {
                    // Check cancellation periodically within batch
                    if progress.is_cancelled() {
                        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }

                    let seed = rng.next_u64();
                    if let Ok(result) = simulate_with_scratch(&batch_params, seed, &mut scratch) {
                        let fnw = final_net_worth(&result);
                        local_stats.add(fnw);
                        local_results.push((seed, fnw));

                        // Update progress counter
                        progress.increment();
                    }
                }

                (local_results, local_stats)
            })
            .collect();

        // Merge results and stats
        for (results, stats) in batch_outputs {
            seed_results.extend(results);
            online_stats.merge(&stats);
        }

        // Update batch seed for next round
        batch_seed += num_batches as u64;

        // Check for cancellation after stats-only batch
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) || progress.is_cancelled() {
            return Err(MarketError::Cancelled);
        }

        // Sort results for percentile-based convergence checks
        seed_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Check convergence if we have enough iterations
        if let Some(ref mut tracker) = convergence_tracker {
            if seed_results.len() >= min_iterations {
                let (is_converged, metric_value) =
                    tracker.check_convergence(&seed_results, &online_stats);
                final_convergence_value = metric_value;
                if is_converged {
                    converged = true;
                    break;
                }
            }
        } else if seed_results.len() >= config.iterations {
            // Fixed mode: stop after reaching target iterations
            break;
        }
    }

    // Final sort (may already be sorted from last iteration)
    seed_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Calculate final statistics
    let actual_iterations = seed_results.len();
    let final_values: Vec<f64> = seed_results.iter().map(|(_, v)| *v).collect();
    let mean_final_net_worth = online_stats.mean();
    let std_dev_final_net_worth = online_stats.std_dev();

    let min_final_net_worth = final_values.first().copied().unwrap_or(0.0);
    let max_final_net_worth = final_values.last().copied().unwrap_or(0.0);

    let success_count = final_values.iter().filter(|v| **v > 0.0).count();
    let success_rate = if actual_iterations > 0 {
        success_count as f64 / actual_iterations as f64
    } else {
        0.0
    };

    // Calculate percentile indices and values, and capture seeds for lazy reconstruction
    let mut percentile_values = Vec::new();
    let mut percentile_seeds = Vec::new();

    if actual_iterations > 0 {
        for &p in &config.percentiles {
            let idx = ((actual_iterations as f64 * p).floor() as usize).min(actual_iterations - 1);
            let (seed, value) = seed_results[idx];
            percentile_values.push((p, value));
            percentile_seeds.push((p, seed));
        }
    }

    // Build stats (no percentile_runs since we return seeds for lazy computation)
    let stats = MonteCarloStats {
        num_iterations: actual_iterations,
        success_rate,
        mean_final_net_worth,
        std_dev_final_net_worth,
        min_final_net_worth,
        max_final_net_worth,
        percentile_values,
        converged: config.convergence.as_ref().map(|_| converged),
        convergence_metric: config.convergence.as_ref().map(|c| c.metric),
        convergence_value: final_convergence_value,
    };

    Ok((stats, percentile_seeds))
}
