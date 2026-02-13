use std::sync::Mutex;

use rustc_hash::FxHashMap;

use crate::apply::{SimulationScratch, process_events_with_scratch};
use crate::config::SimulationConfig;
use crate::error::SimulationError;
use crate::metrics::{InstrumentationConfig, SimulationMetrics};
use crate::model::{
    AccountFlavor, AccountId, CashFlowKind, ConvergenceMetric, EventTrigger, LedgerEntry,
    MeanAccumulators, MonteCarloConfig, MonteCarloProgress, MonteCarloStats, MonteCarloSummary,
    MonthlyCashFlowSummary, SimulationResult, SimulationWarning, StateEvent, TaxStatus,
    WarningKind, YearlyCashFlowSummary, final_net_worth,
};
use crate::simulation_state::SimulationState;
use rand::{RngCore, SeedableRng};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

// Re-export for backwards compatibility
pub use crate::model::n_day_rate;

// ── Single simulation ────────────────────────────────────────────────

/// Build yearly cash flow summaries from ledger entries.
/// Uses a Vec indexed by (year - min_year) for O(1) lookups instead of BTreeMap.
fn build_yearly_cash_flows(ledger: &[LedgerEntry]) -> Vec<YearlyCashFlowSummary> {
    if ledger.is_empty() {
        return Vec::new();
    }

    // Find year range from ledger (entries are chronological)
    let min_year = ledger.first().map_or(2024, |e| e.date.year());
    let max_year = ledger.last().map_or(min_year, |e| e.date.year());
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
        let summary = &mut yearly[year_idx];

        match &entry.event {
            StateEvent::CashCredit { amount, kind, .. } => match kind {
                CashFlowKind::Income => summary.income += amount,
                CashFlowKind::LiquidationProceeds | CashFlowKind::RmdWithdrawal => {
                    summary.withdrawals += amount;
                }
                CashFlowKind::Appreciation => summary.appreciation += amount,
                _ => {}
            },
            StateEvent::CashDebit { amount, kind, .. } => match kind {
                CashFlowKind::Expense => summary.expenses += amount,
                CashFlowKind::Contribution => summary.contributions += amount,
                CashFlowKind::InvestmentPurchase => {}
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

    for summary in &mut yearly {
        summary.net_cash_flow = summary.income - summary.expenses + summary.appreciation;
    }

    yearly
}

/// Build monthly cash flow summaries from ledger entries.
/// Called lazily (not during simulation) when the user wants monthly granularity.
#[must_use]
pub fn build_monthly_cash_flows(ledger: &[LedgerEntry]) -> Vec<MonthlyCashFlowSummary> {
    if ledger.is_empty() {
        return Vec::new();
    }

    // Collect unique (year, month) pairs from the ledger.
    // Entries are chronological so we can track by (year, month) key.
    let min_year = ledger.first().map_or(2024, |e| e.date.year());
    let max_year = ledger.last().map_or(min_year, |e| e.date.year());
    let num_months = ((max_year - min_year) as usize + 1) * 12;

    // Index: (year - min_year) * 12 + (month - 1)
    let mut monthly: Vec<MonthlyCashFlowSummary> = (0..num_months)
        .map(|i| {
            let year = min_year + (i / 12) as i16;
            let month = (i % 12) as u8 + 1;
            MonthlyCashFlowSummary {
                year,
                month,
                ..Default::default()
            }
        })
        .collect();

    for entry in ledger {
        let year = entry.date.year();
        let month = entry.date.month() as u8;
        let idx = (year - min_year) as usize * 12 + (month as usize - 1);
        let summary = &mut monthly[idx];

        match &entry.event {
            StateEvent::CashCredit { amount, kind, .. } => match kind {
                CashFlowKind::Income => summary.income += amount,
                CashFlowKind::LiquidationProceeds | CashFlowKind::RmdWithdrawal => {
                    summary.withdrawals += amount;
                }
                CashFlowKind::Appreciation => summary.appreciation += amount,
                _ => {}
            },
            StateEvent::CashDebit { amount, kind, .. } => match kind {
                CashFlowKind::Expense => summary.expenses += amount,
                CashFlowKind::Contribution => summary.contributions += amount,
                CashFlowKind::InvestmentPurchase => {}
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

    for summary in &mut monthly {
        summary.net_cash_flow =
            summary.income + summary.withdrawals - summary.expenses - summary.contributions
                + summary.appreciation;
    }

    // Remove trailing empty months (those after the last ledger entry)
    let last_year = ledger.last().map_or(min_year, |e| e.date.year());
    let last_month = ledger.last().map_or(12, |e| e.date.month() as u8);
    let last_idx = (last_year - min_year) as usize * 12 + (last_month as usize - 1);
    monthly.truncate(last_idx + 1);

    monthly
}

/// Extract the final `SimulationResult` from a completed simulation state.
fn build_simulation_result(state: &mut SimulationState) -> SimulationResult {
    let yearly_cash_flows = build_yearly_cash_flows(&state.history.ledger);
    let cumulative_inflation = state.portfolio.market.get_cumulative_inflation_factors();

    SimulationResult {
        wealth_snapshots: std::mem::take(&mut state.portfolio.wealth_snapshots),
        yearly_taxes: std::mem::take(&mut state.taxes.yearly_taxes),
        yearly_cash_flows,
        ledger: std::mem::take(&mut state.history.ledger),
        warnings: std::mem::take(&mut state.warnings),
        cumulative_inflation,
    }
}

pub fn simulate(params: &SimulationConfig, seed: u64) -> Result<SimulationResult, SimulationError> {
    let mut scratch = SimulationScratch::new();
    simulate_with_scratch(params, seed, &mut scratch)
}

/// Simulate with a pre-allocated scratch buffer for reuse across Monte Carlo iterations.
/// This avoids allocation overhead when running many simulations.
pub fn simulate_with_scratch(
    params: &SimulationConfig,
    seed: u64,
    scratch: &mut SimulationScratch,
) -> Result<SimulationResult, SimulationError> {
    simulate_inner(params, seed, scratch, None)
}

/// Instrumented simulation that collects metrics and enforces iteration limits.
///
/// Returns both the simulation result and collected metrics.
pub fn simulate_with_metrics(
    params: &SimulationConfig,
    seed: u64,
    config: &InstrumentationConfig,
) -> Result<(SimulationResult, SimulationMetrics), SimulationError> {
    let mut scratch = SimulationScratch::new();
    let mut metrics = SimulationMetrics::new();
    let result = simulate_inner(params, seed, &mut scratch, Some((config, &mut metrics)))?;
    Ok((result, metrics))
}

/// Unified simulation loop with optional instrumentation.
fn simulate_inner(
    params: &SimulationConfig,
    seed: u64,
    scratch: &mut SimulationScratch,
    mut instrumentation: Option<(&InstrumentationConfig, &mut SimulationMetrics)>,
) -> Result<SimulationResult, SimulationError> {
    let max_iterations = instrumentation
        .as_ref()
        .map_or(1000, |(c, _)| c.max_same_date_iterations);

    let mut state = SimulationState::from_parameters(params, seed)?;
    state.snapshot_wealth();

    while state.timeline.current_date < state.timeline.end_date {
        let mut something_happened = true;
        let mut iteration_count: u64 = 0;

        while something_happened {
            something_happened = false;
            iteration_count += 1;

            if iteration_count > max_iterations {
                if let Some((config, ref mut metrics)) = instrumentation
                    && config.collect_metrics
                {
                    metrics.record_limit_hit(state.timeline.current_date);
                }
                state.warnings.push(SimulationWarning {
                    date: state.timeline.current_date,
                    event_id: None,
                    message: format!(
                        "iteration limit ({max_iterations}) reached, possible infinite loop"
                    ),
                    kind: WarningKind::IterationLimitHit,
                });
                break;
            }

            process_events_with_scratch(&mut state, scratch);
            if !scratch.triggered.is_empty() {
                something_happened = true;

                if let Some((config, ref mut metrics)) = instrumentation
                    && config.collect_metrics
                {
                    for event_id in &scratch.triggered {
                        metrics.record_event_triggered(*event_id);
                    }
                }
            }

            if let Some((config, ref mut metrics)) = instrumentation
                && config.collect_metrics
            {
                metrics.record_iteration(state.timeline.current_date, iteration_count);
            }
        }

        if let Some((config, ref mut metrics)) = instrumentation
            && config.collect_metrics
        {
            metrics.record_time_step();
        }

        advance_time(&mut state);
    }

    state.snapshot_wealth();
    state.finalize_year_taxes();

    Ok(build_simulation_result(&mut state))
}

// ── Time advancement ─────────────────────────────────────────────────

/// Determine the next simulation checkpoint date.
fn find_next_checkpoint(state: &SimulationState) -> jiff::civil::Date {
    let mut next = state.timeline.end_date;

    // Check event dates
    for event in state.event_state.iter_events() {
        if event.once
            && state.event_state.is_triggered(event.event_id)
            && !matches!(event.trigger, EventTrigger::Repeating { .. })
        {
            continue;
        }

        if let EventTrigger::Date(d) = event.trigger
            && d > state.timeline.current_date
            && d < next
        {
            next = d;
        }

        if let EventTrigger::RelativeToEvent {
            event_id: ref_event_id,
            offset,
        } = &event.trigger
            && let Some(trigger_date) = state.event_state.triggered_date(*ref_event_id)
        {
            let d = offset.add_to_date(trigger_date);
            if d > state.timeline.current_date && d < next {
                next = d;
            }
        }
    }

    // Check repeating event scheduled dates
    for date in state.event_state.event_next_date.iter().flatten() {
        if *date > state.timeline.current_date && *date < next {
            next = *date;
        }
    }

    // Heartbeat - advance at least quarterly
    let heartbeat = crate::model::TriggerOffset::Months(3).add_to_date(state.timeline.current_date);
    if heartbeat < next {
        next = heartbeat;
    }

    // Ensure we capture December 31 for RMD year-end balance tracking
    let dec_31 = jiff::civil::date(state.timeline.current_date.year(), 12, 31);
    if state.timeline.current_date < dec_31 && dec_31 < next {
        next = dec_31;
    }

    next
}

/// Compound a single cash balance and optionally record a ledger entry.
#[allow(clippy::too_many_arguments)]
fn compound_cash_balance(
    cash_value: &mut f64,
    return_profile_id: crate::model::ReturnProfileId,
    market: &crate::model::Market,
    year_index: usize,
    days_passed: i32,
    account_id: AccountId,
    checkpoint: jiff::civil::Date,
    collect_ledger: bool,
    ledger: &mut Vec<LedgerEntry>,
) {
    if *cash_value <= 0.0 {
        return;
    }
    if let Ok(multiplier) =
        market.get_period_multiplier(year_index, i64::from(days_passed), return_profile_id)
    {
        let previous_value = *cash_value;
        *cash_value *= multiplier;
        let return_rate = multiplier - 1.0;

        if collect_ledger && (*cash_value - previous_value).abs() > 0.001 {
            ledger.push(LedgerEntry::new(
                checkpoint,
                StateEvent::CashAppreciation {
                    account_id,
                    previous_value,
                    new_value: *cash_value,
                    return_rate,
                    days: days_passed,
                },
            ));
        }
    }
}

/// Apply interest/returns to all accounts for the elapsed time period.
fn compound_accounts(
    state: &mut SimulationState,
    next_checkpoint: jiff::civil::Date,
    days_passed: i32,
) {
    let year_index =
        (state.timeline.current_date.year() - state.timeline.start_date.year()) as usize;

    let account_ids: Vec<AccountId> = state.portfolio.accounts.keys().copied().collect();

    for account_id in account_ids {
        let Some(account) = state.portfolio.accounts.get_mut(&account_id) else {
            continue;
        };
        match &mut account.flavor {
            AccountFlavor::Bank(cash) => {
                compound_cash_balance(
                    &mut cash.value,
                    cash.return_profile_id,
                    &state.portfolio.market,
                    year_index,
                    days_passed,
                    account_id,
                    next_checkpoint,
                    state.collect_ledger,
                    &mut state.history.ledger,
                );
            }
            AccountFlavor::Investment(inv) => {
                compound_cash_balance(
                    &mut inv.cash.value,
                    inv.cash.return_profile_id,
                    &state.portfolio.market,
                    year_index,
                    days_passed,
                    account_id,
                    next_checkpoint,
                    state.collect_ledger,
                    &mut state.history.ledger,
                );
            }
            AccountFlavor::Liability(loan) => {
                if loan.interest_rate > 0.0 {
                    let previous_principal = loan.principal;
                    let multiplier =
                        (1.0 + loan.interest_rate).powf(f64::from(days_passed) / 365.0);
                    loan.principal *= multiplier;

                    if state.collect_ledger && (loan.principal - previous_principal).abs() > 0.001 {
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

/// Capture year-end balances for RMD calculations (December 31).
fn capture_year_end_balances(state: &mut SimulationState, checkpoint: jiff::civil::Date) {
    let year = checkpoint.year();
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

    state.snapshot_wealth();
}

fn advance_time(state: &mut SimulationState) {
    state.maybe_rollover_year();

    let next_checkpoint = find_next_checkpoint(state);
    let days_passed =
        crate::date_math::fast_days_between(state.timeline.current_date, next_checkpoint);

    if days_passed > 0 {
        compound_accounts(state, next_checkpoint, days_passed);
    }

    // Capture year-end balances for RMD calculations (December 31)
    let dec_31 = jiff::civil::date(state.timeline.current_date.year(), 12, 31);
    if next_checkpoint == dec_31 {
        capture_year_end_balances(state, next_checkpoint);
    }

    // Reset monthly contributions on month boundary
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

// ── Online statistics & convergence ──────────────────────────────────

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

struct ConvergenceTracker {
    metric: ConvergenceMetric,
    threshold: f64,
    prev_median: Option<f64>,
    prev_success_rate: Option<f64>,
    prev_percentiles: Option<(f64, f64, f64)>,
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
                if let Some(rse) = online_stats.relative_standard_error() {
                    (rse < self.threshold, Some(rse))
                } else {
                    (false, None)
                }
            }
            ConvergenceMetric::Median => {
                let median_idx = (n as f64 * 0.5).floor() as usize;
                let median = seed_results
                    .get(median_idx.min(n - 1))
                    .map_or(0.0, |(_, v)| *v);

                let relative_change = if let Some(prev) = self.prev_median {
                    relative_change_or_inf(median, prev)
                } else {
                    f64::INFINITY
                };

                self.prev_median = Some(median);
                (relative_change < self.threshold, Some(relative_change))
            }
            ConvergenceMetric::SuccessRate => {
                let success_count = seed_results.iter().filter(|(_, v)| *v > 0.0).count();
                let success_rate = success_count as f64 / n as f64;

                let absolute_change = if let Some(prev) = self.prev_success_rate {
                    (success_rate - prev).abs()
                } else {
                    f64::INFINITY
                };

                self.prev_success_rate = Some(success_rate);
                (absolute_change < self.threshold, Some(absolute_change))
            }
            ConvergenceMetric::Percentiles => {
                let p5 = percentile_value(seed_results, 0.05);
                let p50 = percentile_value(seed_results, 0.50);
                let p95 = percentile_value(seed_results, 0.95);

                let max_relative_change =
                    if let Some((prev_p5, prev_p50, prev_p95)) = self.prev_percentiles {
                        relative_change_or_inf(p5, prev_p5)
                            .max(relative_change_or_inf(p50, prev_p50))
                            .max(relative_change_or_inf(p95, prev_p95))
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

fn relative_change_or_inf(curr: f64, prev: f64) -> f64 {
    if prev.abs() < f64::EPSILON {
        if curr.abs() < f64::EPSILON {
            0.0
        } else {
            f64::INFINITY
        }
    } else {
        ((curr - prev) / prev).abs()
    }
}

fn percentile_value(sorted: &[(u64, f64)], p: f64) -> f64 {
    let n = sorted.len();
    let idx = (n as f64 * p).floor() as usize;
    sorted.get(idx.min(n - 1)).map_or(0.0, |(_, v)| *v)
}

// ── Monte Carlo: unified core ────────────────────────────────────────

/// Internal options controlling Monte Carlo execution behavior.
struct MonteCarloOptions<'a> {
    progress: Option<&'a MonteCarloProgress>,
    compute_mean: bool,
    run_phase2: bool,
}

struct MonteCarloInternalResult {
    stats: MonteCarloStats,
    percentile_runs: Vec<(f64, SimulationResult)>,
    mean_accumulators: Option<MeanAccumulators>,
    percentile_seeds: Vec<(f64, u64)>,
}

/// Core Monte Carlo engine. All three public MC functions delegate here.
fn monte_carlo_core(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
    options: &MonteCarloOptions<'_>,
) -> Result<MonteCarloInternalResult, SimulationError> {
    let batch_size = config.batch_size;
    let parallel_batches = config.parallel_batches;

    // Reset and check progress if tracking
    if let Some(progress) = options.progress {
        progress.reset();
        if progress.is_cancelled() {
            return Err(SimulationError::Cancelled);
        }
    }

    // Validate by running one simulation
    let _ = simulate(params, 0)?;

    if let Some(progress) = options.progress
        && progress.is_cancelled()
    {
        return Err(SimulationError::Cancelled);
    }

    // Disable ledger for batch iterations (only need final_net_worth)
    let mut batch_params = params.clone();
    batch_params.collect_ledger = false;

    let min_iterations = config.iterations;
    let max_iterations = config
        .convergence
        .as_ref()
        .map_or(config.iterations, |c| c.max_iterations);

    let mut convergence_tracker = config
        .convergence
        .as_ref()
        .map(|c| ConvergenceTracker::new(c.metric, c.relative_threshold));

    let mut seed_results: Vec<(u64, f64)> = Vec::new();
    let mut online_stats = OnlineStats::new();
    let mut mean_accumulators: Option<MeanAccumulators> = None;
    let mut batch_seed: u64 = config.seed.unwrap_or_else(|| rand::rng().next_u64());
    let mut converged = false;
    let mut final_convergence_value: Option<f64> = None;

    let cancelled = std::sync::atomic::AtomicBool::new(false);

    loop {
        let current_count = seed_results.len();
        if current_count >= max_iterations {
            break;
        }

        // Check cancellation
        if let Some(progress) = options.progress
            && (cancelled.load(std::sync::atomic::Ordering::Relaxed) || progress.is_cancelled())
        {
            return Err(SimulationError::Cancelled);
        }

        let remaining = max_iterations - current_count;
        let target_this_round = remaining.min(batch_size * parallel_batches);
        let num_batches = target_this_round.div_ceil(batch_size);

        // Set up mean accumulator mutex if needed
        let mean_accumulator: Option<Mutex<Option<MeanAccumulators>>> =
            if options.compute_mean && config.compute_mean {
                Some(Mutex::new(mean_accumulators.take()))
            } else {
                None
            };

        let batch_outputs: Vec<(Vec<(u64, f64)>, OnlineStats)> = (0..num_batches)
            .into_par_iter()
            .map(|local_batch_idx| {
                // Check cancellation at batch start
                if let Some(progress) = options.progress
                    && (cancelled.load(std::sync::atomic::Ordering::Relaxed)
                        || progress.is_cancelled())
                {
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
                    if let Some(progress) = options.progress
                        && progress.is_cancelled()
                    {
                        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }

                    let seed = rng.next_u64();
                    if let Ok(result) = simulate_with_scratch(&batch_params, seed, &mut scratch) {
                        let fnw = final_net_worth(&result);
                        local_stats.add(fnw);
                        local_results.push((seed, fnw));

                        if let Some(ref acc_mutex) = mean_accumulator {
                            let mut acc_guard = match acc_mutex.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };

                            if let Some(acc) = acc_guard.as_mut() {
                                acc.accumulate(&result);
                            } else {
                                let mut new_acc = MeanAccumulators::new(&result);
                                new_acc.accumulate(&result);
                                *acc_guard = Some(new_acc);
                            }
                        }

                        if let Some(progress) = options.progress {
                            progress.increment();
                        }
                    }
                }

                (local_results, local_stats)
            })
            .collect();

        // Merge results
        for (results, stats) in batch_outputs {
            seed_results.extend(results);
            online_stats.merge(&stats);
        }

        batch_seed += num_batches as u64;

        // Extract mean accumulators for next round
        mean_accumulators = mean_accumulator.and_then(|m| match m.into_inner() {
            Ok(opt) => opt,
            Err(poisoned) => poisoned.into_inner(),
        });

        // Check cancellation after batch
        if let Some(progress) = options.progress
            && (cancelled.load(std::sync::atomic::Ordering::Relaxed) || progress.is_cancelled())
        {
            return Err(SimulationError::Cancelled);
        }

        seed_results.sort_by(|a, b| a.1.total_cmp(&b.1));

        // Check convergence
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
            break;
        }
    }

    // Final sort
    seed_results.sort_by(|a, b| a.1.total_cmp(&b.1));

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

    // Phase 2: Re-run percentile seeds for full results (if requested)
    let percentile_runs = if options.run_phase2 {
        percentile_seeds
            .iter()
            .filter_map(|&(p, seed)| simulate(params, seed).ok().map(|result| (p, result)))
            .collect()
    } else {
        Vec::new()
    };

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

    Ok(MonteCarloInternalResult {
        stats,
        percentile_runs,
        mean_accumulators,
        percentile_seeds,
    })
}

// ── Monte Carlo: public API ──────────────────────────────────────────

/// Memory-efficient Monte Carlo simulation.
///
/// Runs simulations in two phases:
/// 1. First pass: Run all iterations, keeping only (seed, `final_net_worth`) and accumulating mean sums
/// 2. Second pass: Re-run only the specific seeds needed for percentile runs
///
/// Supports convergence-based stopping via `config.convergence`.
pub fn monte_carlo_simulate_with_config(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
) -> Result<MonteCarloSummary, SimulationError> {
    let options = MonteCarloOptions {
        progress: None,
        compute_mean: config.compute_mean,
        run_phase2: true,
    };
    let result = monte_carlo_core(params, config, &options)?;
    Ok(MonteCarloSummary {
        stats: result.stats,
        percentile_runs: result.percentile_runs,
        mean_accumulators: result.mean_accumulators,
    })
}

/// Memory-efficient Monte Carlo simulation with progress tracking.
///
/// Identical to `monte_carlo_simulate_with_config` but provides real-time progress
/// updates and cancellation support via `MonteCarloProgress`.
///
/// # Example
/// ```ignore
/// let progress = MonteCarloProgress::new();
/// let progress_clone = progress.clone();
///
/// let handle = std::thread::spawn(move || {
///     monte_carlo_simulate_with_progress(&config, &mc_config, &progress_clone)
/// });
///
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
) -> Result<MonteCarloSummary, SimulationError> {
    let options = MonteCarloOptions {
        progress: Some(progress),
        compute_mean: config.compute_mean,
        run_phase2: true,
    };
    let result = monte_carlo_core(params, config, &options)?;
    Ok(MonteCarloSummary {
        stats: result.stats,
        percentile_runs: result.percentile_runs,
        mean_accumulators: result.mean_accumulators,
    })
}

/// Memory-efficient Monte Carlo simulation that returns only stats and percentile seeds.
///
/// Skips Phase 2 (re-running simulations for percentile results), returning seeds
/// for on-demand reconstruction. Ideal for sweep analysis where storing full results
/// for every grid point would consume excessive memory.
pub fn monte_carlo_stats_only(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
    progress: &MonteCarloProgress,
) -> Result<(MonteCarloStats, Vec<(f64, u64)>), SimulationError> {
    let options = MonteCarloOptions {
        progress: Some(progress),
        compute_mean: false,
        run_phase2: false,
    };
    let result = monte_carlo_core(params, config, &options)?;
    Ok((result.stats, result.percentile_seeds))
}
