//! Apply `EvalEvents` to `SimulationState`
//!
//! This module takes the evaluated `EvalEvents` from evaluate.rs and applies them
//! to mutate the `SimulationState`, recording state changes to the ledger.

use crate::{
    error::{AccountTypeError, ApplyError, LookupError},
    evaluate::{EvalEvent, TriggerEvent, evaluate_effect_into, evaluate_trigger},
    model::{
        AccountFlavor, AssetLot, EventId, EventTrigger, LedgerEntry, SimulationWarning, StateEvent,
        WarningKind,
    },
    simulation_state::{SimulationState, cached_spans},
};

/// Pre-allocated scratch buffers for simulation hot paths.
/// Allocated once per thread and reused across Monte Carlo iterations.
#[derive(Debug)]
pub struct SimulationScratch {
    /// Scratch for triggered event IDs (`process_events_into`)
    pub triggered: Vec<EventId>,
    /// Scratch for `evaluate_effect` results
    pub eval_events: Vec<EvalEvent>,
    /// Scratch for event IDs to check
    pub event_ids_to_check: Vec<EventId>,
}

impl SimulationScratch {
    /// Create a new `SimulationScratch` with pre-allocated capacity
    #[must_use]
    pub fn new() -> Self {
        Self {
            triggered: Vec::with_capacity(16),
            eval_events: Vec::with_capacity(8),
            event_ids_to_check: Vec::with_capacity(32),
        }
    }

    /// Clear all scratch buffers for reuse
    pub fn clear(&mut self) {
        self.triggered.clear();
        self.eval_events.clear();
        self.event_ids_to_check.clear();
    }
}

impl Default for SimulationScratch {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply an `EvalEvent` to mutate the `SimulationState` and record to ledger
pub fn apply_eval_event(state: &mut SimulationState, event: &EvalEvent) -> Result<(), ApplyError> {
    apply_eval_event_with_source(state, event, None)
}

/// Apply an `EvalEvent` to mutate the `SimulationState` and record to ledger
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
                .ok_or(ApplyError::Lookup(LookupError::AccountNotFound(*to)))?;

            match &mut account.flavor {
                AccountFlavor::Bank(cash) => {
                    cash.value += net_amount;
                }
                AccountFlavor::Investment(inv) => {
                    inv.cash.value += net_amount;
                }
                _ => {
                    return Err(ApplyError::AccountType(AccountTypeError::NotACashAccount(
                        *to,
                    )));
                }
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
                .ok_or(ApplyError::Lookup(LookupError::AccountNotFound(*from)))?;

            match &mut account.flavor {
                AccountFlavor::Bank(cash) => {
                    cash.value -= net_amount;
                }
                AccountFlavor::Investment(inv) => {
                    inv.cash.value -= net_amount;
                }
                _ => {
                    return Err(ApplyError::AccountType(AccountTypeError::NotACashAccount(
                        *from,
                    )));
                }
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
            let account =
                state
                    .portfolio
                    .accounts
                    .get_mut(&to.account_id)
                    .ok_or(ApplyError::Lookup(LookupError::AccountNotFound(
                        to.account_id,
                    )))?;

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
                Err(ApplyError::AccountType(
                    AccountTypeError::NotAnInvestmentAccount(to.account_id),
                ))
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
            let account =
                state
                    .portfolio
                    .accounts
                    .get_mut(&from.account_id)
                    .ok_or(ApplyError::Lookup(LookupError::AccountNotFound(
                        from.account_id,
                    )))?;

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
                Err(ApplyError::AccountType(
                    AccountTypeError::NotAnInvestmentAccount(from.account_id),
                ))
            }
        }

        EvalEvent::TriggerEvent(event_id) => {
            // Mark event for immediate triggering
            state.pending_triggers.push(*event_id);
            // Note: EventTriggered is recorded when the event actually fires
            Ok(())
        }

        EvalEvent::PauseEvent(event_id) => {
            state.event_state.set_repeating_active(*event_id, false);

            // Record to ledger
            let ledger_event = StateEvent::EventPaused {
                event_id: *event_id,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::ResumeEvent(event_id) => {
            state.event_state.set_repeating_active(*event_id, true);

            // Record to ledger
            let ledger_event = StateEvent::EventResumed {
                event_id: *event_id,
            };
            record_ledger_entry(state, current_date, source_event, ledger_event);

            Ok(())
        }

        EvalEvent::TerminateEvent(event_id) => {
            // Mark the event as permanently terminated so it can't start or fire again
            state.event_state.set_terminated(*event_id);
            state.event_state.clear_repeating(*event_id);

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
                .ok_or(ApplyError::Lookup(LookupError::AccountNotFound(*account)))?;

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

/// Helper to record a ledger entry (skipped if `collect_ledger` is false)
#[inline]
fn record_ledger_entry(
    state: &mut SimulationState,
    date: jiff::civil::Date,
    source_event: Option<EventId>,
    event: StateEvent,
) {
    if !state.collect_ledger {
        return;
    }
    let entry = match source_event {
        Some(eid) => LedgerEntry::with_source(date, eid, event),
        None => LedgerEntry::new(date, event),
    };
    state.history.ledger.push(entry);
}

/// Process all pending events for the current date
/// Returns list of event IDs that were triggered
pub fn process_events(state: &mut SimulationState) -> Vec<EventId> {
    let mut scratch = SimulationScratch::new();
    process_events_with_scratch(state, &mut scratch);
    std::mem::take(&mut scratch.triggered)
}

/// Process all pending events for the current date, appending triggered IDs to the provided buffer
/// This avoids allocations when called in a loop with a reused buffer
pub fn process_events_into(state: &mut SimulationState, triggered: &mut Vec<EventId>) {
    let mut scratch = SimulationScratch {
        triggered: std::mem::take(triggered),
        eval_events: Vec::with_capacity(8),
        event_ids_to_check: Vec::with_capacity(32),
    };
    process_events_with_scratch(state, &mut scratch);
    *triggered = std::mem::take(&mut scratch.triggered);
}

/// Process all pending events for the current date using pre-allocated scratch buffers.
/// This is the most efficient variant - reuses all scratch buffers across calls.
pub fn process_events_with_scratch(state: &mut SimulationState, scratch: &mut SimulationScratch) {
    scratch.triggered.clear();
    scratch.eval_events.clear();
    scratch.event_ids_to_check.clear();

    // Collect event IDs to evaluate using simple loop (faster than iterator chain + extend)
    let num_events = state.event_state.events.len();
    for idx in 0..num_events {
        let Some(event) = state.event_state.events.get(idx).and_then(|o| o.as_ref()) else {
            continue;
        };
        let event_id = EventId(idx as u16);

        // Skip if already triggered and once=true (but not for Repeating)
        if event.once
            && state.event_state.is_triggered(event_id)
            && !matches!(event.trigger, EventTrigger::Repeating { .. })
        {
            continue;
        }

        scratch.event_ids_to_check.push(event_id);
    }

    // Evaluate each event - iterate by index to avoid moving out of scratch
    let current_date = state.timeline.current_date;
    for i in 0..scratch.event_ids_to_check.len() {
        let event_id = scratch.event_ids_to_check[i];

        // Early-skip optimization: if we know this event can't trigger until a future date, skip it
        if let Some(next_trigger) = state.event_state.next_possible_trigger(event_id)
            && current_date < next_trigger
        {
            continue;
        }

        // Get trigger reference without cloning - borrow ends when evaluate_trigger returns
        let trigger_result = {
            let trigger = match state.event_state.get_event(event_id) {
                Some(event) => &event.trigger,
                None => continue,
            };
            match evaluate_trigger(&event_id, trigger, state) {
                Ok(result) => result,
                Err(_) => continue, // Skip events that fail to evaluate
            }
        };

        let should_trigger = match trigger_result {
            TriggerEvent::Triggered => {
                // For balance-based triggers (AccountBalance, AssetBalance, NetWorth),
                // set a 1-day cooldown to prevent them from re-firing in the inner loop
                // on the same simulation date. These triggers can't predict when they'll
                // next be relevant, so without a cooldown they'd be evaluated every
                // iteration, potentially causing 1000+ iterations per time step.
                let trigger_type = state.event_state.get_event(event_id).map(|e| &e.trigger);
                let needs_cooldown = matches!(
                    trigger_type,
                    Some(
                        EventTrigger::AccountBalance { .. }
                            | EventTrigger::AssetBalance { .. }
                            | EventTrigger::NetWorth { .. }
                    )
                );

                if needs_cooldown {
                    let tomorrow = state
                        .timeline
                        .current_date
                        .saturating_add(*cached_spans::ONE_DAY);
                    state
                        .event_state
                        .set_next_possible_trigger(event_id, tomorrow);
                } else {
                    // Clear next_possible_trigger since event is firing
                    state.event_state.clear_next_possible_trigger(event_id);
                }
                true
            }
            TriggerEvent::StartRepeating(next_date) => {
                // Activate the repeating event
                state.event_state.set_repeating_active(event_id, true);
                state.event_state.set_next_date(event_id, next_date);
                // Set next possible trigger to the scheduled next occurrence
                state
                    .event_state
                    .set_next_possible_trigger(event_id, next_date);
                // Increment occurrence count for max_occurrences tracking
                state.event_state.increment_occurrence_count(event_id);
                true // Trigger immediately on activation
            }
            TriggerEvent::TriggerRepeating(next_date) => {
                // Schedule next occurrence
                state.event_state.set_next_date(event_id, next_date);
                // Set next possible trigger to the scheduled next occurrence
                state
                    .event_state
                    .set_next_possible_trigger(event_id, next_date);
                // Increment occurrence count for max_occurrences tracking
                state.event_state.increment_occurrence_count(event_id);
                true
            }
            TriggerEvent::StopRepeating => {
                // Terminate the repeating event
                state.event_state.clear_repeating(event_id);
                state.event_state.clear_next_possible_trigger(event_id);
                false
            }
            TriggerEvent::NextTriggerDate(date) => {
                // Record when this event might next trigger for early-skip optimization
                state.event_state.set_next_possible_trigger(event_id, date);
                false
            }
            TriggerEvent::NotTriggered => false,
        };

        if should_trigger {
            // Record trigger for once checks and RelativeToEvent
            state
                .event_state
                .set_triggered(event_id, state.timeline.current_date);

            // Record event trigger to ledger
            if state.collect_ledger {
                state.history.ledger.push(LedgerEntry::with_source(
                    state.timeline.current_date,
                    event_id,
                    StateEvent::EventTriggered { event_id },
                ));
            }

            scratch.triggered.push(event_id);

            // Get effects length to iterate by index, avoiding clone of effects vector
            let effects_len = state
                .event_state
                .get_event(event_id)
                .map_or(0, |e| e.effects.len());

            // Evaluate and apply effects by index, avoiding EventEffect clone
            for effect_idx in 0..effects_len {
                // Phase 1: Evaluate with immutable borrows (no clone needed)
                scratch.eval_events.clear();
                let eval_result = {
                    // Borrow effect without cloning - both borrows are immutable so this is safe
                    let Some(effect) = state
                        .event_state
                        .get_event(event_id)
                        .and_then(|e| e.effects.get(effect_idx))
                    else {
                        break;
                    };
                    // Evaluate while holding effect borrow
                    evaluate_effect_into(effect, state, &mut scratch.eval_events)
                }; // effect borrow ends here

                // Phase 2: Apply with mutable borrow
                match eval_result {
                    Ok(()) => {
                        for ee in scratch.eval_events.drain(..) {
                            if let Err(e) = apply_eval_event_with_source(state, &ee, Some(event_id))
                            {
                                // Record warning but continue processing
                                state.warnings.push(SimulationWarning {
                                    date: state.timeline.current_date,
                                    event_id: Some(event_id),
                                    message: format!("failed to apply effect: {e}"),
                                    kind: WarningKind::EffectSkipped,
                                });
                            }
                        }
                    }
                    Err(e) => {
                        state.warnings.push(SimulationWarning {
                            date: state.timeline.current_date,
                            event_id: Some(event_id),
                            message: format!("failed to evaluate effect: {e}"),
                            kind: WarningKind::EvaluationFailed,
                        });
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
            // Check if event exists and get `once` flag without cloning
            let Some(event) = state.event_state.get_event(event_id) else {
                continue;
            };
            let is_once = event.once;

            // Skip if already triggered and once=true
            if is_once && state.event_state.is_triggered(event_id) {
                continue;
            }

            state
                .event_state
                .set_triggered(event_id, state.timeline.current_date);
            if state.collect_ledger {
                state.history.ledger.push(LedgerEntry::new(
                    state.timeline.current_date,
                    StateEvent::EventTriggered { event_id },
                ));
            }
            scratch.triggered.push(event_id);

            // Get effects length to iterate by index, avoiding clone of entire Event
            let effects_len = state
                .event_state
                .get_event(event_id)
                .map_or(0, |e| e.effects.len());

            // Evaluate and apply effects by index, avoiding EventEffect clone
            for effect_idx in 0..effects_len {
                // Phase 1: Evaluate with immutable borrows (no clone needed)
                scratch.eval_events.clear();
                let eval_ok = {
                    // Borrow effect without cloning
                    let Some(effect) = state
                        .event_state
                        .get_event(event_id)
                        .and_then(|e| e.effects.get(effect_idx))
                    else {
                        break;
                    };
                    evaluate_effect_into(effect, state, &mut scratch.eval_events).is_ok()
                }; // effect borrow ends here

                // Phase 2: Apply with mutable borrow
                if eval_ok {
                    for ee in scratch.eval_events.drain(..) {
                        let _ = apply_eval_event_with_source(state, &ee, Some(event_id));
                    }
                }
            }
        }
    }
}
