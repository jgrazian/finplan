use crate::config::SimulationParameters;
use crate::event_engine::process_events;
use crate::model::{
    AccountId, AccountType, AssetId, CashFlow, CashFlowDirection, CashFlowId, CashFlowState,
    EventTrigger, LimitPeriod, MonteCarloResult, Record, RepeatInterval, SimulationResult,
    SpendingTarget, SpendingTargetId, SpendingTargetState, TriggerOffset, WithdrawalStrategy,
};
use crate::simulation_state::SimulationState;
use crate::taxes::{calculate_withdrawal_tax, gross_up_for_net_target};

use jiff::ToSpan;
use rand::{RngCore, SeedableRng};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::collections::HashMap;

pub fn n_day_rate(yearly_rate: f64, n_days: f64) -> f64 {
    (1.0 + yearly_rate).powf(n_days / 365.0) - 1.0
}

pub fn simulate(params: &SimulationParameters, seed: u64) -> SimulationResult {
    let mut state = SimulationState::from_parameters(params, seed);

    while state.current_date < state.end_date {
        let mut something_happened = true;
        while something_happened {
            something_happened = false;

            // Process events first - this may activate/pause cash flows and spending targets
            if !process_events(&mut state).is_empty() {
                something_happened = true;
            }

            // Process spending targets
            if apply_spending_targets(&mut state, params) {
                something_happened = true;
            }

            // Process cash flows
            if apply_cash_flows(&mut state, params) {
                something_happened = true;
            }
        }

        advance_time(&mut state, params);
    }

    // Finalize last year's taxes
    state.finalize_year_taxes();

    SimulationResult {
        yearly_inflation: state.inflation_rates.clone(),
        dates: state.dates.clone(),
        return_profile_returns: state.return_profile_returns.clone(),
        accounts: state.build_account_snapshots(params),
        yearly_taxes: state.yearly_taxes.clone(),
        records: state.records.clone(),
    }
}

fn apply_spending_targets(state: &mut SimulationState, params: &SimulationParameters) -> bool {
    state.maybe_rollover_year();

    let mut something_happened = false;

    // Collect spending targets to process (avoid borrow issues)
    let targets_to_process: Vec<(SpendingTargetId, SpendingTarget)> = state
        .spending_targets
        .iter()
        .filter(|(_, (_, st_state))| *st_state == SpendingTargetState::Active)
        .map(|(id, (st, _))| (*id, st.clone()))
        .collect();

    for (st_id, st) in targets_to_process {
        // Check if scheduled for today
        let next_date = state.spending_target_next_date.get(&st_id).copied();
        let Some(date) = next_date else {
            continue;
        };

        if date > state.current_date {
            continue;
        }

        // Calculate target amount (with inflation adjustment)
        let target_amount = state.inflation_adjusted_amount(
            st.amount,
            st.adjust_for_inflation,
            params.duration_years,
        );

        // Execute withdrawal strategy
        let withdrawal_order = get_withdrawal_order(state, params, &st);
        let mut remaining_target = target_amount;

        for (account_id, asset_id) in withdrawal_order {
            if remaining_target <= 0.0 {
                break;
            }

            // Find account type
            let account_type = params
                .accounts
                .iter()
                .find(|a| a.account_id == account_id)
                .map(|a| &a.account_type)
                .unwrap_or(&AccountType::Taxable);

            // Get available balance
            let available = state.asset_balance(account_id, asset_id).max(0.0);

            if available <= 0.0 {
                continue;
            }

            // Calculate how much to withdraw
            let gross_withdrawal = if st.net_amount_mode {
                // Need to gross up for taxes
                let gross = gross_up_for_net_target(
                    remaining_target,
                    account_type,
                    &params.tax_config,
                    state.ytd_tax.ordinary_income,
                )
                .unwrap_or(remaining_target);
                gross.min(available)
            } else {
                remaining_target.min(available)
            };

            // Calculate taxes on this withdrawal
            let tax_result = calculate_withdrawal_tax(
                gross_withdrawal,
                account_type,
                &params.tax_config,
                state.ytd_tax.ordinary_income,
            );

            // Apply the withdrawal to the account
            if let Some(balance) = state.asset_balances.get_mut(&(account_id, asset_id)) {
                *balance -= gross_withdrawal;
            }

            // Track taxes
            match account_type {
                AccountType::TaxDeferred => {
                    state.ytd_tax.ordinary_income += gross_withdrawal;
                }
                AccountType::Taxable => {
                    state.ytd_tax.capital_gains +=
                        gross_withdrawal * params.tax_config.taxable_gains_percentage;
                }
                AccountType::TaxFree => {
                    state.ytd_tax.tax_free_withdrawals += gross_withdrawal;
                }
                AccountType::Illiquid => {}
            }
            state.ytd_tax.federal_tax += tax_result.federal_tax;
            state.ytd_tax.state_tax += tax_result.state_tax + tax_result.capital_gains_tax;

            // Record the withdrawal
            state.records.push(Record::withdrawal(
                state.current_date,
                st.spending_target_id,
                account_id,
                asset_id,
                gross_withdrawal,
                tax_result.federal_tax,
                tax_result.state_tax + tax_result.capital_gains_tax,
                tax_result.net_amount,
            ));

            // Update remaining target
            if st.net_amount_mode {
                remaining_target -= tax_result.net_amount;
            } else {
                remaining_target -= gross_withdrawal;
            }

            something_happened = true;
        }

        // Schedule next occurrence
        let next = match &st.repeats {
            RepeatInterval::Never => None,
            interval => Some(date.saturating_add(interval.span())),
        };

        if let Some(next_date) = next {
            state.spending_target_next_date.insert(st_id, next_date);
        } else {
            state.spending_target_next_date.remove(&st_id);
        }
    }

    something_happened
}

/// Get the order of (account_id, asset_id) pairs to withdraw from based on strategy
fn get_withdrawal_order(
    state: &SimulationState,
    params: &SimulationParameters,
    target: &SpendingTarget,
) -> Vec<(AccountId, AssetId)> {
    // Build list of all liquid accounts/assets with their balances
    let mut candidates: Vec<(AccountId, AssetId, &AccountType, f64)> = Vec::new();

    for account in &params.accounts {
        // Skip illiquid and excluded accounts
        if matches!(account.account_type, AccountType::Illiquid) {
            continue;
        }
        if target.exclude_accounts.contains(&account.account_id) {
            continue;
        }

        // Get current balances from state
        for asset in &account.assets {
            let balance = state.asset_balance(account.account_id, asset.asset_id);

            if balance > 0.0 {
                candidates.push((
                    account.account_id,
                    asset.asset_id,
                    &account.account_type,
                    balance,
                ));
            }
        }
    }

    match &target.withdrawal_strategy {
        WithdrawalStrategy::Sequential { order } => {
            // Sort by the provided order
            candidates.sort_by(|a, b| {
                let a_idx = order.iter().position(|id| *id == a.0).unwrap_or(usize::MAX);
                let b_idx = order.iter().position(|id| *id == b.0).unwrap_or(usize::MAX);
                a_idx.cmp(&b_idx)
            });
        }
        WithdrawalStrategy::ProRata => {
            // For pro-rata, we'll handle proportional withdrawal in the main loop
            // For now, just sort by balance descending
            candidates.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
        }
        WithdrawalStrategy::TaxOptimized => {
            // Tax-optimized order: Taxable first (cap gains), then TaxDeferred, then TaxFree
            candidates.sort_by(|a, b| {
                let type_order = |t: &AccountType| match t {
                    AccountType::Taxable => 0,
                    AccountType::TaxDeferred => 1,
                    AccountType::TaxFree => 2,
                    AccountType::Illiquid => 3,
                };
                type_order(a.2).cmp(&type_order(b.2))
            });
        }
    }

    candidates.into_iter().map(|(a, b, _, _)| (a, b)).collect()
}

fn apply_cash_flows(state: &mut SimulationState, params: &SimulationParameters) -> bool {
    let mut something_happened = false;

    // Collect cash flows to process (avoid borrow issues)
    let flows_to_process: Vec<(CashFlowId, CashFlow)> = state
        .cash_flows
        .iter()
        .filter(|(_, (_, cf_state))| *cf_state == CashFlowState::Active)
        .map(|(id, (cf, _))| (*id, cf.clone()))
        .collect();

    for (cf_id, cf) in flows_to_process {
        // Check if scheduled for today
        let next_date = state.cash_flow_next_date.get(&cf_id).copied();
        let Some(date) = next_date else {
            continue;
        };

        if date > state.current_date {
            continue;
        }

        // Calculate amount with inflation adjustment
        let mut amount = state.inflation_adjusted_amount(
            cf.amount,
            cf.adjust_for_inflation,
            params.duration_years,
        );

        // Apply limits if present
        if let Some(limits) = &cf.cash_flow_limits {
            let current_year = state.current_date.year();
            let period_key = match limits.limit_period {
                LimitPeriod::Yearly => current_year,
                LimitPeriod::Lifetime => 0,
            };

            let last_period_key = state
                .cash_flow_last_period_key
                .get(&cf_id)
                .copied()
                .unwrap_or(current_year);

            if period_key != last_period_key {
                state.cash_flow_ytd.remove(&cf_id);
                state.cash_flow_last_period_key.insert(cf_id, period_key);
            }

            let accumulated = if limits.limit_period == LimitPeriod::Lifetime {
                state.cash_flow_lifetime.get(&cf_id).copied().unwrap_or(0.0)
            } else {
                state.cash_flow_ytd.get(&cf_id).copied().unwrap_or(0.0)
            };

            let magnitude = amount.abs();
            let remaining = limits.limit - accumulated;
            let allowed_magnitude = magnitude.min(remaining.max(0.0));

            if allowed_magnitude < magnitude {
                amount = amount.signum() * allowed_magnitude;
            }

            // Track accumulated amount
            if limits.limit_period == LimitPeriod::Lifetime {
                *state.cash_flow_lifetime.entry(cf_id).or_insert(0.0) += allowed_magnitude;
            } else {
                *state.cash_flow_ytd.entry(cf_id).or_insert(0.0) += allowed_magnitude;
            }
        }

        // Apply the cash flow based on direction
        match &cf.direction {
            CashFlowDirection::Income {
                target_account_id,
                target_asset_id,
            } => {
                if let Some(balance) = state
                    .asset_balances
                    .get_mut(&(*target_account_id, *target_asset_id))
                {
                    *balance += amount;
                }

                // Record as contribution (positive amount)
                state.records.push(Record::cash_flow(
                    state.current_date,
                    cf_id,
                    *target_account_id,
                    *target_asset_id,
                    amount,
                ));
            }

            CashFlowDirection::Expense {
                source_account_id,
                source_asset_id,
            } => {
                if let Some(balance) = state
                    .asset_balances
                    .get_mut(&(*source_account_id, *source_asset_id))
                {
                    *balance -= amount;
                }

                // Record as expense (negative amount)
                state.records.push(Record::cash_flow(
                    state.current_date,
                    cf_id,
                    *source_account_id,
                    *source_asset_id,
                    -amount,
                ));
            }
        }

        // Schedule next occurrence
        let next = match &cf.repeats {
            RepeatInterval::Never => None,
            interval => Some(date.saturating_add(interval.span())),
        };

        if let Some(next_date) = next {
            state.cash_flow_next_date.insert(cf_id, next_date);
        } else {
            state.cash_flow_next_date.remove(&cf_id);
            // Mark as terminated if it was a one-time flow
            if let Some((_, cf_state)) = state.cash_flows.get_mut(&cf_id) {
                *cf_state = CashFlowState::Terminated;
            }
        }

        something_happened = true;
    }

    something_happened
}

fn advance_time(state: &mut SimulationState, params: &SimulationParameters) {
    // Check for year rollover before advancing
    state.maybe_rollover_year();

    // Find next checkpoint
    let mut next_checkpoint = state.end_date;

    // Check cash flow next dates
    for date in state.cash_flow_next_date.values() {
        if *date > state.current_date && *date < next_checkpoint {
            next_checkpoint = *date;
        }
    }

    // Check spending target next dates
    for date in state.spending_target_next_date.values() {
        if *date > state.current_date && *date < next_checkpoint {
            next_checkpoint = *date;
        }
    }

    // Check event dates
    for event in state.events.values() {
        // Skip if already triggered and once=true (unless Repeating)
        if event.once
            && state.triggered_events.contains_key(&event.event_id)
            && !matches!(event.trigger, EventTrigger::Repeating { .. })
        {
            continue;
        }

        if let EventTrigger::Date(d) = event.trigger
            && d > state.current_date
            && d < next_checkpoint
        {
            next_checkpoint = d;
        }

        // Also check relative events
        if let EventTrigger::RelativeToEvent { event_id, offset } = &event.trigger
            && let Some(trigger_date) = state.triggered_events.get(event_id)
        {
            let target_date = match offset {
                TriggerOffset::Days(d) => trigger_date.checked_add((*d as i64).days()),
                TriggerOffset::Months(m) => trigger_date.checked_add((*m as i64).months()),
                TriggerOffset::Years(y) => trigger_date.checked_add((*y as i64).years()),
            };
            if let Ok(d) = target_date
                && d > state.current_date
                && d < next_checkpoint
            {
                next_checkpoint = d;
            }
        }
    }

    // Check repeating event scheduled dates
    for date in state.event_next_date.values() {
        if *date > state.current_date && *date < next_checkpoint {
            next_checkpoint = *date;
        }
    }

    // Heartbeat - advance at least monthly
    let heartbeat = state.current_date.saturating_add(1.month());
    if heartbeat < next_checkpoint {
        next_checkpoint = heartbeat;
    }

    // Ensure we capture December 31 for RMD year-end balance tracking
    let current_year = state.current_date.year();
    let dec_31 = jiff::civil::date(current_year, 12, 31);
    if state.current_date < dec_31 && dec_31 < next_checkpoint {
        next_checkpoint = dec_31;
    }

    // Apply interest/returns
    let days_passed = (next_checkpoint - state.current_date).get_days();
    if days_passed > 0 {
        let years_passed = (state.current_date - state.start_date).get_days() as f64 / 365.0;
        let year_idx = (years_passed.floor() as usize).min(params.duration_years.saturating_sub(1));

        // Apply returns to each asset
        for account in &params.accounts {
            for asset in &account.assets {
                if asset.return_profile_index < state.return_profile_returns.len()
                    && year_idx < state.return_profile_returns[asset.return_profile_index].len()
                {
                    let yearly_rate =
                        state.return_profile_returns[asset.return_profile_index][year_idx];
                    let rate = n_day_rate(yearly_rate, days_passed as f64);
                    let key = (account.account_id, asset.asset_id);
                    if let Some(balance) = state.asset_balances.get_mut(&key) {
                        let balance_before = *balance;
                        let return_amount = balance_before * rate;
                        let new_value = balance_before + return_amount;
                        *balance = new_value;

                        // Record the return transaction (includes negative returns for debt/losses)
                        if return_amount.abs() > 0.001 {
                            state.records.push(Record::investment_return(
                                next_checkpoint,
                                account.account_id,
                                asset.asset_id,
                                balance_before,
                                rate,
                                return_amount,
                            ));
                        }
                    }
                }
            }
        }

        // Record date checkpoint
        state.dates.push(next_checkpoint);
    }

    // Capture year-end balances for RMD calculations (December 31)
    if next_checkpoint == dec_31 {
        let year = next_checkpoint.year();
        let mut year_balances = HashMap::new();

        for (account_id, account) in &state.accounts {
            if matches!(account.account_type, AccountType::TaxDeferred) {
                let balance = state.account_balance(*account_id);
                year_balances.insert(*account_id, balance);
            }
        }

        state.year_end_balances.insert(year, year_balances);
    }

    state.current_date = next_checkpoint;
}

pub fn monte_carlo_simulate(
    params: &SimulationParameters,
    num_iterations: usize,
) -> MonteCarloResult {
    const MAX_BATCH_SIZE: usize = 100;
    let num_batches = num_iterations.div_ceil(MAX_BATCH_SIZE);

    let iterations = (0..num_batches)
        .into_par_iter()
        .flat_map(|i| {
            let mut rng = rand::rngs::SmallRng::seed_from_u64(i as u64);

            let batch_size = if i == num_batches - 1 {
                num_iterations - i * MAX_BATCH_SIZE
            } else {
                MAX_BATCH_SIZE
            };

            (0..batch_size)
                .map(|_| {
                    let seed = rng.next_u64();
                    simulate(params, seed)
                })
                .collect::<Vec<_>>()
        })
        .collect();

    MonteCarloResult { iterations }
}
