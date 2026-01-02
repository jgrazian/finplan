use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::event_engine::process_events;
use crate::model::{
    AccountType, EventTrigger, MonteCarloResult, Record, SimulationResult, TriggerOffset,
};
use crate::simulation_state::SimulationState;

use jiff::ToSpan;
use rand::{RngCore, SeedableRng};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

pub fn n_day_rate(yearly_rate: f64, n_days: f64) -> f64 {
    (1.0 + yearly_rate).powf(n_days / 365.0) - 1.0
}

pub fn simulate(params: &SimulationConfig, seed: u64) -> SimulationResult {
    let mut state = SimulationState::from_parameters(params, seed);

    while state.current_date < state.end_date {
        let mut something_happened = true;
        while something_happened {
            something_happened = false;

            // Process events - now handles ALL money movement
            if !process_events(&mut state).is_empty() {
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

fn advance_time(state: &mut SimulationState, params: &SimulationConfig) {
    // Check for year rollover before advancing
    state.maybe_rollover_year();

    // Find next checkpoint
    let mut next_checkpoint = state.end_date;

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

    // Heartbeat - advance at least quarterly
    let heartbeat = state.current_date.saturating_add(3.months());
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

pub fn monte_carlo_simulate(params: &SimulationConfig, num_iterations: usize) -> MonteCarloResult {
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
