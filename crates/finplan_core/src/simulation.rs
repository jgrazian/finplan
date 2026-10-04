use rustc_hash::FxHashMap;

mod quantiles;
pub use quantiles::RealAccumulator;

use crate::apply::{SimulationScratch, apply_eval_event, process_events_with_scratch};
use crate::config::SimulationConfig;
use crate::error::SimulationError;
use crate::evaluate::evaluate_effect_into;
use crate::metrics::{InstrumentationConfig, SimulationMetrics};
use crate::model::{
    AccountFlavor, AccountId, AmountMode, AssetId, AssetLot, CashFlowKind, ConvergenceMetric,
    EventEffect, EventTrigger, IncomeType, LedgerEntry, LoanDetail, LotMethod, MeanAccumulators,
    MonteCarloConfig, MonteCarloProgress, MonteCarloStats, MonteCarloSummary,
    MonthlyCashFlowSummary, SimulationResult, SimulationWarning, StateEvent, TaxStatus,
    TransferAmount, WarningKind, WithdrawalSources, YearlyCashFlowSummary,
    after_tax_final_net_worth, final_net_worth,
};
use crate::simulation_state::SimulationState;
use rand::{RngCore, SeedableRng};
#[cfg(feature = "parallel")]
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};

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
                CashFlowKind::Expense | CashFlowKind::Tax => summary.expenses += amount,
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
                CashFlowKind::Expense | CashFlowKind::Tax => summary.expenses += amount,
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

    let mut result = SimulationResult {
        wealth_snapshots: std::mem::take(&mut state.portfolio.wealth_snapshots),
        yearly_taxes: std::mem::take(&mut state.taxes.yearly_taxes),
        yearly_cash_flows,
        ledger: std::mem::take(&mut state.history.ledger),
        warnings: std::mem::take(&mut state.warnings),
        cumulative_inflation,
        diagnostics: std::mem::take(&mut state.diagnostics),
    };
    let mut diagnostics = std::mem::take(&mut result.diagnostics);
    diagnostics.observe_snapshots(&result);
    result.diagnostics = diagnostics;
    result
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
    let mut last_shortfall_year = None;

    while state.timeline.current_date < state.timeline.end_date {
        // Loan payments fall before the day's events, so an event reading a
        // balance sees the month's payment already made.
        crate::apply::pay_scheduled_loans(&mut state);
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
                    account_id: None,
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
        settle_funding_policy(&mut state, scratch);
        record_cash_shortfall(
            &mut state,
            &mut cash_shortfall_recorded,
            &mut last_shortfall_year,
        );
        advance_time(&mut state);
    }

    // The final advance can change balances even when there are no more events.
    settle_funding_policy(&mut state, scratch);
    record_cash_shortfall(
        &mut state,
        &mut cash_shortfall_recorded,
        &mut last_shortfall_year,
    );
    state.snapshot_wealth();
    state.finalize_year_taxes();

    Ok(build_simulation_result(&mut state))
}

/// Cover overdrawn bank accounts by selling investments, when the plan has a
/// funding policy in force. Runs after the date's events settle, so event
/// sweeps always go first and the policy only covers what they left short.
///
/// Each deficit is a `Net` sweep through the same evaluate/apply path events
/// use, so lots, tax withholding and penalties are reused; its ledger entries
/// carry no source event. A sweep that fails (nothing to sell) is dropped: the
/// shortfall check that follows records the problem.
fn settle_funding_policy(state: &mut SimulationState, scratch: &mut SimulationScratch) {
    let Some(policy) = &state.funding else {
        return;
    };
    if policy
        .from
        .is_some_and(|from| state.timeline.current_date < from)
    {
        return;
    }
    let sources = WithdrawalSources::Strategy {
        order: policy.order,
        exclude_accounts: policy.exclude_accounts.clone(),
    };

    let mut deficits: Vec<(AccountId, f64)> = state
        .portfolio
        .accounts
        .iter()
        .filter(|(_, account)| matches!(account.flavor, AccountFlavor::Bank(_)))
        .filter_map(|(id, account)| account.cash_balance().map(|balance| (*id, balance)))
        .filter(|(_, balance)| *balance < -0.005)
        .map(|(id, balance)| (id, -balance))
        .collect();
    deficits.sort_by_key(|(id, _)| *id);

    for (account_id, deficit) in deficits {
        let sweep = EventEffect::Sweep {
            sources: sources.clone(),
            to: account_id,
            amount: TransferAmount::fixed(deficit),
            amount_mode: AmountMode::Net,
            lot_method: LotMethod::Fifo,
            income_type: IncomeType::TaxFree,
        };
        scratch.eval_events.clear();
        if evaluate_effect_into(&sweep, state, &mut scratch.eval_events).is_err() {
            scratch.eval_events.clear();
            continue;
        }
        for ee in scratch.eval_events.drain(..) {
            if let Err(e) = apply_eval_event(state, &ee) {
                state.warnings.push(SimulationWarning {
                    date: state.timeline.current_date,
                    event_id: None,
                    message: format!("failed to apply funding policy sale: {e}"),
                    kind: WarningKind::EffectSkipped,
                    account_id: e.account_id(),
                });
            }
        }
    }
}

/// Record settled cash deficits, independently of ledger collection.
/// Every checkpoint feeds the path's diagnostics, but only the first deficit
/// becomes a warning, so an unfunded monthly expense cannot flood results.
fn record_cash_shortfall(
    state: &mut SimulationState,
    recorded: &mut bool,
    last_year: &mut Option<i16>,
) {
    let lowest = state
        .portfolio
        .accounts
        .iter()
        .filter_map(|(id, account)| account.cash_balance().map(|balance| (*id, balance)))
        // Ties go to the lower id, so the account named does not depend on
        // hash-map iteration order.
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    // The last check falls on the end date, the first day after the plan;
    // its deficit belongs to the plan's final year.
    let date = state.timeline.current_date;
    let year_of = if date >= state.timeline.end_date {
        date.yesterday().unwrap_or(date)
    } else {
        date
    };
    state
        .diagnostics
        .observe_cash(date, year_of, lowest, last_year);
    if *recorded {
        return;
    }
    let funding_active = state
        .funding
        .as_ref()
        .is_some_and(|policy| policy.from.is_none_or(|from| date >= from));
    if let Some((account_id, balance)) = lowest
        && balance < -0.005
    {
        *recorded = true;
        state.warnings.push(SimulationWarning {
            date: state.timeline.current_date,
            event_id: None,
            message: if funding_active {
                format!(
                    "A cash account is overdrawn by ${:.2} in nominal dollars after this date's events settle. \
                     The investments the funding policy may sell could not cover the deficit. \
                     Later recovery does not erase this shortfall.",
                    -balance
                )
            } else {
                format!(
                    "A cash account is overdrawn by ${:.2} in nominal dollars after this date's events settle. \
                     Other assets do not automatically fund spending; add a withdrawal or transfer \
                     rule, or reduce spending. Later recovery does not erase this shortfall.",
                    -balance
                )
            },
            kind: WarningKind::CashShortfall,
            account_id: Some(account_id),
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

        scan_trigger_dates(
            &event.trigger,
            state.timeline.birth_date,
            state.timeline.current_date,
            &mut next,
        );

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

    // Loan payment dates
    for account in state.portfolio.accounts.values() {
        if let AccountFlavor::Liability(LoanDetail {
            schedule: Some(schedule),
            ..
        }) = &account.flavor
        {
            let due = schedule.next_due();
            if due > state.timeline.current_date && due < next {
                next = due;
            }
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

/// Include nested static dates so recurring start/end conditions fire on the
/// intended day, even when that day falls between quarterly heartbeats.
fn scan_trigger_dates(
    trigger: &EventTrigger,
    birth_date: jiff::civil::Date,
    current: jiff::civil::Date,
    next: &mut jiff::civil::Date,
) {
    let candidate = match trigger {
        EventTrigger::Date(date) => Some(*date),
        EventTrigger::Age { years, months } => crate::simulation_state::checked_age_date(
            birth_date,
            crate::model::CalendarAge::new(*years, months.unwrap_or(0)),
        )
        .ok(),
        _ => None,
    };
    if let Some(date) = candidate
        && date > current
        && date < *next
    {
        *next = date;
    }
    match trigger {
        EventTrigger::And(children) | EventTrigger::Or(children) => {
            for child in children {
                scan_trigger_dates(child, birth_date, current, next);
            }
        }
        EventTrigger::Repeating {
            start_condition,
            end_condition,
            ..
        } => {
            if let Some(start) = start_condition {
                scan_trigger_dates(start, birth_date, current, next);
            }
            if let Some(end) = end_condition {
                scan_trigger_dates(end, birth_date, current, next);
            }
        }
        _ => {}
    }
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

    // Open the new tax year before the checkpoint's events run, so their
    // income is taxed in, and summarized under, the year it happens in.
    state.maybe_rollover_year();

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

/// Running count, sum and sum of squares of terminal net worth, mergeable
/// across batches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineStats {
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
            // E[x^2] - mean^2 cancels catastrophically when the samples agree
            // (a deterministic plan) and can land just below zero, whose
            // square root is NaN.
            ((self.sum_sq / self.count as f64) - (mean * mean)).max(0.0)
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

// ── Monte Carlo: batch plan ──────────────────────────────────────────
//
// A run is a plan of batches: how many, how big, and which seed each starts
// from. `MonteCarloConfig` fixes the plan, so the result is a function of the
// plan alone. Who executes a batch (a rayon thread, a WebAssembly worker in
// another tab process) and in what order they finish is not an input, which is
// why the pieces below are public and their outputs serializable:
//
//   coordinator.next_round()  ->  [BatchSpec]      what to run next
//   run_batch(prepared, spec) ->  BatchOutput      one worker, any worker
//   coordinator.absorb(outs)                       merged in index order
//   coordinator.finish(..)    ->  MonteCarloSummary
//
// `monte_carlo_core` is that loop with rayon as the workers.

/// One batch of a round: a contiguous run of iterations on its own RNG stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchSpec {
    /// Position within the round. Outputs merge in this order.
    pub index: usize,
    /// The batch's RNG seed: the round's base seed plus `index`.
    pub seed: u64,
    /// How many iterations this batch runs.
    pub iterations: usize,
}

/// What one batch produces, ready to merge.
///
/// Carries only mergeable state (sums, counts, per-iteration terminal values),
/// never a ledger, so it stays small enough to post between workers. Every
/// float serializes exactly, so a batch merged after a trip through a worker
/// equals the same batch merged in place — provided the reader parses floats
/// exactly: `serde_json` needs its `float_roundtrip` feature, whose absence
/// leaves its parser an ulp off now and then. (Structured clone and
/// `serde-wasm-bindgen` pass the `f64` itself and need nothing.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOutput {
    /// The `BatchSpec::index` this was run for.
    pub index: usize,
    /// `(iteration seed, terminal net worth)` in iteration order.
    pub results: Vec<(u64, f64)>,
    /// Each iteration's after-tax terminal net worth, aligned to `results`.
    #[serde(default)]
    pub after_tax: Vec<f64>,
    pub stats: OnlineStats,
    pub mean_accumulators: Option<MeanAccumulators>,
    pub real_accumulator: Option<RealAccumulator>,
    pub funding: crate::model::FundingAccumulator,
}

/// Everything a worker needs to run batches, built once by `prepare_run`.
///
/// Holds no randomness: any two workers that prepare the same plan produce
/// identical batches for identical `BatchSpec`s.
#[derive(Debug, Clone)]
pub struct PreparedRun {
    /// The plan with the ledger off. Funding checks and warnings still run.
    batch_params: SimulationConfig,
    /// The plan's tax-deferred accounts, for each path's after-tax value.
    tax_deferred: Vec<AccountId>,
    /// An empty real-wealth accumulator on the plan's date grid; each batch
    /// starts from a copy. `None` when real quantiles are not wanted.
    real_template: Option<RealAccumulator>,
    compute_means: bool,
}

fn validate_counts(config: &MonteCarloConfig) -> Result<(), SimulationError> {
    if config.iterations == 0 || config.parallel_batches == 0 {
        return Err(SimulationError::Config(
            "iterations and parallel_batches must be positive".into(),
        ));
    }
    Ok(())
}

fn max_iterations(config: &MonteCarloConfig) -> usize {
    config
        .convergence
        .as_ref()
        .map_or(config.iterations, |c| c.max_iterations)
}

fn validate_max_iterations(config: &MonteCarloConfig) -> Result<(), SimulationError> {
    if max_iterations(config) < config.iterations {
        return Err(SimulationError::Config(
            "max_iterations must be at least iterations".into(),
        ));
    }
    Ok(())
}

/// Validate the plan and build what a worker needs to run its batches.
///
/// Runs one simulation (seed 0) as validation, so a plan that cannot simulate
/// fails here, once, rather than in every batch. Collects real net worth
/// quantiles, as `monte_carlo_simulate_with_config` does.
pub fn prepare_run(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
) -> Result<PreparedRun, SimulationError> {
    prepare_run_with(params, config, true)
}

/// `prepare_run`, with the real-wealth accumulator optional. Without it a run
/// has no phase 2, which is what sweeps want (`monte_carlo_stats_only`).
pub fn prepare_run_with(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
    collect_real: bool,
) -> Result<PreparedRun, SimulationError> {
    validate_counts(config)?;
    let template = simulate(params, 0)?;
    validate_max_iterations(config)?;

    let mut batch_params = params.clone();
    batch_params.collect_ledger = false;
    Ok(PreparedRun {
        tax_deferred: batch_params.tax_deferred_accounts(),
        batch_params,
        real_template: collect_real.then(|| RealAccumulator::new(&template)),
        compute_means: config.compute_mean,
    })
}

/// The base seed for a plan: `config.seed`, or a fresh unseeded draw.
///
/// Draw it once per run and hand the same value to the coordinator. On
/// `wasm32` an unseeded plan is a config error (no OS entropy).
pub fn base_seed(config: &MonteCarloConfig) -> Result<u64, SimulationError> {
    match config.seed {
        Some(seed) => Ok(seed),
        None => unseeded_batch_seed(),
    }
}

/// Run one batch of a prepared plan.
///
/// `progress`, when given, counts each finished iteration and is polled for
/// cancellation before each one; a cancelled batch is an error, never a
/// short output, so a partial batch cannot be merged by accident.
pub fn run_batch(
    prepared: &PreparedRun,
    spec: &BatchSpec,
    progress: Option<&MonteCarloProgress>,
) -> Result<BatchOutput, SimulationError> {
    run_batch_observed(prepared, spec, progress, |_| false)
}

/// [`run_batch`], telling `each` how many of the batch's iterations have
/// finished after every one. Returning true from it cancels the batch. It
/// observes and never steers: the output is [`run_batch`]'s, bit for bit.
///
/// For a single-threaded host (a WebAssembly worker) that cannot read
/// [`MonteCarloProgress`]'s atomics while the batch holds the thread, but can
/// post a message from inside the loop.
pub fn run_batch_observed(
    prepared: &PreparedRun,
    spec: &BatchSpec,
    progress: Option<&MonteCarloProgress>,
    mut each: impl FnMut(usize) -> bool,
) -> Result<BatchOutput, SimulationError> {
    let cancelled = || progress.is_some_and(MonteCarloProgress::is_cancelled);
    if cancelled() {
        return Err(SimulationError::Cancelled);
    }

    let mut rng = rand::rngs::SmallRng::seed_from_u64(spec.seed);
    let mut scratch = SimulationScratch::new();
    let mut local_stats = OnlineStats::new();
    let mut local_acc: Option<MeanAccumulators> = None;
    let mut local_real = prepared.real_template.clone();
    let mut local_funding = crate::model::FundingAccumulator::default();
    let mut local_results = Vec::with_capacity(spec.iterations);
    let mut local_after_tax = Vec::with_capacity(spec.iterations);

    for _ in 0..spec.iterations {
        if cancelled() {
            return Err(SimulationError::Cancelled);
        }

        let seed = rng.next_u64();
        // Never silently discard/retry failed iterations: doing so biases
        // both distributions and success rates toward survivors.
        let result = simulate_with_scratch(&prepared.batch_params, seed, &mut scratch)?;
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
        local_funding.add(seed, &result, fnw);
        local_results.push((seed, fnw));
        local_after_tax.push(after_tax_final_net_worth(
            &result,
            &prepared.tax_deferred,
            prepared.batch_params.deferred_tax_rate,
        ));

        if prepared.compute_means {
            if let Some(ref mut acc) = local_acc {
                acc.accumulate(&result);
            } else {
                let mut new_acc = MeanAccumulators::new(&result);
                new_acc.accumulate(&result);
                local_acc = Some(new_acc);
            }
        }

        if let Some(progress) = progress {
            progress.increment();
        }
        if each(local_results.len()) {
            return Err(SimulationError::Cancelled);
        }
    }

    Ok(BatchOutput {
        index: spec.index,
        results: local_results,
        after_tax: local_after_tax,
        stats: local_stats,
        mean_accumulators: local_acc,
        real_accumulator: local_real,
        funding: local_funding,
    })
}

/// Plans rounds of batches, merges their outputs and decides when to stop.
///
/// Holds the run's running state and nothing about *how* batches execute, so
/// the same coordinator drives rayon threads in-process or WebAssembly workers
/// that answer in any order.
pub struct MonteCarloCoordinator {
    parallel_batches: usize,
    batch_size: usize,
    min_iterations: usize,
    max_iterations: usize,
    percentiles: Vec<f64>,
    convergence_metric: Option<ConvergenceMetric>,
    convergence_tracker: Option<ConvergenceTracker>,

    seed_results: Vec<(u64, f64)>,
    /// After-tax terminal net worth per iteration, in no particular order.
    after_tax_results: Vec<f64>,
    online_stats: OnlineStats,
    mean_accumulators: Option<MeanAccumulators>,
    real_accumulator: Option<RealAccumulator>,
    funding: crate::model::FundingAccumulator,

    batch_seed: u64,
    converged: bool,
    final_convergence_value: Option<f64>,
    done: bool,
    /// The round handed out and not yet absorbed.
    pending: Option<Vec<BatchSpec>>,
}

impl MonteCarloCoordinator {
    /// Start a run from `base_seed` (see `base_seed`).
    pub fn new(config: &MonteCarloConfig, base_seed: u64) -> Result<Self, SimulationError> {
        validate_counts(config)?;
        validate_max_iterations(config)?;
        Ok(Self {
            parallel_batches: config.parallel_batches,
            batch_size: config.batch_size,
            min_iterations: config.iterations,
            max_iterations: max_iterations(config),
            percentiles: config.percentiles.clone(),
            convergence_metric: config.convergence.as_ref().map(|c| c.metric),
            convergence_tracker: config
                .convergence
                .as_ref()
                .map(|c| ConvergenceTracker::new(c.metric, c.relative_threshold)),
            seed_results: Vec::new(),
            after_tax_results: Vec::new(),
            online_stats: OnlineStats::new(),
            mean_accumulators: None,
            real_accumulator: None,
            funding: crate::model::FundingAccumulator::default(),
            batch_seed: base_seed,
            converged: false,
            final_convergence_value: None,
            done: false,
            pending: None,
        })
    }

    /// Iterations merged so far.
    #[must_use]
    pub fn completed(&self) -> usize {
        self.seed_results.len()
    }

    /// The next round of batches to run, or `None` when the run is over: the
    /// fixed count is reached, the metric converged, or the ceiling is hit.
    ///
    /// Asking again before `absorb` returns the same round.
    pub fn next_round(&mut self) -> Option<Vec<BatchSpec>> {
        if self.done {
            return None;
        }
        if let Some(pending) = &self.pending {
            return Some(pending.clone());
        }
        let current_count = self.seed_results.len();
        if current_count >= self.max_iterations {
            self.done = true;
            return None;
        }

        // Dispatch a round of work, one batch per core, each core taking an
        // equal share of it. A fixed-count run has one round: everything.
        //
        // A converging run cannot, because the point of it is to stop early —
        // dispatching the whole ceiling would run every iteration before the
        // metric was ever looked at, which is the fixed run it was chosen
        // instead of. So it takes the minimum sample first, then `batch_size`
        // per core, and tests the metric between rounds.
        let round = match self.convergence_tracker {
            Some(_) if current_count < self.min_iterations => self.min_iterations - current_count,
            Some(_) => self.batch_size.max(1) * self.parallel_batches,
            None => usize::MAX,
        };
        let remaining = (self.max_iterations - current_count).min(round);
        let num_batches = self.parallel_batches.min(remaining);
        let per_batch = remaining / num_batches;
        let extra = remaining % num_batches;

        // Distribute remainder across first `extra` batches
        let specs: Vec<BatchSpec> = (0..num_batches)
            .map(|index| BatchSpec {
                index,
                seed: self.batch_seed.wrapping_add(index as u64),
                iterations: per_batch + usize::from(index < extra),
            })
            .collect();
        self.pending = Some(specs.clone());
        Some(specs)
    }

    /// Merge a finished round, one output per `BatchSpec`, in any order.
    ///
    /// Outputs are merged in batch index order whatever order they arrive in:
    /// floating-point sums are not associative, and the result must not depend
    /// on which worker finished first. A round that does not match what
    /// `next_round` handed out (missing, duplicated or short batches) is
    /// rejected without merging anything.
    pub fn absorb(&mut self, mut outputs: Vec<BatchOutput>) -> Result<(), SimulationError> {
        let Some(specs) = &self.pending else {
            return Err(SimulationError::Config(
                "absorb called with no round pending".into(),
            ));
        };
        outputs.sort_by_key(|o| o.index);
        let matches_round = outputs.len() == specs.len()
            && outputs.iter().zip(specs).all(|(o, s)| {
                o.index == s.index
                    && o.results.len() == s.iterations
                    && o.after_tax.len() == s.iterations
            });
        if !matches_round {
            return Err(SimulationError::Config(
                "batch outputs do not match the round".into(),
            ));
        }
        let num_batches = specs.len();
        self.pending = None;

        for out in outputs {
            self.funding.merge(out.funding);
            if let Some(local) = out.real_accumulator {
                match &mut self.real_accumulator {
                    Some(acc) => acc.merge(local),
                    None => self.real_accumulator = Some(local),
                }
            }
            self.seed_results.extend(out.results);
            self.after_tax_results.extend(out.after_tax);
            self.online_stats.merge(&out.stats);
            if let Some(acc) = out.mean_accumulators {
                if let Some(ref mut existing) = self.mean_accumulators {
                    existing.merge(&acc);
                } else {
                    self.mean_accumulators = Some(acc);
                }
            }
        }

        self.batch_seed = self.batch_seed.wrapping_add(num_batches as u64);
        self.seed_results.sort_by(|a, b| a.1.total_cmp(&b.1));

        // Check convergence
        if let Some(ref mut tracker) = self.convergence_tracker {
            if self.seed_results.len() >= self.min_iterations {
                let (is_converged, metric_value) =
                    tracker.check_convergence(&self.seed_results, &self.online_stats);
                self.final_convergence_value = metric_value;
                if is_converged {
                    self.converged = true;
                    self.done = true;
                }
            }
        } else if self.seed_results.len() >= self.min_iterations {
            self.done = true;
        }
        Ok(())
    }

    /// Final statistics, percentile seeds and (with `run_phase2`) the full
    /// re-run of each percentile path and the real-wealth quantiles.
    ///
    /// `params` is the plan with its ledger on: phase 2 re-simulates the
    /// percentile seeds from it. Finishing before `next_round` returns `None`
    /// summarizes what has been absorbed.
    pub fn finish(
        self,
        params: &SimulationConfig,
        run_phase2: bool,
    ) -> Result<MonteCarloSummary, SimulationError> {
        let result = self.finish_inner(run_phase2.then_some(params))?;
        Ok(MonteCarloSummary {
            stats: result.stats,
            percentile_runs: result.percentile_runs,
            mean_accumulators: result.mean_accumulators,
            real_net_worth: result.real_net_worth,
            funding: Some(result.funding),
            percentile_seeds: result.percentile_seeds,
        })
    }

    /// Stats and percentile seeds only: no phase 2, so no plan is needed.
    pub fn finish_stats(self) -> Result<(MonteCarloStats, Vec<(f64, u64)>), SimulationError> {
        let result = self.finish_inner(None)?;
        Ok((result.stats, result.percentile_seeds))
    }

    fn finish_inner(
        mut self,
        phase2_params: Option<&SimulationConfig>,
    ) -> Result<MonteCarloInternalResult, SimulationError> {
        // Final sort
        self.seed_results.sort_by(|a, b| a.1.total_cmp(&b.1));
        let seed_results = &self.seed_results;

        // Calculate final statistics
        let actual_iterations = seed_results.len();
        let final_values: Vec<f64> = seed_results.iter().map(|(_, v)| *v).collect();
        let mean_final_net_worth = self.online_stats.mean();
        let std_dev_final_net_worth = self.online_stats.std_dev();

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
        let mut after_tax_percentile_values = Vec::new();
        self.after_tax_results.sort_by(f64::total_cmp);

        if actual_iterations > 0 {
            for &p in &self.percentiles {
                let idx =
                    ((actual_iterations as f64 * p).floor() as usize).min(actual_iterations - 1);
                let (seed, value) = seed_results[idx];
                percentile_values.push((p, value));
                percentile_seeds.push((p, seed));
                after_tax_percentile_values.push((p, self.after_tax_results[idx]));
            }
        }

        // Phase 2: Re-run percentile seeds for full results (if requested)
        let percentile_runs = match phase2_params {
            Some(params) => percentile_seeds
                .iter()
                .map(|&(p, seed)| simulate(params, seed).map(|result| (p, result)))
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };

        let stats = MonteCarloStats {
            num_iterations: actual_iterations,
            success_rate,
            funding_success_rate: Some(if actual_iterations > 0 {
                self.online_stats.funded_count as f64 / actual_iterations as f64
            } else {
                0.0
            }),
            mean_final_net_worth,
            std_dev_final_net_worth,
            min_final_net_worth,
            max_final_net_worth,
            percentile_values,
            after_tax_percentile_values,
            converged: self.convergence_metric.map(|_| self.converged),
            convergence_metric: self.convergence_metric,
            convergence_value: self.final_convergence_value,
        };

        // Real quantiles belong to phase 2: a stats-only run drops them.
        let real_accumulator = self.real_accumulator.filter(|_| phase2_params.is_some());
        Ok(MonteCarloInternalResult {
            stats,
            percentile_runs,
            mean_accumulators: self.mean_accumulators,
            real_net_worth: real_accumulator.map(RealAccumulator::finish).transpose()?,
            percentile_seeds,
            funding: self.funding.finish(),
        })
    }
}

/// Merge the batches of a one-round plan into a summary.
///
/// `outputs` may arrive in any order. Errors if they do not make up the whole
/// plan: a converging plan whose first round has not converged needs the
/// coordinator, because only it can ask for the next round.
pub fn merge_batches(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
    outputs: Vec<BatchOutput>,
) -> Result<MonteCarloSummary, SimulationError> {
    // The seed only names batches, and these are already run.
    let mut coordinator = MonteCarloCoordinator::new(config, config.seed.unwrap_or(0))?;
    coordinator.next_round();
    coordinator.absorb(outputs)?;
    if coordinator.next_round().is_some() {
        return Err(SimulationError::Config(
            "batches do not complete the plan".into(),
        ));
    }
    coordinator.finish(params, true)
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
    funding: crate::model::FundingDiagnostics,
}

/// Base seed for a run whose `MonteCarloConfig::seed` is `None`.
///
/// Native builds draw it from the OS-seeded thread RNG. `wasm32` has no OS
/// entropy source without JavaScript glue, so there the caller must supply a
/// seed (draw it with `crypto.getRandomValues`) and this is a config error.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::unnecessary_wraps)]
fn unseeded_batch_seed() -> Result<u64, SimulationError> {
    Ok(rand::rng().next_u64())
}

#[cfg(target_arch = "wasm32")]
fn unseeded_batch_seed() -> Result<u64, SimulationError> {
    Err(SimulationError::Config(
        "seed is required on wasm32: no OS entropy source; supply one drawn from crypto.getRandomValues"
            .into(),
    ))
}

/// Run a round's batches on the rayon pool (or in order without `parallel`).
/// Outputs come back in batch index order either way.
fn run_round(
    prepared: &PreparedRun,
    round: Vec<BatchSpec>,
    progress: Option<&MonteCarloProgress>,
) -> Result<Vec<BatchOutput>, SimulationError> {
    #[cfg(feature = "parallel")]
    let specs = round.into_par_iter();
    #[cfg(not(feature = "parallel"))]
    let specs = round.into_iter();
    // Each batch accumulates independently; there is no shared state to lock.
    specs
        .map(|spec| run_batch(prepared, &spec, progress))
        .collect()
}

/// Core Monte Carlo engine. All three public MC functions delegate here.
fn monte_carlo_core(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
    options: &MonteCarloOptions<'_>,
) -> Result<MonteCarloInternalResult, SimulationError> {
    validate_counts(config)?;

    // Reset and check progress if tracking
    if let Some(progress) = options.progress {
        progress.reset();
        if progress.is_cancelled() {
            return Err(SimulationError::Cancelled);
        }
    }

    let prepared = prepare_run_with(params, config, options.run_phase2)?;

    if let Some(progress) = options.progress
        && progress.is_cancelled()
    {
        return Err(SimulationError::Cancelled);
    }

    let mut coordinator = MonteCarloCoordinator::new(config, base_seed(config)?)?;
    while let Some(round) = coordinator.next_round() {
        let outputs = run_round(&prepared, round, options.progress)?;

        // Check cancellation after the round
        if let Some(progress) = options.progress
            && progress.is_cancelled()
        {
            return Err(SimulationError::Cancelled);
        }

        coordinator.absorb(outputs)?;
    }

    coordinator.finish_inner(options.run_phase2.then_some(params))
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
        funding: Some(result.funding),
        percentile_seeds: result.percentile_seeds,
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
        funding: Some(result.funding),
        percentile_seeds: result.percentile_seeds,
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
