use std::collections::HashMap;

use crate::apply::process_events;
use crate::config::SimulationConfig;
use crate::model::{
    AccountFlavor, AccountId, EventTrigger, LedgerEntry, MonteCarloResult, SimulationResult,
    StateEvent, TaxStatus, TriggerOffset,
};
use crate::simulation_state::SimulationState;

use jiff::ToSpan;
use rand::{RngCore, SeedableRng};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

// Re-export for backwards compatibility
pub use crate::model::n_day_rate;

pub fn simulate(params: &SimulationConfig, seed: u64) -> SimulationResult {
    let mut state = SimulationState::from_parameters(params, seed);
    state.snapshot_wealth();

    while state.timeline.current_date < state.timeline.end_date {
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
    state.snapshot_wealth();
    state.finalize_year_taxes();

    SimulationResult {
        wealth_snapshots: state.portfolio.wealth_snapshots.clone(),
        yearly_taxes: state.taxes.yearly_taxes.clone(),
        ledger: state.history.ledger.clone(),
    }
}

fn advance_time(state: &mut SimulationState, _params: &SimulationConfig) {
    // Check for year rollover before advancing
    state.maybe_rollover_year();

    // Find next checkpoint
    let mut next_checkpoint = state.timeline.end_date;

    // Check event dates
    for event in state.event_state.events.values() {
        // Skip if already triggered and once=true (unless Repeating)
        if event.once
            && state
                .event_state
                .triggered_events
                .contains_key(&event.event_id)
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

        // Also check relative events
        if let EventTrigger::RelativeToEvent { event_id, offset } = &event.trigger
            && let Some(trigger_date) = state.event_state.triggered_events.get(event_id)
        {
            let target_date = match offset {
                TriggerOffset::Days(d) => trigger_date.checked_add((*d as i64).days()),
                TriggerOffset::Months(m) => trigger_date.checked_add((*m as i64).months()),
                TriggerOffset::Years(y) => trigger_date.checked_add((*y as i64).years()),
            };
            if let Ok(d) = target_date
                && d > state.timeline.current_date
                && d < next_checkpoint
            {
                next_checkpoint = d;
            }
        }
    }

    // Check repeating event scheduled dates
    for date in state.event_state.event_next_date.values() {
        if *date > state.timeline.current_date && *date < next_checkpoint {
            next_checkpoint = *date;
        }
    }

    // Heartbeat - advance at least quarterly
    let heartbeat = state.timeline.current_date.saturating_add(3.months());
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

        // Collect appreciation events to record (need to collect IDs first due to borrow)
        let account_ids: Vec<AccountId> = state.portfolio.accounts.keys().copied().collect();

        // Compound cash balances for all accounts and record appreciation events
        for account_id in account_ids {
            let account = state.portfolio.accounts.get_mut(&account_id).unwrap();
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
                        let multiplier = (1.0 + loan.interest_rate).powf(days_passed as f64 / 365.0);
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
        let mut year_balances = HashMap::new();

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
