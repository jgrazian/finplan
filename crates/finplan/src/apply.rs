//! Apply StateEvents to SimulationState
//!
//! This module takes the evaluated StateEvents from evaluate.rs and applies them
//! to mutate the SimulationState.

use crate::{
    error::ApplyError,
    evaluate::{StateEvent, TriggerEvent, evaluate_effect, evaluate_trigger},
    model::{AccountFlavor, AssetLot, Event, EventId, EventTrigger, Record},
    simulation_state::SimulationState,
};

/// Apply a StateEvent to mutate the SimulationState
pub fn apply_state_event(
    state: &mut SimulationState,
    event: &StateEvent,
) -> Result<(), ApplyError> {
    match event {
        StateEvent::CreateAccount(account) => {
            state.accounts.insert(account.account_id, account.clone());
            Ok(())
        }

        StateEvent::DeleteAccount(account_id) => {
            state.accounts.remove(account_id);
            Ok(())
        }

        StateEvent::CashCredit { to, net_amount } => {
            let account = state
                .accounts
                .get_mut(to)
                .ok_or(ApplyError::AccountNotFound(*to))?;

            match &mut account.flavor {
                AccountFlavor::Bank(cash) => {
                    cash.value += net_amount;
                }
                AccountFlavor::Investment(inv) => {
                    inv.cash.value += net_amount;
                }
                _ => return Err(ApplyError::NotACashAccount(*to)),
            }
            Ok(())
        }

        StateEvent::CashDebit { from, net_amount } => {
            let account = state
                .accounts
                .get_mut(from)
                .ok_or(ApplyError::AccountNotFound(*from))?;

            match &mut account.flavor {
                AccountFlavor::Bank(cash) => {
                    cash.value -= net_amount;
                }
                AccountFlavor::Investment(inv) => {
                    inv.cash.value -= net_amount;
                }
                _ => return Err(ApplyError::NotACashAccount(*from)),
            }
            Ok(())
        }

        StateEvent::IncomeTax {
            gross_income_amount,
            federal_tax,
            state_tax,
        } => {
            state.ytd_tax.ordinary_income += gross_income_amount;
            state.ytd_tax.federal_tax += federal_tax;
            state.ytd_tax.state_tax += state_tax;
            Ok(())
        }

        StateEvent::ShortTermCapitalGainsTax {
            gross_gain_amount,
            federal_tax,
            state_tax,
        } => {
            state.ytd_tax.capital_gains += gross_gain_amount;
            state.ytd_tax.federal_tax += federal_tax;
            state.ytd_tax.state_tax += state_tax;
            Ok(())
        }

        StateEvent::LongTermCapitalGainsTax {
            gross_gain_amount,
            federal_tax,
            state_tax,
        } => {
            state.ytd_tax.capital_gains += gross_gain_amount;
            state.ytd_tax.federal_tax += federal_tax;
            state.ytd_tax.state_tax += state_tax;
            Ok(())
        }

        StateEvent::AddAssetLot {
            to,
            units,
            cost_basis,
        } => {
            let account = state
                .accounts
                .get_mut(&to.account_id)
                .ok_or(ApplyError::AccountNotFound(to.account_id))?;

            if let AccountFlavor::Investment(inv) = &mut account.flavor {
                inv.positions.push(AssetLot {
                    asset_id: to.asset_id,
                    purchase_date: state.current_date,
                    units: *units,
                    cost_basis: *cost_basis,
                });
                Ok(())
            } else {
                Err(ApplyError::NotAnInvestmentAccount(to.account_id))
            }
        }

        StateEvent::SubtractAssetLot {
            from,
            lot_date,
            units,
            cost_basis,
        } => {
            let account = state
                .accounts
                .get_mut(&from.account_id)
                .ok_or(ApplyError::AccountNotFound(from.account_id))?;

            if let AccountFlavor::Investment(inv) = &mut account.flavor {
                // Find and reduce the matching lot
                if let Some(lot) = inv
                    .positions
                    .iter_mut()
                    .find(|l| l.asset_id == from.asset_id && l.purchase_date == *lot_date)
                {
                    lot.units -= units;
                    lot.cost_basis -= cost_basis;

                    // Remove lot if depleted
                    if lot.units <= 0.001 {
                        inv.positions.retain(|l| {
                            !(l.asset_id == from.asset_id && l.purchase_date == *lot_date)
                        });
                    }
                }
                Ok(())
            } else {
                Err(ApplyError::NotAnInvestmentAccount(from.account_id))
            }
        }

        StateEvent::TriggerEvent(event_id) => {
            // Mark event for immediate triggering
            state.pending_triggers.push(*event_id);
            Ok(())
        }

        StateEvent::PauseEvent(event_id) => {
            state.repeating_event_active.insert(*event_id, false);
            Ok(())
        }

        StateEvent::ResumeEvent(event_id) => {
            state.repeating_event_active.insert(*event_id, true);
            Ok(())
        }

        StateEvent::TerminateEvent(event_id) => {
            state.repeating_event_active.remove(event_id);
            state.event_next_date.remove(event_id);
            Ok(())
        }
    }
}

/// Process all pending events for the current date
/// Returns list of event IDs that were triggered
pub fn process_events(state: &mut SimulationState) -> Vec<EventId> {
    let mut triggered = Vec::new();

    // Collect events to evaluate (avoid borrow issues)
    let events_to_check: Vec<(EventId, Event)> = state
        .events
        .iter()
        .filter(|(id, event)| {
            // Skip if already triggered and once=true (but not for Repeating)
            if event.once
                && state.triggered_events.contains_key(id)
                && !matches!(event.trigger, EventTrigger::Repeating { .. })
            {
                return false;
            }
            true
        })
        .map(|(id, e)| (*id, e.clone()))
        .collect();

    // Evaluate each event
    for (event_id, event) in events_to_check {
        let trigger_result = match evaluate_trigger(&event_id, &event.trigger, state) {
            Ok(result) => result,
            Err(_) => continue, // Skip events that fail to evaluate
        };

        let should_trigger = match trigger_result {
            TriggerEvent::Triggered => true,
            TriggerEvent::StartRepeating(next_date) => {
                // Activate the repeating event
                state.repeating_event_active.insert(event_id, true);
                state.event_next_date.insert(event_id, next_date);
                true // Trigger immediately on activation
            }
            TriggerEvent::TriggerRepeating(next_date) => {
                // Schedule next occurrence
                state.event_next_date.insert(event_id, next_date);
                true
            }
            TriggerEvent::StopRepeating => {
                // Terminate the repeating event
                state.repeating_event_active.remove(&event_id);
                state.event_next_date.remove(&event_id);
                false
            }
            TriggerEvent::NotTriggered | TriggerEvent::NextTriggerDate(_) => false,
        };

        if should_trigger {
            // Record trigger for once checks and RelativeToEvent
            state.triggered_events.insert(event_id, state.current_date);

            // Record to linear event history
            state
                .records
                .push(Record::event(state.current_date, event_id));

            triggered.push(event_id);

            // Evaluate and apply effects
            for effect in &event.effects {
                match evaluate_effect(effect, state) {
                    Ok(state_events) => {
                        for se in state_events {
                            if let Err(e) = apply_state_event(state, &se) {
                                // Log error but continue processing
                                eprintln!("Error applying state event: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error evaluating effect: {:?}", e);
                    }
                }
            }
        }
    }

    // Process chained event triggers (with recursion protection)
    let mut depth = 0;
    while !state.pending_triggers.is_empty() && depth < 10 {
        depth += 1;
        let triggers = std::mem::take(&mut state.pending_triggers);

        for event_id in triggers {
            if let Some(event) = state.events.get(&event_id).cloned() {
                // Skip if already triggered and once=true
                if event.once && state.triggered_events.contains_key(&event_id) {
                    continue;
                }

                state.triggered_events.insert(event_id, state.current_date);
                state
                    .records
                    .push(Record::event(state.current_date, event_id));
                triggered.push(event_id);

                for effect in &event.effects {
                    if let Ok(state_events) = evaluate_effect(effect, state) {
                        for se in state_events {
                            let _ = apply_state_event(state, &se);
                        }
                    }
                }
            }
        }
    }

    triggered
}
