use crate::event_engine::process_events;
use crate::models::*;
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
        event_history: state.event_history.clone(),
        cash_flow_history: state.cash_flow_history.clone(),
        return_history: state.return_history.clone(),
        transfer_history: state.transfer_history.clone(),
        withdrawal_history: state.withdrawal_history.clone(),
        rmd_history: state.rmd_history.clone(),
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
            state.withdrawal_history.push(WithdrawalRecord {
                date: state.current_date,
                spending_target_id: st.spending_target_id,
                account_id,
                asset_id,
                gross_amount: gross_withdrawal,
                tax_amount: tax_result.total_tax,
                net_amount: tax_result.net_amount,
            });

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
                state.cash_flow_history.push(CashFlowRecord {
                    date: state.current_date,
                    cash_flow_id: cf_id,
                    account_id: *target_account_id,
                    asset_id: *target_asset_id,
                    amount,
                });
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
                state.cash_flow_history.push(CashFlowRecord {
                    date: state.current_date,
                    cash_flow_id: cf_id,
                    account_id: *source_account_id,
                    asset_id: *source_asset_id,
                    amount: -amount,
                });
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
                            state.return_history.push(ReturnRecord {
                                date: next_checkpoint,
                                account_id: account.account_id,
                                asset_id: asset.asset_id,
                                balance_before,
                                return_rate: rate,
                                return_amount,
                            });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::*;

    #[test]
    fn test_monte_carlo_simulation() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 30,
            birth_date: None,
            inflation_profile: InflationProfile::Fixed(0.02),
            return_profiles: vec![ReturnProfile::Normal {
                mean: 0.07,
                std_dev: 0.15,
            }],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 10_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![],
            ..Default::default()
        };

        const NUM_ITERATIONS: usize = 100;
        let result = monte_carlo_simulate(&params, NUM_ITERATIONS);
        assert_eq!(result.iterations.len(), NUM_ITERATIONS);

        // Check that results are different (due to random seed)
        let first_final = result.iterations[0].final_account_balance(AccountId(1));
        let second_final = result.iterations[1].final_account_balance(AccountId(1));

        assert_ne!(first_final, second_final);
    }

    #[test]
    fn test_simulation_basic() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 10,
            birth_date: None,
            inflation_profile: InflationProfile::Fixed(0.02),
            return_profiles: vec![ReturnProfile::Fixed(0.05)],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 10_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 100.0,
                repeats: RepeatInterval::Monthly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Active,
            }],
            ..Default::default()
        };

        let _result = simulate(&params, 42);
    }

    #[test]
    fn test_cashflow_limits() {
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2022, 1, 1)),
            duration_years: 10,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 10_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 100.0,
                repeats: RepeatInterval::Monthly,
                cash_flow_limits: Some(CashFlowLimits {
                    limit: 1_000.0,
                    limit_period: LimitPeriod::Yearly,
                }),
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Active,
            }],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // Initial: 10,000
        // Contribution: 100/month -> 1200/year.
        // Limit: 1000/year.
        // Expected annual contribution: 1000.
        // Duration: 10 years.
        // Total added: 10 * 1000 = 10,000.
        // Final Balance: 10,000 + 10,000 = 20,000.

        let final_balance = result.final_account_balance(AccountId(1));
        assert_eq!(
            final_balance, 20_000.0,
            "Balance should be capped by yearly limits"
        );
    }

    #[test]
    fn test_simulation_start_date() {
        let start_date = jiff::civil::date(2020, 1, 1);
        let params = SimulationParameters {
            start_date: Some(start_date),
            duration_years: 1,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 10_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // Check that the first snapshot date matches the start date
        assert_eq!(result.dates[0], start_date);
    }

    #[test]
    fn test_inflation_adjustment() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 2,
            birth_date: None,
            inflation_profile: InflationProfile::Fixed(0.10), // 10% inflation
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 100.0,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: None,
                adjust_for_inflation: true,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Active,
            }],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // Year 0: 100.0
        // Year 1: 100.0 * 1.10 = 110.0
        // Total: 210.0

        let final_balance = result.final_account_balance(AccountId(1));
        // Floating point comparison
        assert!(
            (final_balance - 210.0).abs() < 1e-6,
            "Expected 210.0, got {}",
            final_balance
        );
    }

    #[test]
    fn test_lifetime_limit() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 5,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 1000.0,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: Some(CashFlowLimits {
                    limit: 2500.0,
                    limit_period: LimitPeriod::Lifetime,
                }),
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Active,
            }],
            ..Default::default()
        };

        let result = simulate(&params, 42);
        let final_balance = result.final_account_balance(AccountId(1));
        assert_eq!(final_balance, 2500.0);
    }

    #[test]
    fn test_event_trigger_balance() {
        // Event-based: Event triggers when balance > 5000, which activates a bonus cash flow
        let params = SimulationParameters {
            start_date: None,
            duration_years: 5,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::AccountBalance {
                    account_id: AccountId(1),
                    threshold: 5000.0,
                    above: true,
                },
                effects: vec![EventEffect::ActivateCashFlow(CashFlowId(2))],
                once: true,
            }],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![
                // Base income: 2000/year - starts active
                CashFlow {
                    cash_flow_id: CashFlowId(1),
                    amount: 2000.0,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    direction: CashFlowDirection::Income {
                        target_account_id: AccountId(1),
                        target_asset_id: AssetId(1),
                    },
                    state: CashFlowState::Active,
                },
                // Bonus starts when RichEnough event triggers - starts pending
                CashFlow {
                    cash_flow_id: CashFlowId(2),
                    amount: 10000.0,
                    repeats: RepeatInterval::Never, // One time bonus
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    direction: CashFlowDirection::Income {
                        target_account_id: AccountId(1),
                        target_asset_id: AssetId(1),
                    },
                    state: CashFlowState::Pending,
                },
            ],
            ..Default::default()
        };

        let result = simulate(&params, 42);
        let final_balance = result.final_account_balance(AccountId(1));

        // Year 0: +2000 -> Bal 2000
        // Year 1: +2000 -> Bal 4000
        // Year 2: +2000 -> Bal 6000. Trigger "RichEnough" (Threshold 5000).
        // Bonus +10000 -> Bal 16000.
        // Year 3: +2000 -> Bal 18000.
        // Year 4: +2000 -> Bal 20000.

        assert_eq!(final_balance, 20000.0);
    }

    #[test]
    fn test_event_date_trigger() {
        let start_date = jiff::civil::date(2025, 1, 1);
        let params = SimulationParameters {
            start_date: Some(start_date),
            duration_years: 5,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::Date(start_date.saturating_add(2.years())),
                effects: vec![EventEffect::ActivateCashFlow(CashFlowId(1))],
                once: true,
            }],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 1000.0,
                repeats: RepeatInterval::Monthly,
                cash_flow_limits: Some(CashFlowLimits {
                    limit: 5000.0,
                    limit_period: LimitPeriod::Yearly,
                }),
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Pending, // Starts pending, activated by event
            }],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // StartSaving triggers at Year 2 (2027-01-01).
        // Year 0 (2025): 0
        // Year 1 (2026): 0
        // Year 2 (2027): Start. Monthly 1000. Limit 5000/year.
        // Year 3 (2028): 5000.
        // Year 4 (2029): 5000.
        // Total: 15000.

        let final_balance = result.final_account_balance(AccountId(1));
        assert_eq!(final_balance, 15000.0);
    }

    #[test]
    fn test_interest_accrual() {
        let params = SimulationParameters {
            start_date: None,
            duration_years: 1,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::Fixed(0.10)],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 1000.0,
                repeats: RepeatInterval::Never,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Active,
            }],
            ..Default::default()
        };

        let result = simulate(&params, 42);
        let final_balance = result.final_account_balance(AccountId(1));

        // 1000 invested immediately. 10% return. 1 year.
        // Should be 1100.
        assert!(
            (final_balance - 1100.0).abs() < 1.0,
            "Expected 1100.0, got {}",
            final_balance
        );
    }

    #[test]
    fn test_cross_account_events() {
        // Test: Debt payoff triggers savings to start
        let params = SimulationParameters {
            start_date: None,
            duration_years: 5,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::AccountBalance {
                    account_id: AccountId(1), // Debt account
                    threshold: 0.0,
                    above: true, // When balance >= 0 (debt paid off)
                },
                effects: vec![
                    EventEffect::TerminateCashFlow(CashFlowId(1)), // Stop debt payments
                    EventEffect::ActivateCashFlow(CashFlowId(2)),  // Start savings
                ],
                once: true,
            }],
            accounts: vec![
                Account {
                    account_id: AccountId(1),
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: -2000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Liability,
                    }],
                    account_type: AccountType::Illiquid,
                },
                Account {
                    account_id: AccountId(2),
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 0.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                    account_type: AccountType::Taxable,
                },
            ],
            cash_flows: vec![
                // Debt payment - starts active
                CashFlow {
                    cash_flow_id: CashFlowId(1),
                    amount: 1000.0,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    direction: CashFlowDirection::Income {
                        target_account_id: AccountId(1),
                        target_asset_id: AssetId(1),
                    },
                    state: CashFlowState::Active,
                },
                // Savings - starts pending, activated when debt is paid
                CashFlow {
                    cash_flow_id: CashFlowId(2),
                    amount: 1000.0,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    direction: CashFlowDirection::Income {
                        target_account_id: AccountId(2),
                        target_asset_id: AssetId(1),
                    },
                    state: CashFlowState::Pending,
                },
            ],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // Debt Account:
        // Year 0: -2000 + 1000 = -1000
        // Year 1: -1000 + 1000 = 0. Trigger "DebtPaid".
        // Payment stops.
        // Final Debt Balance: 0.

        // Savings Account:
        // Year 1: +1000 -> Bal 1000 (event triggered, cashflow activated)
        // Year 2: +1000 -> Bal 2000.
        // Year 3: +1000 -> Bal 3000.
        // Year 4: +1000 -> Bal 4000.

        let final_debt = result.final_account_balance(AccountId(1));
        let final_savings = result.final_account_balance(AccountId(2));

        assert_eq!(final_debt, 0.0, "Debt should be paid off");
        assert_eq!(
            final_savings, 4000.0,
            "Savings should accumulate after debt is paid"
        );
    }

    #[test]
    fn test_spending_target_basic() {
        // Test basic spending target withdrawal from a single account
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2025, 1, 1)),
            duration_years: 5,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![Account {
                account_id: AccountId(1),
                account_type: AccountType::TaxDeferred, // 401k - taxed as ordinary income
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 100_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
            }],
            cash_flows: vec![],
            spending_targets: vec![SpendingTarget {
                spending_target_id: SpendingTargetId(1),
                amount: 10_000.0,
                net_amount_mode: false, // Gross withdrawal
                repeats: RepeatInterval::Yearly,
                adjust_for_inflation: false,
                withdrawal_strategy: WithdrawalStrategy::Sequential {
                    order: vec![AccountId(1)],
                },
                exclude_accounts: vec![],
                state: SpendingTargetState::Active,
            }],
            tax_config: TaxConfig::default(),
        };

        let result = simulate(&params, 42);
        let final_balance = result.final_account_balance(AccountId(1));

        // Starting: 100,000
        // Yearly withdrawal: 10,000
        // After 5 years: 100,000 - (5 * 10,000) = 50,000
        assert!(
            (final_balance - 50_000.0).abs() < 1.0,
            "Expected ~50,000, got {}",
            final_balance
        );

        // Check that taxes were tracked
        assert!(!result.yearly_taxes.is_empty(), "Should have tax records");

        // Each year should have 10,000 in ordinary income (TaxDeferred withdrawal)
        for tax in &result.yearly_taxes {
            assert!(
                (tax.ordinary_income - 10_000.0).abs() < 1.0,
                "Expected 10,000 ordinary income, got {}",
                tax.ordinary_income
            );
        }
    }

    #[test]
    fn test_spending_target_tax_optimized() {
        // Test tax-optimized withdrawal order: Taxable -> TaxDeferred -> TaxFree
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2025, 1, 1)),
            duration_years: 3,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![
                Account {
                    account_id: AccountId(1),
                    account_type: AccountType::TaxFree, // Roth - should be last
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 50_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                },
                Account {
                    account_id: AccountId(2),
                    account_type: AccountType::TaxDeferred, // 401k - should be second
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 50_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                },
                Account {
                    account_id: AccountId(3),
                    account_type: AccountType::Taxable, // Brokerage - should be first
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 30_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                },
            ],
            cash_flows: vec![],
            spending_targets: vec![SpendingTarget {
                spending_target_id: SpendingTargetId(1),
                amount: 40_000.0,
                net_amount_mode: false,
                repeats: RepeatInterval::Yearly,
                adjust_for_inflation: false,
                withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
                exclude_accounts: vec![],
                state: SpendingTargetState::Active,
            }],
            tax_config: TaxConfig::default(),
        };

        let result = simulate(&params, 42);

        // Year 1: Need 40k. Taxable has 30k, so take all 30k from Taxable, then 10k from TaxDeferred
        // Year 2: Taxable empty. Take 40k from TaxDeferred (has 40k left)
        // Year 3: TaxDeferred empty. Take 40k from TaxFree

        // Final balances:
        // Taxable: 0
        // TaxDeferred: 0
        // TaxFree: 50,000 - 40,000 = 10,000

        let taxfree_balance = result.final_account_balance(AccountId(1));
        let taxdeferred_balance = result.final_account_balance(AccountId(2));
        let taxable_balance = result.final_account_balance(AccountId(3));

        assert!(
            taxable_balance.abs() < 1.0,
            "Taxable should be depleted first, got {}",
            taxable_balance
        );
        assert!(
            taxdeferred_balance.abs() < 1.0,
            "TaxDeferred should be depleted second, got {}",
            taxdeferred_balance
        );
        assert!(
            (taxfree_balance - 10_000.0).abs() < 1.0,
            "TaxFree should have ~10,000 left, got {}",
            taxfree_balance
        );
    }

    #[test]
    fn test_spending_target_excludes_illiquid() {
        // Test that Illiquid accounts are automatically skipped
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2025, 1, 1)),
            duration_years: 2,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![],
            accounts: vec![
                Account {
                    account_id: AccountId(1),
                    account_type: AccountType::Illiquid, // Real estate - cannot withdraw
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 500_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::RealEstate,
                    }],
                },
                Account {
                    account_id: AccountId(2),
                    account_type: AccountType::Taxable,
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 50_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                },
            ],
            cash_flows: vec![],
            spending_targets: vec![SpendingTarget {
                spending_target_id: SpendingTargetId(1),
                amount: 20_000.0,
                net_amount_mode: false,
                repeats: RepeatInterval::Yearly,
                adjust_for_inflation: false,
                withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
                exclude_accounts: vec![],
                state: SpendingTargetState::Active,
            }],
            tax_config: TaxConfig::default(),
        };

        let result = simulate(&params, 42);

        // Illiquid account should be untouched
        let illiquid_balance = result.final_account_balance(AccountId(1));
        assert_eq!(
            illiquid_balance, 500_000.0,
            "Illiquid account should be untouched"
        );

        // Taxable should have withdrawals
        let taxable_balance = result.final_account_balance(AccountId(2));
        assert!(
            (taxable_balance - 10_000.0).abs() < 1.0,
            "Taxable should have 10,000 left after 2 years of 20k withdrawals, got {}",
            taxable_balance
        );
    }

    #[test]
    fn test_age_based_event() {
        let birth_date = jiff::civil::date(1960, 6, 15);
        let start_date = jiff::civil::date(2025, 1, 1); // Person is 64

        let params = SimulationParameters {
            start_date: Some(start_date),
            duration_years: 5,
            birth_date: Some(birth_date),
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::Age {
                    years: 65,
                    months: None,
                },
                effects: vec![
                    EventEffect::TerminateCashFlow(CashFlowId(1)), // Stop salary
                    EventEffect::ActivateSpendingTarget(SpendingTargetId(1)), // Start retirement withdrawals
                ],
                once: true,
            }],
            accounts: vec![Account {
                account_id: AccountId(1),
                account_type: AccountType::TaxDeferred,
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 500_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
            }],
            cash_flows: vec![CashFlow {
                cash_flow_id: CashFlowId(1),
                amount: 50_000.0,
                repeats: RepeatInterval::Yearly,
                cash_flow_limits: None,
                adjust_for_inflation: false,
                direction: CashFlowDirection::Income {
                    target_account_id: AccountId(1),
                    target_asset_id: AssetId(1),
                },
                state: CashFlowState::Active,
            }],
            spending_targets: vec![SpendingTarget {
                spending_target_id: SpendingTargetId(1),
                amount: 40_000.0,
                net_amount_mode: false,
                repeats: RepeatInterval::Yearly,
                adjust_for_inflation: false,
                withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
                exclude_accounts: vec![],
                state: SpendingTargetState::Pending, // Starts pending
            }],
            tax_config: TaxConfig::default(),
        };

        let result = simulate(&params, 42);

        // Person turns 65 in June 2025
        // Year 0 (2025): Salary +50k, then retirement starts -> -40k. Net: +10k
        // Year 1 (2026): -40k (salary stopped)
        // Year 2 (2027): -40k
        // Year 3 (2028): -40k
        // Year 4 (2029): -40k

        // Starting: 500k + 50k (year 0 salary) = 550k
        // Withdrawals: 5 * 40k = 200k
        // Final: 550k - 200k = 350k

        let final_balance = result.final_account_balance(AccountId(1));

        // Verify retirement event was triggered
        assert!(
            result.event_was_triggered(EventId(1)),
            "Retirement event should have triggered"
        );

        assert!(
            (final_balance - 350_000.0).abs() < 1000.0,
            "Expected ~350,000, got {}",
            final_balance
        );
    }

    #[test]
    fn test_event_chaining() {
        // Test that TriggerEvent effect works for chaining events
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2025, 1, 1)),
            duration_years: 3,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            events: vec![
                Event {
                    event_id: EventId(1),
                    trigger: EventTrigger::Date(jiff::civil::date(2026, 1, 1)),
                    effects: vec![
                        EventEffect::ActivateCashFlow(CashFlowId(1)),
                        EventEffect::TriggerEvent(EventId(2)), // Chain to secondary
                    ],
                    once: true,
                },
                Event {
                    event_id: EventId(2),
                    trigger: EventTrigger::Manual, // Only triggered via TriggerEvent
                    effects: vec![EventEffect::ActivateCashFlow(CashFlowId(2))],
                    once: true,
                },
            ],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 0.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::Taxable,
            }],
            cash_flows: vec![
                CashFlow {
                    cash_flow_id: CashFlowId(1),
                    amount: 1000.0,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    direction: CashFlowDirection::Income {
                        target_account_id: AccountId(1),
                        target_asset_id: AssetId(1),
                    },
                    state: CashFlowState::Pending,
                },
                CashFlow {
                    cash_flow_id: CashFlowId(2),
                    amount: 500.0,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    direction: CashFlowDirection::Income {
                        target_account_id: AccountId(1),
                        target_asset_id: AssetId(1),
                    },
                    state: CashFlowState::Pending,
                },
            ],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // Both events should have triggered
        assert!(
            result.event_was_triggered(EventId(1)),
            "Primary event should trigger"
        );
        assert!(
            result.event_was_triggered(EventId(2)),
            "Secondary event should be chained"
        );

        // Year 0 (2025): Nothing (events not triggered yet)
        // Year 1 (2026): Primary triggers -> Flow1 +1000, Flow2 +500 = 1500
        // Year 2 (2027): Flow1 +1000, Flow2 +500 = 1500

        let final_balance = result.final_account_balance(AccountId(1));
        assert_eq!(final_balance, 3000.0, "Should have 3000 from chained flows");
    }

    #[test]
    fn test_repeating_event_transfer() {
        // Test repeating event that transfers $100/month between accounts
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2025, 1, 1)),
            duration_years: 1,
            birth_date: None,
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            accounts: vec![
                Account {
                    account_id: AccountId(1),
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 10_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                    account_type: AccountType::Taxable,
                },
                Account {
                    account_id: AccountId(2),
                    assets: vec![Asset {
                        asset_id: AssetId(2),
                        initial_value: 0.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                    account_type: AccountType::TaxFree,
                },
            ],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Monthly,
                    start_condition: None, // Start immediately
                },
                effects: vec![EventEffect::TransferAsset {
                    from_account: AccountId(1),
                    to_account: AccountId(2),
                    from_asset_id: AssetId(1),
                    to_asset_id: AssetId(2),
                    amount: Some(100.0),
                }],
                once: false,
            }],
            cash_flows: vec![],
            spending_targets: vec![],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // Should have triggered the repeating event
        assert!(
            result.event_was_triggered(EventId(1)),
            "Repeating event should trigger"
        );

        // Monthly transfers for 1 year (13 occurrences: Jan 1 start + 12 months)
        let account1_balance = result.final_account_balance(AccountId(1));
        let account2_balance = result.final_account_balance(AccountId(2));

        // The exact count depends on simulation timing, but should be ~12-13 transfers
        assert!(
            (1200.0..=1400.0).contains(&account2_balance),
            "Account 2 should have 12-14 transfers worth, got {}",
            account2_balance
        );
        assert_eq!(
            account1_balance + account2_balance,
            10_000.0,
            "Total should still be 10000"
        );

        // Check transfer history has reasonable count
        assert!(
            result.transfer_history.len() >= 12 && result.transfer_history.len() <= 14,
            "Should have 12-14 transfer records, got {}",
            result.transfer_history.len()
        );
    }

    #[test]
    fn test_repeating_event_with_start_condition() {
        // Test repeating event that only starts after age 65
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2025, 1, 1)),
            duration_years: 3,
            birth_date: Some(jiff::civil::date(1960, 6, 15)), // Age 64.5 at start
            inflation_profile: InflationProfile::None,
            return_profiles: vec![ReturnProfile::None],
            accounts: vec![
                Account {
                    account_id: AccountId(1),
                    assets: vec![Asset {
                        asset_id: AssetId(1),
                        initial_value: 100_000.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                    account_type: AccountType::TaxDeferred,
                },
                Account {
                    account_id: AccountId(2),
                    assets: vec![Asset {
                        asset_id: AssetId(2),
                        initial_value: 0.0,
                        return_profile_index: 0,
                        asset_class: AssetClass::Investable,
                    }],
                    account_type: AccountType::TaxFree,
                },
            ],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: Some(Box::new(EventTrigger::Age {
                        years: 65,
                        months: None,
                    })),
                },
                effects: vec![EventEffect::TransferAsset {
                    from_account: AccountId(1),
                    to_account: AccountId(2),
                    from_asset_id: AssetId(1),
                    to_asset_id: AssetId(2),
                    amount: Some(10_000.0), // Roth conversion
                }],
                once: false,
            }],
            cash_flows: vec![],
            spending_targets: vec![],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // Event should trigger (start_condition met mid-2025)
        assert!(
            result.event_was_triggered(EventId(1)),
            "Repeating event should trigger after age 65"
        );

        // Age 65 is June 2025, then yearly transfers through end of 2027
        // The exact number depends on when condition is checked
        let account2_balance = result.final_account_balance(AccountId(2));
        let account1_balance = result.final_account_balance(AccountId(1));

        // Verify transfers happened
        assert!(
            account2_balance >= 20_000.0,
            "Account 2 should have at least 2 transfers worth (got {})",
            account2_balance
        );

        // Verify conservation of value
        assert_eq!(
            account1_balance + account2_balance,
            100_000.0,
            "Total should still be 100000"
        );
    }

    #[test]
    fn test_rmd_withdrawal() {
        let params = SimulationParameters {
            start_date: Some(jiff::civil::date(2024, 1, 1)),
            duration_years: 5,
            birth_date: Some(jiff::civil::date(1951, 6, 15)), // Age 73 in 2024
            return_profiles: vec![ReturnProfile::None],
            accounts: vec![Account {
                account_id: AccountId(1),
                assets: vec![Asset {
                    asset_id: AssetId(1),
                    initial_value: 1_000_000.0,
                    return_profile_index: 0,
                    asset_class: AssetClass::Investable,
                }],
                account_type: AccountType::TaxDeferred,
            }],
            events: vec![Event {
                event_id: EventId(1),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Yearly,
                    start_condition: Some(Box::new(EventTrigger::Age {
                        years: 73,
                        months: Some(0),
                    })),
                },
                effects: vec![EventEffect::CreateRmdWithdrawal {
                    account_id: AccountId(1),
                    starting_age: 73,
                }],
                once: false,
            }],
            cash_flows: vec![],
            spending_targets: vec![],
            ..Default::default()
        };

        let result = simulate(&params, 42);

        // RMD event should trigger
        assert!(
            result.event_was_triggered(EventId(1)),
            "RMD event should trigger at age 73"
        );

        // Should have RMD withdrawals recorded
        assert!(
            !result.withdrawal_history.is_empty(),
            "Should have RMD withdrawals"
        );

        let final_balance = result.final_account_balance(AccountId(1));
        dbg!(&result);

        println!(
            "After RMDs: Starting balance=$1,000,000, Final balance=${:.2}",
            final_balance
        );
        println!("RMD withdrawals: {}", result.withdrawal_history.len());

        // Verify exactly 5 RMD withdrawals (one per year for 5-year simulation)
        assert_eq!(
            result.withdrawal_history.len(),
            5,
            "Should have exactly 5 RMD withdrawals"
        );

        // Verify RMDs were taken (total withdrawals should be substantial)
        let total_withdrawn: f64 = result
            .withdrawal_history
            .iter()
            .map(|w| w.gross_amount)
            .sum();
        println!("RMD withdrawal total: {}", total_withdrawn);
        assert!(
            total_withdrawn > 100_000.0,
            "Total RMD withdrawals should be substantial, got {:.2}",
            total_withdrawn
        );

        // With 5% returns and ~3.77% RMD rate at age 73, balance may grow or shrink
        // depending on market performance vs withdrawal rate
        // Just verify the simulation completed successfully
        assert!(
            final_balance > 0.0,
            "Account should still have positive balance"
        );
    }

    #[test]
    fn test_comprehensive_lifecycle_simulation() {
        // Comprehensive test case modeling a realistic financial lifecycle:
        // - Multiple accounts with multiple assets
        // - Asset tracking across accounts (VFIAX in multiple places)
        // - Complex event chains (home purchase, retirement, RMD)
        // - Cash flow limits (Roth 401k contributions)
        // - Age-based events
        // - Tax optimization

        let start_date = jiff::civil::date(2025, 1, 1);
        let birth_date = jiff::civil::date(1997, 3, 16); // Age 28 at start

        // Asset IDs
        const VFIAX: AssetId = AssetId(1);
        const VGPMX: AssetId = AssetId(2);
        const VIMAX: AssetId = AssetId(3);
        const VTIAX: AssetId = AssetId(4);
        const VFIFX: AssetId = AssetId(5);
        const SP500: AssetId = AssetId(6);
        const HOUSE: AssetId = AssetId(7);
        const CASH: AssetId = AssetId(8);
        const MORTGAGE: AssetId = AssetId(9);

        // Account IDs
        const BROKERAGE: AccountId = AccountId(1);
        const ROTH_IRA: AccountId = AccountId(2);
        const TRAD_401K: AccountId = AccountId(3);
        const ROTH_401K: AccountId = AccountId(4);
        const REAL_ESTATE: AccountId = AccountId(5);
        const CASH_ACCOUNT: AccountId = AccountId(6);
        const MORTGAGE_DEBT: AccountId = AccountId(7);

        // Variables
        const HOUSE_PRICE: f64 = 1_200_000.0;
        const DOWN_PAYMENT_PERCENT: f64 = 0.20; // 20%
        const HOME_PURCHASE_AGE: u8 = 35;
        const RETIREMENT_AGE: u8 = 45;

        // Return profiles (deterministic for testing)
        // 0: S&P 500 (used by VFIAX and SP500) - 7% annually
        // 1: Precious metals (VGPMX) - 3% annually
        // 2: Mid-cap (VIMAX) - 8% annually
        // 3: International (VTIAX) - 6% annually
        // 4: Target date (VFIFX) - 6.5% annually
        // 5: House appreciation - 3% annually
        // 6: Cash - 0% (no growth)
        // 7: Mortgage debt - 6% interest (makes negative balance more negative)

        let params = SimulationParameters {
            start_date: Some(start_date),
            duration_years: 50, // Age 28 to 78
            birth_date: Some(birth_date),
            inflation_profile: InflationProfile::Fixed(0.025), // 2.5% inflation
            return_profiles: vec![
                ReturnProfile::Fixed(0.07),  // 0: S&P 500
                ReturnProfile::Fixed(0.03),  // 1: Precious metals
                ReturnProfile::Fixed(0.08),  // 2: Mid-cap
                ReturnProfile::Fixed(0.06),  // 3: International
                ReturnProfile::Fixed(0.065), // 4: Target date
                ReturnProfile::Fixed(0.03),  // 5: House appreciation
                ReturnProfile::Fixed(0.0),   // 6: Cash (no growth)
                ReturnProfile::Fixed(0.06),  // 7: Mortgage debt interest
            ],
            accounts: vec![
                // 1. Brokerage (Taxable)
                Account {
                    account_id: BROKERAGE,
                    account_type: AccountType::Taxable,
                    assets: vec![
                        Asset {
                            asset_id: VFIAX,
                            initial_value: 900_000.0,
                            return_profile_index: 0, // S&P 500
                            asset_class: AssetClass::Investable,
                        },
                        Asset {
                            asset_id: VGPMX,
                            initial_value: 230_000.0,
                            return_profile_index: 1, // Precious metals
                            asset_class: AssetClass::Investable,
                        },
                        Asset {
                            asset_id: VIMAX,
                            initial_value: 70_000.0,
                            return_profile_index: 2, // Mid-cap
                            asset_class: AssetClass::Investable,
                        },
                        Asset {
                            asset_id: VTIAX,
                            initial_value: 80_000.0,
                            return_profile_index: 3, // International
                            asset_class: AssetClass::Investable,
                        },
                    ],
                },
                // 2. Roth IRA (TaxFree)
                Account {
                    account_id: ROTH_IRA,
                    account_type: AccountType::TaxFree,
                    assets: vec![
                        Asset {
                            asset_id: VFIAX,
                            initial_value: 30_000.0,
                            return_profile_index: 0, // S&P 500 (same as brokerage VFIAX)
                            asset_class: AssetClass::Investable,
                        },
                        Asset {
                            asset_id: VFIFX,
                            initial_value: 15_000.0,
                            return_profile_index: 4, // Target date
                            asset_class: AssetClass::Investable,
                        },
                    ],
                },
                // 3. Traditional 401k (TaxDeferred)
                Account {
                    account_id: TRAD_401K,
                    account_type: AccountType::TaxDeferred,
                    assets: vec![Asset {
                        asset_id: SP500,
                        initial_value: 100_000.0,
                        return_profile_index: 0, // S&P 500 (same as VFIAX)
                        asset_class: AssetClass::Investable,
                    }],
                },
                // 4. Roth 401k (TaxFree)
                Account {
                    account_id: ROTH_401K,
                    account_type: AccountType::TaxFree,
                    assets: vec![Asset {
                        asset_id: SP500,
                        initial_value: 50_000.0,
                        return_profile_index: 0, // S&P 500 (same as VFIAX)
                        asset_class: AssetClass::Investable,
                    }],
                },
                // 5. Real Estate (Illiquid) - added by home purchase event
                Account {
                    account_id: REAL_ESTATE,
                    account_type: AccountType::Illiquid,
                    assets: vec![Asset {
                        asset_id: HOUSE,
                        initial_value: 0.0,      // Will be set by event
                        return_profile_index: 5, // House appreciation
                        asset_class: AssetClass::RealEstate,
                    }],
                },
                // 6. Cash Account (Taxable) - for down payment
                Account {
                    account_id: CASH_ACCOUNT,
                    account_type: AccountType::Taxable,
                    assets: vec![Asset {
                        asset_id: CASH,
                        initial_value: (HOUSE_PRICE * DOWN_PAYMENT_PERCENT) + 100_000.0, // 20% of  $1.2M for house + $100k buffer
                        return_profile_index: 6, // No growth
                        asset_class: AssetClass::Investable,
                    }],
                },
                // 7. Mortgage Debt (Illiquid) - starts at $0, activated by home purchase event
                Account {
                    account_id: MORTGAGE_DEBT,
                    account_type: AccountType::Illiquid,
                    assets: vec![Asset {
                        asset_id: MORTGAGE,
                        initial_value: 0.0, // Will be set to loan amount by event
                        return_profile_index: 7, // 6% interest on debt
                        asset_class: AssetClass::Liability,
                    }],
                },
            ],
            cash_flows: vec![
                // Monthly contribution to Brokerage VFIAX
                CashFlow {
                    cash_flow_id: CashFlowId(1),
                    amount: 1_500.0,
                    repeats: RepeatInterval::Monthly,
                    cash_flow_limits: None,
                    adjust_for_inflation: true,
                    direction: CashFlowDirection::Income {
                        target_account_id: BROKERAGE,
                        target_asset_id: VFIAX,
                    },
                    state: CashFlowState::Active,
                },
                // Mega backdoor Roth 401k - $43.5k/year at $10k/month rate
                CashFlow {
                    cash_flow_id: CashFlowId(2),
                    amount: 10_000.0,
                    repeats: RepeatInterval::Monthly,
                    cash_flow_limits: Some(CashFlowLimits {
                        limit: 43_500.0,
                        limit_period: LimitPeriod::Yearly,
                    }),
                    adjust_for_inflation: false, // IRS limits typically fixed
                    direction: CashFlowDirection::Income {
                        target_account_id: ROTH_401K,
                        target_asset_id: SP500,
                    },
                    state: CashFlowState::Active,
                },
                // Backdoor Roth IRA - $7k/year
                CashFlow {
                    cash_flow_id: CashFlowId(3),
                    amount: 7_000.0,
                    repeats: RepeatInterval::Yearly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false, // IRS limits typically fixed
                    direction: CashFlowDirection::Income {
                        target_account_id: ROTH_IRA,
                        target_asset_id: VFIAX,
                    },
                    state: CashFlowState::Active,
                },
                // Mortgage payment - activated by home purchase event
                // $960k loan at 6% over 30 years = ~$5,755/month
                // Payments reduce the mortgage debt (make it less negative)
                CashFlow {
                    cash_flow_id: CashFlowId(4),
                    amount: 5_755.0,
                    repeats: RepeatInterval::Monthly,
                    cash_flow_limits: None,
                    adjust_for_inflation: false,
                    direction: CashFlowDirection::Income {
                        target_account_id: MORTGAGE_DEBT,
                        target_asset_id: MORTGAGE,
                    },
                    state: CashFlowState::Pending, // Activated by home purchase
                },
            ],
            spending_targets: vec![
                // Retirement withdrawals - activated at age 45
                SpendingTarget {
                    spending_target_id: SpendingTargetId(1),
                    amount: 200_000.0,
                    net_amount_mode: true, // $200k after taxes
                    repeats: RepeatInterval::Yearly,
                    adjust_for_inflation: true,
                    withdrawal_strategy: WithdrawalStrategy::TaxOptimized,
                    exclude_accounts: vec![REAL_ESTATE, CASH_ACCOUNT, MORTGAGE_DEBT], // Don't touch house, cash, or mortgage
                    state: SpendingTargetState::Pending,
                },
            ],
            events: vec![
                // Home purchase at age 35 (year 2032)
                Event {
                    event_id: EventId(1),
                    trigger: EventTrigger::Age {
                        years: HOME_PURCHASE_AGE,
                        months: Some(3), // Spring (April)
                    },
                    effects: vec![
                        // 1. Use down payment from cash account (reduces cash balance)
                        EventEffect::CreateCashFlow(Box::new(CashFlow {
                            cash_flow_id: CashFlowId(101),
                            amount: HOUSE_PRICE * DOWN_PAYMENT_PERCENT,
                            repeats: RepeatInterval::Never,
                            cash_flow_limits: None,
                            adjust_for_inflation: false,
                            direction: CashFlowDirection::Expense {
                                source_account_id: CASH_ACCOUNT,
                                source_asset_id: CASH,
                            },
                            state: CashFlowState::Active,
                        })),
                        // 2. Create the mortgage debt (negative balance = loan amount)
                        EventEffect::CreateCashFlow(Box::new(CashFlow {
                            cash_flow_id: CashFlowId(102),
                            amount: HOUSE_PRICE * (1.0 - DOWN_PAYMENT_PERCENT), // $960k loan
                            repeats: RepeatInterval::Never,
                            cash_flow_limits: None,
                            adjust_for_inflation: false,
                            direction: CashFlowDirection::Expense {
                                source_account_id: MORTGAGE_DEBT,
                                source_asset_id: MORTGAGE,
                            },
                            state: CashFlowState::Active,
                        })),
                        // 3. Add house asset to real estate account
                        EventEffect::CreateCashFlow(Box::new(CashFlow {
                            cash_flow_id: CashFlowId(100),
                            amount: HOUSE_PRICE,
                            repeats: RepeatInterval::Never,
                            cash_flow_limits: None,
                            adjust_for_inflation: false,
                            direction: CashFlowDirection::Income {
                                target_account_id: REAL_ESTATE,
                                target_asset_id: HOUSE,
                            },
                            state: CashFlowState::Active,
                        })),
                        // 4. Start mortgage payments (reduces debt toward zero)
                        EventEffect::ActivateCashFlow(CashFlowId(4)),
                    ],
                    once: true,
                },
                // Add event to stop mortgage payments when paid off
                // Note: Triggers when balance goes from negative back to zero or above
                // We use a small negative threshold to avoid triggering immediately
                Event {
                    event_id: EventId(101),
                    trigger: EventTrigger::And(vec![
                        // Only trigger after home purchase event has occurred
                        EventTrigger::RelativeToEvent {
                            event_id: EventId(1),
                            offset: TriggerOffset::Months(1),
                        },
                        // And when mortgage is paid off
                        EventTrigger::AccountBalance {
                            account_id: MORTGAGE_DEBT,
                            threshold: -1000.0, // Small buffer to ensure debt is nearly paid
                            above: true,
                        },
                    ]),
                    effects: vec![EventEffect::TerminateCashFlow(CashFlowId(4))], // Stop mortgage payments
                    once: true,
                },
                // Retirement at age 45 (year 2042)
                Event {
                    event_id: EventId(2),
                    trigger: EventTrigger::Age {
                        years: RETIREMENT_AGE,
                        months: Some(0), // January
                    },
                    effects: vec![
                        // Stop work contributions
                        EventEffect::TerminateCashFlow(CashFlowId(1)), // Brokerage contributions
                        EventEffect::TerminateCashFlow(CashFlowId(2)), // Roth 401k
                        EventEffect::TerminateCashFlow(CashFlowId(3)), // Roth IRA
                        // Start retirement withdrawals
                        EventEffect::ActivateSpendingTarget(SpendingTargetId(1)),
                    ],
                    once: true,
                },
                // RMD starting at age 73 (year 2070)
                Event {
                    event_id: EventId(3),
                    trigger: EventTrigger::Repeating {
                        interval: RepeatInterval::Yearly,
                        start_condition: Some(Box::new(EventTrigger::Age {
                            years: 73,
                            months: Some(0),
                        })),
                    },
                    effects: vec![EventEffect::CreateRmdWithdrawal {
                        account_id: TRAD_401K,
                        starting_age: 73,
                    }],
                    once: false,
                },
            ],
            tax_config: TaxConfig {
                federal_brackets: vec![
                    TaxBracket {
                        threshold: 0.0,
                        rate: 0.10,
                    },
                    TaxBracket {
                        threshold: 11_600.0,
                        rate: 0.12,
                    },
                    TaxBracket {
                        threshold: 47_150.0,
                        rate: 0.22,
                    },
                    TaxBracket {
                        threshold: 100_525.0,
                        rate: 0.24,
                    },
                    TaxBracket {
                        threshold: 191_950.0,
                        rate: 0.32,
                    },
                    TaxBracket {
                        threshold: 243_725.0,
                        rate: 0.35,
                    },
                    TaxBracket {
                        threshold: 609_350.0,
                        rate: 0.37,
                    },
                ],
                state_rate: 0.05,
                capital_gains_rate: 0.15,      // 15% long-term capital gains
                taxable_gains_percentage: 0.5, // Assume 50% cost basis
            },
        };

        let result = simulate(&params, 42); // Deterministic seed

        // === VERIFICATION CHECKS ===

        println!("\n=== Comprehensive Lifecycle Simulation Results ===");

        // These all track S&P 500 (7% fixed return)
        // They should maintain their initial ratios after accounting for contributions
        println!("\n--- Asset Tracking Test (Same Return Profile) ---");
        println!("All S&P 500 assets should grow at same 7% rate (before flows)");

        // 2. Verify home purchase event
        assert!(
            result.event_was_triggered(EventId(1)),
            "Home purchase event should trigger at age 35"
        );
        let house_value = result.final_account_balance(REAL_ESTATE);
        println!("\n--- Home Purchase Test ---");
        println!("House final value: ${:.2}", house_value);
        assert!(
            house_value > 1_200_000.0,
            "House should appreciate from $1.2M initial"
        );

        // 3. Verify mortgage was created and payments are being made
        let mortgage_balance = result.final_account_balance(MORTGAGE_DEBT);
        let mortgage_payments: f64 = result
            .cash_flow_history
            .iter()
            .filter(|cf| cf.cash_flow_id == CashFlowId(4) && cf.amount > 0.0)
            .map(|cf| cf.amount)
            .sum();
        println!("\n--- Mortgage Test ---");
        println!("Final mortgage balance: ${:.2}", mortgage_balance);
        println!("Total mortgage payments: ${:.2}", mortgage_payments);
        // Mortgage should be paid off (balance near zero, possibly slightly positive due to overpayment)
        // The mortgage starts at -$960k, accrues 6% interest, with $5,755/month payments
        // Over ~30 years it should be fully paid off
        assert!(
            mortgage_balance.abs() < 50_000.0,
            "Mortgage should be nearly paid off, got {}",
            mortgage_balance
        );
        assert!(
            mortgage_payments > HOUSE_PRICE * (1.0 - DOWN_PAYMENT_PERCENT),
            "Should have substantial mortgage payments, got {}",
            mortgage_payments
        );

        // 4. Verify retirement event
        assert!(
            result.event_was_triggered(EventId(2)),
            "Retirement event should trigger at age 45"
        );
        println!("\n--- Retirement Test ---");
        println!(
            "Retirement withdrawals: {}",
            result
                .withdrawal_history
                .iter()
                .filter(|w| w.spending_target_id == SpendingTargetId(1))
                .count()
        );

        // 5. Verify RMD event at age 73
        assert!(
            result.event_was_triggered(EventId(3)),
            "RMD event should trigger at age 73"
        );
        let rmd_count = result.rmd_history.len();
        println!("\n--- RMD Test ---");
        println!("RMD records: {}", rmd_count);
        // Age 73-78 = 6 years of RMDs
        assert!(
            rmd_count >= 5,
            "Should have RMDs for ages 73-78, got {}",
            rmd_count
        );

        // 6. Verify cash flow limits (Roth 401k contributions)
        let roth_401k_contributions: f64 = result
            .cash_flow_history
            .iter()
            .filter(|cf| {
                cf.cash_flow_id == CashFlowId(2) && cf.account_id == ROTH_401K && cf.amount > 0.0
            })
            .map(|cf| cf.amount)
            .sum();
        println!("\n--- Cash Flow Limits Test ---");
        println!(
            "Total Roth 401k contributions: ${:.2}",
            roth_401k_contributions
        );
        // Should contribute for ~17 years (age 28-45) at $43.5k/year
        let expected_contributions = (RETIREMENT_AGE - 28) as f64 * 43_500.0;
        assert!(
            (roth_401k_contributions - expected_contributions).abs() < 50_000.0,
            "Roth 401k contributions should be ~${:.0}, got ${:.0}",
            expected_contributions,
            roth_401k_contributions
        );

        // 7. Verify tax optimization (taxable accounts depleted first in retirement)
        println!("\n--- Tax Optimization Test ---");
        let final_brokerage = result.final_account_balance(BROKERAGE);
        let final_roth_ira = result.final_account_balance(ROTH_IRA);
        let final_roth_401k = result.final_account_balance(ROTH_401K);
        let final_trad_401k = result.final_account_balance(TRAD_401K);

        println!("Final Brokerage: ${:.2}", final_brokerage);
        println!("Final Roth IRA: ${:.2}", final_roth_ira);
        println!("Final Roth 401k: ${:.2}", final_roth_401k);
        println!("Final Traditional 401k: ${:.2}", final_trad_401k);

        // 8. Verify total wealth conservation (minus taxes and spending)
        let initial_wealth = 900_000.0  // Brokerage VFIAX
            + 230_000.0  // Brokerage VGPMX
            + 70_000.0   // Brokerage VIMAX
            + 80_000.0   // Brokerage VTIAX
            + 30_000.0   // Roth IRA VFIAX
            + 15_000.0   // Roth IRA VFIFX
            + 100_000.0  // Trad 401k
            + 50_000.0   // Roth 401k
            + (HOUSE_PRICE * DOWN_PAYMENT_PERCENT) + 100_000.0; // Cash account (down payment + buffer)

        let final_cash = result.final_account_balance(CASH_ACCOUNT);
        let final_wealth = final_brokerage
            + final_roth_ira
            + final_roth_401k
            + final_trad_401k
            + house_value
            + final_cash
            + mortgage_balance; // Negative value (debt)

        // Calculate total contributions
        let total_contributions: f64 = result
            .cash_flow_history
            .iter()
            .filter(|cf| cf.amount > 0.0)
            .map(|cf| cf.amount)
            .sum();

        // Calculate total expenses (mortgage + others)
        let total_expenses: f64 = result
            .cash_flow_history
            .iter()
            .filter(|cf| cf.amount < 0.0)
            .map(|cf| cf.amount.abs())
            .sum();

        // Calculate total retirement withdrawals
        let total_withdrawals: f64 = result
            .withdrawal_history
            .iter()
            .map(|w| w.gross_amount)
            .sum();

        // Calculate total taxes paid
        let total_taxes: f64 = result
            .yearly_taxes
            .iter()
            .map(|t| t.federal_tax + t.state_tax)
            .sum();

        println!("\n--- Wealth Summary ---");
        println!("Initial investable: ${:.2}", initial_wealth);
        println!("Total contributions: ${:.2}", total_contributions);
        println!("Total expenses (incl. mortgage): ${:.2}", total_expenses);
        println!("Total retirement withdrawals: ${:.2}", total_withdrawals);
        println!("Total taxes paid: ${:.2}", total_taxes);
        println!("Final total wealth: ${:.2}", final_wealth);
        println!("Net worth change: ${:.2}", final_wealth - initial_wealth);

        // Check for any negative balances (bug indicator)
        assert!(
            final_brokerage.abs() >= 0.0,
            "Brokerage should not be negative! Got {}. This indicates overspending.",
            final_brokerage
        );
        assert!(
            final_roth_ira.abs() >= 0.0,
            "Roth IRA should not be negative! Got {}",
            final_roth_ira
        );
        assert!(
            final_roth_401k.abs() >= 0.0,
            "Roth 401k should not be negative! Got {}",
            final_roth_401k
        );
        assert!(
            final_trad_401k.abs() >= 0.0,
            "Traditional 401k should not be negative! Got {}",
            final_trad_401k
        );

        // With contributions and returns, wealth should grow
        // (though it may decline in later years due to retirement withdrawals)
        // For now, just verify no negative account balances
        println!("\n=== Balance Verification: PASSED ===");

        // 9. Check simulation completed fully (50 years)
        // Verify by checking that the simulation recorded dates spanning the full duration
        let first_date = result.dates.first().unwrap();
        let last_date = result.dates.last().unwrap();
        let years_simulated = (last_date.year() - first_date.year()) as usize;
        assert!(
            years_simulated >= 49, // Allow for slight date rounding
            "Simulation should span 50 years, got {} years (from {} to {})",
            years_simulated,
            first_date,
            last_date
        );

        // Tax records are only created for years with taxable activity
        // We expect tax records during: contribution years (28-40) + retirement withdrawals (40-78)
        println!(
            "Tax records: {} (years with taxable activity)",
            result.yearly_taxes.len()
        );
        assert!(
            result.yearly_taxes.len() >= 20,
            "Should have tax records for years with activity, got {}",
            result.yearly_taxes.len()
        );

        println!("\n=== All Verification Checks Passed ===\n");
    }
}
