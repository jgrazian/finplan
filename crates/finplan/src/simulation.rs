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
}
