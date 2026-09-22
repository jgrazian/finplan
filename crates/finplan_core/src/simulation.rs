use rustc_hash::FxHashMap;

mod quantiles;
use quantiles::RealAccumulator;

use crate::apply::{SimulationScratch, process_events_with_scratch};
use crate::config::SimulationConfig;
use crate::error::SimulationError;
use crate::metrics::{InstrumentationConfig, SimulationMetrics};
use crate::model::{
    AccountFlavor, AccountId, AssetId, AssetLot, CashFlowKind, ConvergenceMetric, EventTrigger,
    LedgerEntry, MeanAccumulators, MonteCarloConfig, MonteCarloProgress, MonteCarloStats,
    MonteCarloSummary, MonthlyCashFlowSummary, SimulationResult, SimulationWarning, StateEvent,
    TaxStatus, WarningKind, YearlyCashFlowSummary, final_net_worth,
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
    let mut cash_shortfall_recorded = false;

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

        // Expense and funding events may fire in separate same-date passes.
        // Test only once they have all settled, not between individual effects.
        record_cash_shortfall(&mut state, &mut cash_shortfall_recorded);
        advance_time(&mut state);
    }

    // The final advance can change balances even when there are no more events.
    record_cash_shortfall(&mut state, &mut cash_shortfall_recorded);
    state.snapshot_wealth();
    state.finalize_year_taxes();

    Ok(build_simulation_result(&mut state))
}

/// Record the first settled cash deficit, independently of ledger collection.
/// Keep one warning per path so an unfunded monthly expense cannot flood results.
fn record_cash_shortfall(state: &mut SimulationState, recorded: &mut bool) {
    if *recorded {
        return;
    }
    let lowest = state
        .portfolio
        .accounts
        .values()
        .filter_map(crate::model::Account::cash_balance)
        .min_by(f64::total_cmp);
    if let Some(balance) = lowest
        && balance < -0.005
    {
        *recorded = true;
        state.warnings.push(SimulationWarning {
            date: state.timeline.current_date,
            event_id: None,
            message: format!(
                "A cash account is overdrawn by ${:.2} in nominal dollars after this date's events settle. \
                 Other assets do not automatically fund spending; add a withdrawal or transfer \
                 rule, or reduce spending. Later recovery does not erase this shortfall.",
                -balance
            ),
            kind: WarningKind::CashShortfall,
        });
    }
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

    // Split borrows: market (read) vs accounts (write) vs ledger (write)
    let market = &state.portfolio.market;
    let collect_ledger = state.collect_ledger;
    let ledger = &mut state.history.ledger;

    for (&account_id, account) in &mut state.portfolio.accounts {
        match &mut account.flavor {
            AccountFlavor::Bank(cash) => {
                compound_cash_balance(
                    &mut cash.value,
                    cash.return_profile_id,
                    market,
                    year_index,
                    days_passed,
                    account_id,
                    next_checkpoint,
                    collect_ledger,
                    ledger,
                );
            }
            AccountFlavor::Investment(inv) => {
                compound_cash_balance(
                    &mut inv.cash.value,
                    inv.cash.return_profile_id,
                    market,
                    year_index,
                    days_passed,
                    account_id,
                    next_checkpoint,
                    collect_ledger,
                    ledger,
                );
            }
            AccountFlavor::Liability(loan) => {
                if loan.interest_rate > 0.0 {
                    let previous_principal = loan.principal;
                    let multiplier =
                        (1.0 + loan.interest_rate).powf(f64::from(days_passed) / 365.0);
                    loan.principal *= multiplier;

                    if collect_ledger && (loan.principal - previous_principal).abs() > 0.001 {
                        ledger.push(LedgerEntry::new(
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

/// Consolidate asset lots older than 1 year into per-(asset, year) annual lots.
///
/// Lots with `purchase_date.year() <= current_year - 2` are guaranteed to be
/// long-term (>365 days held) regardless of their month, so merging them
/// preserves tax classification accuracy. Each group is replaced by a single
/// lot dated Jul 1 of that year.
fn consolidate_lots(state: &mut SimulationState, cutoff_year: i16) {
    if !state.portfolio.needs_lot_consolidation {
        return;
    }

    let mut consolidated_any = false;

    for account in state.portfolio.accounts.values_mut() {
        if let AccountFlavor::Investment(inv) = &mut account.flavor {
            // Partition: keep individual lots from recent years, consolidate old ones
            let mut old: Vec<AssetLot> = Vec::new();
            let mut recent: Vec<AssetLot> = Vec::new();

            for lot in inv.positions.drain(..) {
                if lot.purchase_date.year() <= cutoff_year {
                    old.push(lot);
                } else {
                    recent.push(lot);
                }
            }

            if old.is_empty() {
                inv.positions = recent;
                continue;
            }
            consolidated_any = true;

            // Group old lots by (asset_id, year) and merge each group
            let mut groups: FxHashMap<(AssetId, i16), (f64, f64)> = FxHashMap::default();
            for lot in &old {
                let key = (lot.asset_id, lot.purchase_date.year());
                let entry = groups.entry(key).or_insert((0.0, 0.0));
                entry.0 += lot.units;
                entry.1 += lot.cost_basis;
            }

            inv.positions = recent;
            for ((asset_id, year), (units, cost_basis)) in groups {
                inv.positions.push(AssetLot {
                    asset_id,
                    purchase_date: jiff::civil::date(year, 7, 1),
                    units,
                    cost_basis,
                });
            }
        }
    }

    if consolidated_any {
        state.portfolio.needs_lot_consolidation = false;
    }
    // If nothing was old enough to consolidate, keep flag true —
    // those lots will become eligible in a future year.
}

fn advance_time(state: &mut SimulationState) {
    state.maybe_rollover_year();

    let previous = state.timeline.current_date;
    let next_checkpoint = find_next_checkpoint(state);
    let days_passed = crate::date_math::fast_days_between(previous, next_checkpoint);

    if days_passed > 0 {
        compound_accounts(state, next_checkpoint, days_passed);
    }

    // The clock moves before anything reads it.
    //
    // Cash has already been compounded to the checkpoint and asset prices are
    // looked up by date, so anything measured while the clock still reads
    // `previous` mixes two dates: December's balances at December the 1st's
    // prices, filed under December the 1st. What made that more than untidy is
    // that the preceding checkpoint moves with the event schedule, so on a plan
    // whose events trigger off balances or net worth the year's snapshot landed
    // on a different date in every iteration.
    state.timeline.current_date = next_checkpoint;

    // Capture year-end balances for RMD calculations (December 31)
    let dec_31 = jiff::civil::date(previous.year(), 12, 31);
    if next_checkpoint == dec_31 {
        capture_year_end_balances(state, next_checkpoint);
    }

    // Reset monthly contributions on month boundary
    if previous.month() != next_checkpoint.month() || previous.year() != next_checkpoint.year() {
        state.reset_monthly_contributions();
    }

    // Reset yearly contributions and consolidate lots on year boundary
    if previous.year() != next_checkpoint.year() {
        state.portfolio.contributions_ytd.clear();
        // Counted from the year being left, so which lots are old enough to
        // merge does not depend on whether the clock has already ticked over.
        consolidate_lots(state, previous.year() - 2);
    }
}

// ── Online statistics & convergence ──────────────────────────────────

struct OnlineStats {
    count: usize,
    funded_count: usize,
    sum: f64,
    sum_sq: f64,
}

impl OnlineStats {
    fn new() -> Self {
        Self {
            count: 0,
            funded_count: 0,
            sum: 0.0,
            sum_sq: 0.0,
        }
    }

    fn add(&mut self, value: f64, funded: bool) {
        self.count += 1;
        self.funded_count += usize::from(funded);
        self.sum += value;
        self.sum_sq += value * value;
    }

    fn merge(&mut self, other: &OnlineStats) {
        self.count += other.count;
        self.funded_count += other.funded_count;
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
    run_phase2: bool,
}

struct MonteCarloInternalResult {
    stats: MonteCarloStats,
    percentile_runs: Vec<(f64, SimulationResult)>,
    mean_accumulators: Option<MeanAccumulators>,
    real_net_worth: Option<crate::model::RealNetWorthSummary>,
    percentile_seeds: Vec<(f64, u64)>,
}

/// Core Monte Carlo engine. All three public MC functions delegate here.
fn monte_carlo_core(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
    options: &MonteCarloOptions<'_>,
) -> Result<MonteCarloInternalResult, SimulationError> {
    if config.iterations == 0 || config.parallel_batches == 0 {
        return Err(SimulationError::Config(
            "iterations and parallel_batches must be positive".into(),
        ));
    }
    let parallel_batches = config.parallel_batches;

    // Reset and check progress if tracking
    if let Some(progress) = options.progress {
        progress.reset();
        if progress.is_cancelled() {
            return Err(SimulationError::Cancelled);
        }
    }

    // Validate by running one simulation
    let template = simulate(params, 0)?;
    let mut real_accumulator = options.run_phase2.then(|| RealAccumulator::new(&template));

    if let Some(progress) = options.progress
        && progress.is_cancelled()
    {
        return Err(SimulationError::Cancelled);
    }

    // Funding checks and warnings still run without the ledger.
    let mut batch_params = params.clone();
    batch_params.collect_ledger = false;

    let min_iterations = config.iterations;
    let max_iterations = config
        .convergence
        .as_ref()
        .map_or(config.iterations, |c| c.max_iterations);

    if max_iterations < min_iterations {
        return Err(SimulationError::Config(
            "max_iterations must be at least iterations".into(),
        ));
    }

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

    let compute_means = config.compute_mean;

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

        // Dispatch a round of work, one batch per core, each core taking an
        // equal share of it. A fixed-count run has one round: everything.
        //
        // A converging run cannot, because the point of it is to stop early —
        // dispatching the whole ceiling would run every iteration before the
        // metric was ever looked at, which is the fixed run it was chosen
        // instead of. So it takes the minimum sample first, then `batch_size`
        // per core, and tests the metric between rounds.
        let round = match convergence_tracker {
            Some(_) if current_count < min_iterations => min_iterations - current_count,
            Some(_) => config.batch_size.max(1) * parallel_batches,
            None => usize::MAX,
        };
        let remaining = (max_iterations - current_count).min(round);
        let num_batches = parallel_batches.min(remaining);
        let per_batch = remaining / num_batches;
        let extra = remaining % num_batches;

        // Each batch returns its results, stats, and optional local mean accumulator.
        // No shared Mutex — each thread accumulates independently, merge after.
        type BatchOutput = (
            Vec<(u64, f64)>,
            OnlineStats,
            Option<MeanAccumulators>,
            Option<RealAccumulator>,
        );
        let batch_outputs: Result<Vec<BatchOutput>, SimulationError> = (0..num_batches)
            .into_par_iter()
            .map(|local_batch_idx| {
                if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(SimulationError::Cancelled);
                }

                let mut rng =
                    rand::rngs::SmallRng::seed_from_u64(batch_seed + local_batch_idx as u64);
                let mut scratch = SimulationScratch::new();
                let mut local_stats = OnlineStats::new();
                let mut local_acc: Option<MeanAccumulators> = None;
                let mut local_real = options.run_phase2.then(|| RealAccumulator::new(&template));

                // Distribute remainder across first `extra` batches
                let this_batch_size = per_batch + if local_batch_idx < extra { 1 } else { 0 };
                let mut local_results = Vec::with_capacity(this_batch_size);

                for _ in 0..this_batch_size {
                    if let Some(progress) = options.progress
                        && progress.is_cancelled()
                    {
                        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }

                    let seed = rng.next_u64();
                    {
                        // Never silently discard/retry failed iterations: doing so biases
                        // both distributions and success rates toward survivors.
                        let result = simulate_with_scratch(&batch_params, seed, &mut scratch)?;
                        let fnw = final_net_worth(&result);
                        if !fnw.is_finite() {
                            return Err(SimulationError::Config(
                                "nonfinite terminal net worth".into(),
                            ));
                        }
                        if let Some(acc) = &mut local_real {
                            acc.accumulate(&result)?;
                        }
                        // A skipped/failed effect is not evidence that the plan was funded.
                        local_stats.add(fnw, result.warnings.is_empty());
                        local_results.push((seed, fnw));

                        if compute_means {
                            if let Some(ref mut acc) = local_acc {
                                acc.accumulate(&result);
                            } else {
                                let mut new_acc = MeanAccumulators::new(&result);
                                new_acc.accumulate(&result);
                                local_acc = Some(new_acc);
                            }
                        }

                        if let Some(progress) = options.progress {
                            progress.increment();
                        }
                    }
                }

                Ok((local_results, local_stats, local_acc, local_real))
            })
            .collect();

        // Merge results from all batches (single-threaded, fast)
        for (results, stats, local_acc, local_real) in batch_outputs? {
            if let (Some(acc), Some(local)) = (&mut real_accumulator, local_real) {
                acc.merge(local);
            }
            seed_results.extend(results);
            online_stats.merge(&stats);
            if let Some(acc) = local_acc {
                if let Some(ref mut existing) = mean_accumulators {
                    existing.merge(&acc);
                } else {
                    mean_accumulators = Some(acc);
                }
            }
        }

        batch_seed += num_batches as u64;

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
            .map(|&(p, seed)| simulate(params, seed).map(|result| (p, result)))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };

    let stats = MonteCarloStats {
        num_iterations: actual_iterations,
        success_rate,
        funding_success_rate: Some(if actual_iterations > 0 {
            online_stats.funded_count as f64 / actual_iterations as f64
        } else {
            0.0
        }),
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
        real_net_worth: real_accumulator.map(RealAccumulator::finish).transpose()?,
        percentile_seeds,
    })
}

// ── Monte Carlo: public API ──────────────────────────────────────────

/// Memory-efficient Monte Carlo simulation.
///
/// Runs simulations in two phases:
/// 1. First pass: Keep (seed, nominal terminal wealth), real annual vectors and optional mean sums
/// 2. Second pass: Re-run only the specific seeds needed for percentile runs
///
/// Supports convergence-based stopping via `config.convergence`.
pub fn monte_carlo_simulate_with_config(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
) -> Result<MonteCarloSummary, SimulationError> {
    let options = MonteCarloOptions {
        progress: None,
        run_phase2: true,
    };
    let result = monte_carlo_core(params, config, &options)?;
    Ok(MonteCarloSummary {
        stats: result.stats,
        percentile_runs: result.percentile_runs,
        mean_accumulators: result.mean_accumulators,
        real_net_worth: result.real_net_worth,
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
        run_phase2: true,
    };
    let result = monte_carlo_core(params, config, &options)?;
    Ok(MonteCarloSummary {
        stats: result.stats,
        percentile_runs: result.percentile_runs,
        mean_accumulators: result.mean_accumulators,
        real_net_worth: result.real_net_worth,
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
    let mut stats_config = config.clone();
    stats_config.compute_mean = false;
    let options = MonteCarloOptions {
        progress: Some(progress),
        run_phase2: false,
    };
    let result = monte_carlo_core(params, &stats_config, &options)?;
    Ok((result.stats, result.percentile_seeds))
}
