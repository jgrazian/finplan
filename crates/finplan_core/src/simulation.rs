#[cfg(feature = "parallel")]
use std::sync::Mutex;

use rustc_hash::FxHashMap;

use crate::apply::{SimulationScratch, process_events_with_scratch};
use crate::config::SimulationConfig;
use crate::error::MarketError;
use crate::metrics::{InstrumentationConfig, SimulationMetrics};
use crate::model::{
    AccountFlavor, AccountId, CashFlowKind, EventTrigger, LedgerEntry, MeanAccumulators,
    MonteCarloConfig, MonteCarloProgress, MonteCarloResult, MonteCarloStats, MonteCarloSummary,
    SimulationResult, SimulationWarning, StateEvent, TaxStatus, WarningKind, YearlyCashFlowSummary,
    final_net_worth,
};
use crate::simulation_state::{SimulationState, cached_spans};
use rand::{RngCore, SeedableRng};

#[cfg(feature = "parallel")]
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
                            if (cash.value - previous_value).abs() > 0.001 {
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
                            if (inv.cash.value - previous_value).abs() > 0.001 {
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
                        if (loan.principal - previous_principal).abs() > 0.001 {
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
        state.history.ledger.push(LedgerEntry::new(
            next_checkpoint,
            StateEvent::TimeAdvance {
                from_date: state.timeline.current_date,
                to_date: next_checkpoint,
                days_elapsed: days_passed,
            },
        ));
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

    #[cfg(feature = "parallel")]
    let iterations: Vec<SimulationResult> = (0..num_batches)
        .into_par_iter()
        .flat_map(|i| {
            let mut rng = rand::rngs::SmallRng::seed_from_u64(i as u64);
            let mut scratch = SimulationScratch::new();
            let batch_size = if i == num_batches - 1 {
                num_iterations - i * MAX_BATCH_SIZE
            } else {
                MAX_BATCH_SIZE
            };
            (0..batch_size)
                .filter_map(|_| {
                    let seed = rng.next_u64();
                    simulate_with_scratch(params, seed, &mut scratch).ok()
                })
                .collect::<Vec<_>>()
        })
        .collect();

    #[cfg(not(feature = "parallel"))]
    let iterations: Vec<SimulationResult> = (0..num_batches)
        .flat_map(|i| {
            let mut rng = rand::rngs::SmallRng::seed_from_u64(i as u64);
            let mut scratch = SimulationScratch::new();
            let batch_size = if i == num_batches - 1 {
                num_iterations - i * MAX_BATCH_SIZE
            } else {
                MAX_BATCH_SIZE
            };
            (0..batch_size)
                .filter_map(|_| {
                    let seed = rng.next_u64();
                    simulate_with_scratch(params, seed, &mut scratch).ok()
                })
                .collect::<Vec<_>>()
        })
        .collect();

    Ok(MonteCarloResult { iterations })
}

/// Memory-efficient Monte Carlo simulation
///
/// This function runs simulations in two phases:
/// 1. First pass: Run all iterations, keeping only (seed, final_net_worth) and accumulating mean sums
/// 2. Second pass: Re-run only the specific seeds needed for percentile runs
///
/// This approach uses O(N) memory for seeds/values instead of O(N * result_size)
pub fn monte_carlo_simulate_with_config(
    params: &SimulationConfig,
    config: &MonteCarloConfig,
) -> Result<MonteCarloSummary, MarketError> {
    let num_iterations = config.iterations;
    const MAX_BATCH_SIZE: usize = 100;
    let num_batches = num_iterations.div_ceil(MAX_BATCH_SIZE);

    // First validate by running one simulation to check for market errors
    // This prevents us from running many iterations only to fail
    let _ = simulate(params, 0)?;

    // Phase 1: Run all iterations, collecting seeds and final net worth
    // Also accumulate mean sums if requested
    #[cfg(feature = "parallel")]
    let mean_accumulator: Option<Mutex<Option<MeanAccumulators>>> = if config.compute_mean {
        Some(Mutex::new(None))
    } else {
        None
    };

    #[cfg(not(feature = "parallel"))]
    let mut mean_accumulator: Option<MeanAccumulators> = None;

    // Collect (seed, final_net_worth) for all iterations
    #[cfg(feature = "parallel")]
    let mut seed_results: Vec<(u64, f64)> = (0..num_batches)
        .into_par_iter()
        .flat_map(|batch_idx| {
            let mut rng = rand::rngs::SmallRng::seed_from_u64(batch_idx as u64);
            let mut scratch = SimulationScratch::new();
            let batch_size = if batch_idx == num_batches - 1 {
                num_iterations - batch_idx * MAX_BATCH_SIZE
            } else {
                MAX_BATCH_SIZE
            };
            (0..batch_size)
                .filter_map(|_| {
                    let seed = rng.next_u64();
                    let result = simulate_with_scratch(params, seed, &mut scratch).ok()?;
                    let fnw = final_net_worth(&result);
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
                    Some((seed, fnw))
                })
                .collect::<Vec<_>>()
        })
        .collect();

    #[cfg(not(feature = "parallel"))]
    let mut seed_results: Vec<(u64, f64)> = (0..num_batches)
        .flat_map(|batch_idx| {
            let mut rng = rand::rngs::SmallRng::seed_from_u64(batch_idx as u64);
            let mut scratch = SimulationScratch::new();
            let batch_size = if batch_idx == num_batches - 1 {
                num_iterations - batch_idx * MAX_BATCH_SIZE
            } else {
                MAX_BATCH_SIZE
            };
            (0..batch_size)
                .filter_map(|_| {
                    let seed = rng.next_u64();
                    let result = simulate_with_scratch(params, seed, &mut scratch).ok()?;
                    let fnw = final_net_worth(&result);
                    if config.compute_mean {
                        match mean_accumulator.as_mut() {
                            Some(acc) => acc.accumulate(&result),
                            None => {
                                let mut new_acc = MeanAccumulators::new(&result);
                                new_acc.accumulate(&result);
                                mean_accumulator = Some(new_acc);
                            }
                        }
                    }
                    Some((seed, fnw))
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // Sort by final net worth (ascending)
    seed_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Calculate statistics
    let actual_iterations = seed_results.len();
    let final_values: Vec<f64> = seed_results.iter().map(|(_, v)| *v).collect();
    let mean_final_net_worth: f64 = if actual_iterations > 0 {
        final_values.iter().sum::<f64>() / actual_iterations as f64
    } else {
        0.0
    };

    let variance: f64 = if actual_iterations > 0 {
        final_values
            .iter()
            .map(|v| (v - mean_final_net_worth).powi(2))
            .sum::<f64>()
            / actual_iterations as f64
    } else {
        0.0
    };
    let std_dev_final_net_worth = variance.sqrt();

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
        .filter_map(|(p, seed)| {
            // Skip failed re-runs (should match initial run)
            simulate(params, seed).ok().map(|result| (p, result))
        })
        .collect();

    // Extract mean accumulators
    #[cfg(feature = "parallel")]
    let mean_accumulators = mean_accumulator.and_then(|m| match m.into_inner() {
        Ok(opt) => opt,
        Err(poisoned) => poisoned.into_inner(),
    });

    #[cfg(not(feature = "parallel"))]
    let mean_accumulators = mean_accumulator;

    let stats = MonteCarloStats {
        num_iterations: actual_iterations,
        success_rate,
        mean_final_net_worth,
        std_dev_final_net_worth,
        min_final_net_worth,
        max_final_net_worth,
        percentile_values,
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
    let num_iterations = config.iterations;
    const MAX_BATCH_SIZE: usize = 100;
    let num_batches = num_iterations.div_ceil(MAX_BATCH_SIZE);

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

    // Phase 1: Run all iterations, collecting seeds and final net worth
    #[cfg(feature = "parallel")]
    let mean_accumulator: Option<Mutex<Option<MeanAccumulators>>> = if config.compute_mean {
        Some(Mutex::new(None))
    } else {
        None
    };

    #[cfg(not(feature = "parallel"))]
    let mut mean_accumulator: Option<MeanAccumulators> = None;

    // Collect (seed, final_net_worth) for all iterations
    #[cfg(feature = "parallel")]
    let (seed_results, was_cancelled) = {
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let results: Vec<(u64, f64)> = (0..num_batches)
            .into_par_iter()
            .flat_map(|batch_idx| {
                if cancelled.load(std::sync::atomic::Ordering::Relaxed) || progress.is_cancelled() {
                    cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                    return Vec::new();
                }
                let mut rng = rand::rngs::SmallRng::seed_from_u64(batch_idx as u64);
                let mut scratch = SimulationScratch::new();
                let batch_size = if batch_idx == num_batches - 1 {
                    num_iterations - batch_idx * MAX_BATCH_SIZE
                } else {
                    MAX_BATCH_SIZE
                };
                (0..batch_size)
                    .filter_map(|_| {
                        if progress.is_cancelled() {
                            cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                            return None;
                        }
                        let seed = rng.next_u64();
                        let result = simulate_with_scratch(params, seed, &mut scratch).ok()?;
                        let fnw = final_net_worth(&result);
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
                        progress.increment();
                        Some((seed, fnw))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        (
            results,
            cancelled.load(std::sync::atomic::Ordering::Relaxed) || progress.is_cancelled(),
        )
    };

    #[cfg(not(feature = "parallel"))]
    let (seed_results, was_cancelled) = {
        let mut results = Vec::new();
        let mut cancelled = false;
        'outer: for batch_idx in 0..num_batches {
            if progress.is_cancelled() {
                cancelled = true;
                break;
            }
            let mut rng = rand::rngs::SmallRng::seed_from_u64(batch_idx as u64);
            let mut scratch = SimulationScratch::new();
            let batch_size = if batch_idx == num_batches - 1 {
                num_iterations - batch_idx * MAX_BATCH_SIZE
            } else {
                MAX_BATCH_SIZE
            };
            for _ in 0..batch_size {
                if progress.is_cancelled() {
                    cancelled = true;
                    break 'outer;
                }
                let seed = rng.next_u64();
                if let Ok(result) = simulate_with_scratch(params, seed, &mut scratch) {
                    let fnw = final_net_worth(&result);
                    if config.compute_mean {
                        match mean_accumulator.as_mut() {
                            Some(acc) => acc.accumulate(&result),
                            None => {
                                let mut new_acc = MeanAccumulators::new(&result);
                                new_acc.accumulate(&result);
                                mean_accumulator = Some(new_acc);
                            }
                        }
                    }
                    progress.increment();
                    results.push((seed, fnw));
                }
            }
        }
        (results, cancelled)
    };

    let mut seed_results = seed_results;

    // Check if simulation was cancelled
    if was_cancelled {
        return Err(MarketError::Cancelled);
    }

    // Sort by final net worth (ascending)
    seed_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Calculate statistics
    let actual_iterations = seed_results.len();
    let final_values: Vec<f64> = seed_results.iter().map(|(_, v)| *v).collect();
    let mean_final_net_worth: f64 = if actual_iterations > 0 {
        final_values.iter().sum::<f64>() / actual_iterations as f64
    } else {
        0.0
    };

    let variance: f64 = if actual_iterations > 0 {
        final_values
            .iter()
            .map(|v| (v - mean_final_net_worth).powi(2))
            .sum::<f64>()
            / actual_iterations as f64
    } else {
        0.0
    };
    let std_dev_final_net_worth = variance.sqrt();

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

    // Extract mean accumulators
    #[cfg(feature = "parallel")]
    let mean_accumulators = mean_accumulator.and_then(|m| match m.into_inner() {
        Ok(opt) => opt,
        Err(poisoned) => poisoned.into_inner(),
    });

    #[cfg(not(feature = "parallel"))]
    let mean_accumulators = mean_accumulator;

    let stats = MonteCarloStats {
        num_iterations: actual_iterations,
        success_rate,
        mean_final_net_worth,
        std_dev_final_net_worth,
        min_final_net_worth,
        max_final_net_worth,
        percentile_values,
    };

    Ok(MonteCarloSummary {
        stats,
        percentile_runs,
        mean_accumulators,
    })
}
