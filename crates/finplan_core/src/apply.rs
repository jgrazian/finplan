//! Apply EvalEvents to SimulationState
//!
//! This module takes the evaluated EvalEvents from evaluate.rs and applies them
//! to mutate the SimulationState, recording state changes to the ledger.

use crate::{
    error::ApplyError,
    evaluate::{EvalEvent, TriggerEvent, evaluate_effect, evaluate_trigger},
    model::{AccountFlavor, AssetLot, Event, EventId, EventTrigger, LedgerEntry, StateEvent},
    simulation_state::SimulationState,
};

/// Apply an EvalEvent to mutate the SimulationState and record to ledger
pub fn apply_eval_event(state: &mut SimulationState, event: &EvalEvent) -> Result<(), ApplyError> {
    apply_eval_event_with_source(state, event, None)
}

/// Apply an EvalEvent to mutate the SimulationState and record to ledger
/// with an optional source event for attribution
pub fn apply_eval_event_with_source(
    state: &mut SimulationState,
    event: &EvalEvent,
    source_event: Option<EventId>,
) -> Result<(), ApplyError> {
    let current_date = state.timeline.current_date;

    match event {
        EvalEvent::StateEvent(event) => {
            // Directly apply a StateEvent (used for replaying ledger)
            record_ledger_entry(state, current_date, source_event, event.clone());
            Ok(())
        }
        EvalEvent::CreateAccount(account) => {
            state
                .portfolio
                .accounts
                .insert(account.account_id, account.clone());

            // Record to ledger
            let ledger_event = StateEvent::CreateAccount(account.clone());
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::DeleteAccount(account_id) => {
            state.portfolio.accounts.remove(account_id);

            // Record to ledger
            let ledger_event = StateEvent::DeleteAccount(*account_id);
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::CashCredit {
            to,
            net_amount,
            kind,
        } => {
            let account = state
                .portfolio
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

            // Record to ledger
            let ledger_event = StateEvent::CashCredit {
                to: *to,
                amount: *net_amount,
                kind: *kind,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::RecordContribution { account_id, amount } => {
            // Record the contribution in the tracking maps
            // This uses the actual record_contribution method which handles limits
            state.record_contribution(*account_id, *amount)?;
            Ok(())
        }

        EvalEvent::CashDebit {
            from,
            net_amount,
            kind,
        } => {
            let account = state
                .portfolio
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

            // Record to ledger
            let ledger_event = StateEvent::CashDebit {
                from: *from,
                amount: *net_amount,
                kind: *kind,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::IncomeTax {
            gross_income_amount,
            federal_tax,
            state_tax,
        } => {
            state.taxes.ytd_tax.ordinary_income += gross_income_amount;
            state.taxes.ytd_tax.federal_tax += federal_tax;
            state.taxes.ytd_tax.state_tax += state_tax;

            // Record to ledger
            let ledger_event = StateEvent::IncomeTax {
                gross_amount: *gross_income_amount,
                federal_tax: *federal_tax,
                state_tax: *state_tax,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::ShortTermCapitalGainsTax {
            gross_gain_amount,
            federal_tax,
            state_tax,
        } => {
            state.taxes.ytd_tax.capital_gains += gross_gain_amount;
            state.taxes.ytd_tax.federal_tax += federal_tax;
            state.taxes.ytd_tax.state_tax += state_tax;

            // Record to ledger
            let ledger_event = StateEvent::ShortTermCapitalGainsTax {
                gross_gain: *gross_gain_amount,
                federal_tax: *federal_tax,
                state_tax: *state_tax,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::LongTermCapitalGainsTax {
            gross_gain_amount,
            federal_tax,
            state_tax,
        } => {
            state.taxes.ytd_tax.capital_gains += gross_gain_amount;
            state.taxes.ytd_tax.federal_tax += federal_tax;
            state.taxes.ytd_tax.state_tax += state_tax;

            // Record to ledger
            let ledger_event = StateEvent::LongTermCapitalGainsTax {
                gross_gain: *gross_gain_amount,
                federal_tax: *federal_tax,
                state_tax: *state_tax,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::EarlyWithdrawalPenalty {
            gross_amount,
            penalty_amount,
            penalty_rate,
        } => {
            state.taxes.ytd_tax.early_withdrawal_penalties += penalty_amount;

            // Record to ledger
            let ledger_event = StateEvent::EarlyWithdrawalPenalty {
                gross_amount: *gross_amount,
                penalty_amount: *penalty_amount,
                penalty_rate: *penalty_rate,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::AddAssetLot {
            to,
            units,
            cost_basis,
        } => {
            let account = state
                .portfolio
                .accounts
                .get_mut(&to.account_id)
                .ok_or(ApplyError::AccountNotFound(to.account_id))?;

            let price_per_unit = if *units > 0.0 {
                cost_basis / units
            } else {
                0.0
            };

            if let AccountFlavor::Investment(inv) = &mut account.flavor {
                inv.positions.push(AssetLot {
                    asset_id: to.asset_id,
                    purchase_date: state.timeline.current_date,
                    units: *units,
                    cost_basis: *cost_basis,
                });

                // Record to ledger
                let ledger_event = StateEvent::AssetPurchase {
                    account_id: to.account_id,
                    asset_id: to.asset_id,
                    units: *units,
                    cost_basis: *cost_basis,
                    price_per_unit,
                };
                record_ledger_entry(state, current_date, source_event, ledger_event);

                Ok(())
            } else {
                Err(ApplyError::NotAnInvestmentAccount(to.account_id))
            }
        }

        EvalEvent::SubtractAssetLot {
            from,
            lot_date,
            units,
            cost_basis,
            proceeds,
            short_term_gain,
            long_term_gain,
        } => {
            let account = state
                .portfolio
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

                // Record to ledger with proceeds and gains from lot calculation
                let ledger_event = StateEvent::AssetSale {
                    account_id: from.account_id,
                    asset_id: from.asset_id,
                    lot_date: *lot_date,
                    units: *units,
                    cost_basis: *cost_basis,
                    proceeds: *proceeds,
                    short_term_gain: *short_term_gain,
                    long_term_gain: *long_term_gain,
                };
                record_ledger_entry(state, current_date, source_event, ledger_event);

                Ok(())
            } else {
                Err(ApplyError::NotAnInvestmentAccount(from.account_id))
            }
        }

        EvalEvent::TriggerEvent(event_id) => {
            // Mark event for immediate triggering
            state.pending_triggers.push(*event_id);
            // Note: EventTriggered is recorded when the event actually fires
            Ok(())
        }

        EvalEvent::PauseEvent(event_id) => {
            state
                .event_state
                .repeating_event_active
                .insert(*event_id, false);

            // Record to ledger
            let ledger_event = StateEvent::EventPaused {
                event_id: *event_id,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::ResumeEvent(event_id) => {
            state
                .event_state
                .repeating_event_active
                .insert(*event_id, true);

            // Record to ledger
            let ledger_event = StateEvent::EventResumed {
                event_id: *event_id,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::TerminateEvent(event_id) => {
            // Mark the event as permanently terminated so it can't start or fire again
            state.event_state.terminated_events.insert(*event_id);
            state.event_state.repeating_event_active.remove(event_id);
            state.event_state.event_next_date.remove(event_id);

            // Record to ledger
            let ledger_event = StateEvent::EventTerminated {
                event_id: *event_id,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::AdjustBalance { account, delta } => {
            let acc = state
                .portfolio
                .accounts
                .get_mut(account)
                .ok_or(ApplyError::AccountNotFound(*account))?;

            match &mut acc.flavor {
                AccountFlavor::Liability(loan) => {
                    let previous = loan.principal;
                    loan.principal += delta;
                    // Ensure principal doesn't go negative (can't have negative debt)
                    if loan.principal < 0.0 {
                        loan.principal = 0.0;
                    }

                    // Record to ledger
                    let ledger_event = StateEvent::BalanceAdjusted {
                        account: *account,
                        previous_balance: previous,
                        new_balance: loan.principal,
                        delta: *delta,
                    };
                    record_ledger_entry(state, current_date, source_event, ledger_event);

                    Ok(())
                }
                AccountFlavor::Bank(cash) => {
                    let previous = cash.value;
                    cash.value += delta;

                    let ledger_event = StateEvent::BalanceAdjusted {
                        account: *account,
                        previous_balance: previous,
                        new_balance: cash.value,
                        delta: *delta,
                    };
                    record_ledger_entry(state, current_date, source_event, ledger_event);

                    Ok(())
                }
                AccountFlavor::Investment(inv) => {
                    let previous = inv.cash.value;
                    inv.cash.value += delta;

                    let ledger_event = StateEvent::BalanceAdjusted {
                        account: *account,
                        previous_balance: previous,
                        new_balance: inv.cash.value,
                        delta: *delta,
                    };
                    record_ledger_entry(state, current_date, source_event, ledger_event);

                    Ok(())
                }
                AccountFlavor::Property(asset) => {
                    let previous = asset.value;
                    asset.value += delta;
                    // Ensure value doesn't go negative
                    if asset.value < 0.0 {
                        asset.value = 0.0;
                    }

                    let ledger_event = StateEvent::BalanceAdjusted {
                        account: *account,
                        previous_balance: previous,
                        new_balance: asset.value,
                        delta: *delta,
                    };
                    record_ledger_entry(state, current_date, source_event, ledger_event);

                    Ok(())
                }
            }
        }
    }
}

/// Helper to record a ledger entry
fn record_ledger_entry(
    state: &mut SimulationState,
    date: jiff::civil::Date,
    source_event: Option<EventId>,
    event: StateEvent,
) {
    let entry = match source_event {
        Some(eid) => LedgerEntry::with_source(date, eid, event),
        None => LedgerEntry::new(date, event),
    };
    state.history.ledger.push(entry);
}

/// Process all pending events for the current date
/// Returns list of event IDs that were triggered
pub fn process_events(state: &mut SimulationState) -> Vec<EventId> {
    let mut triggered = Vec::new();

    // Collect events to evaluate (avoid borrow issues)
    let events_to_check: Vec<(EventId, Event)> = state
        .event_state
        .events
        .iter()
        .filter(|(id, event)| {
            // Skip if already triggered and once=true (but not for Repeating)
            if event.once
                && state.event_state.triggered_events.contains_key(id)
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
                state
                    .event_state
                    .repeating_event_active
                    .insert(event_id, true);
                state
                    .event_state
                    .event_next_date
                    .insert(event_id, next_date);
                true // Trigger immediately on activation
            }
            TriggerEvent::TriggerRepeating(next_date) => {
                // Schedule next occurrence
                state
                    .event_state
                    .event_next_date
                    .insert(event_id, next_date);
                true
            }
            TriggerEvent::StopRepeating => {
                // Terminate the repeating event
                state.event_state.repeating_event_active.remove(&event_id);
                state.event_state.event_next_date.remove(&event_id);
                false
            }
            TriggerEvent::NotTriggered | TriggerEvent::NextTriggerDate(_) => false,
        };

        if should_trigger {
            // Record trigger for once checks and RelativeToEvent
            state
                .event_state
                .triggered_events
                .insert(event_id, state.timeline.current_date);

            // Record event trigger to ledger
            state.history.ledger.push(LedgerEntry::with_source(
                state.timeline.current_date,
                event_id,
                StateEvent::EventTriggered { event_id },
            ));

            triggered.push(event_id);

            // Evaluate and apply effects
            for effect in &event.effects {
                match evaluate_effect(effect, state) {
                    Ok(eval_events) => {
                        for ee in eval_events {
                            if let Err(e) = apply_eval_event_with_source(state, &ee, Some(event_id))
                            {
                                // Log error but continue processing
                                eprintln!("Error applying eval event: {:?}", e);
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
            if let Some(event) = state.event_state.events.get(&event_id).cloned() {
                // Skip if already triggered and once=true
                if event.once && state.event_state.triggered_events.contains_key(&event_id) {
                    continue;
                }

                state
                    .event_state
                    .triggered_events
                    .insert(event_id, state.timeline.current_date);
                state.history.ledger.push(LedgerEntry::new(
                    state.timeline.current_date,
                    StateEvent::EventTriggered { event_id },
                ));
                triggered.push(event_id);

                for effect in &event.effects {
                    if let Ok(eval_events) = evaluate_effect(effect, state) {
                        for ee in eval_events {
                            let _ = apply_eval_event_with_source(state, &ee, Some(event_id));
                        }
                    }
                }
            }
        }
    }

    triggered
}
