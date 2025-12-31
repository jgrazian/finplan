use crate::models::*;
use crate::simulation_state::SimulationState;
use jiff::ToSpan;

/// Evaluates whether a trigger condition is met
pub fn evaluate_trigger(trigger: &EventTrigger, state: &SimulationState) -> bool {
    match trigger {
        EventTrigger::Date(date) => state.current_date >= *date,

        EventTrigger::Age { years, months } => {
            if let Some((current_years, current_months)) = state.current_age() {
                let target_months = months.unwrap_or(0);
                current_years > *years
                    || (current_years == *years && current_months >= target_months)
            } else {
                false
            }
        }

        EventTrigger::RelativeToEvent { event_id, offset } => {
            if let Some(trigger_date) = state.triggered_events.get(event_id) {
                let target_date = match offset {
                    TriggerOffset::Days(d) => trigger_date.checked_add((*d as i64).days()),
                    TriggerOffset::Months(m) => trigger_date.checked_add((*m as i64).months()),
                    TriggerOffset::Years(y) => trigger_date.checked_add((*y as i64).years()),
                };
                target_date
                    .map(|d| state.current_date >= d)
                    .unwrap_or(false)
            } else {
                false
            }
        }

        EventTrigger::AccountBalance {
            account_id,
            threshold,
            above,
        } => {
            let balance = state.account_balance(*account_id);
            if *above {
                balance >= *threshold
            } else {
                balance <= *threshold
            }
        }

        EventTrigger::AssetBalance {
            account_id,
            asset_id,
            threshold,
            above,
        } => {
            let balance = state.asset_balance(*account_id, *asset_id);
            if *above {
                balance >= *threshold
            } else {
                balance <= *threshold
            }
        }

        EventTrigger::NetWorth { threshold, above } => {
            let net_worth = state.net_worth();
            if *above {
                net_worth >= *threshold
            } else {
                net_worth <= *threshold
            }
        }

        EventTrigger::AccountDepleted(account_id) => state.account_balance(*account_id) <= 0.0,

        EventTrigger::TotalIncomeBelow(threshold) => state.calculate_total_income() < *threshold,

        EventTrigger::CashFlowEnded(cf_id) => state
            .cash_flows
            .get(cf_id)
            .map(|(_, s)| *s == CashFlowState::Terminated)
            .unwrap_or(false),

        EventTrigger::And(triggers) => triggers.iter().all(|t| evaluate_trigger(t, state)),

        EventTrigger::Or(triggers) => triggers.iter().any(|t| evaluate_trigger(t, state)),

        EventTrigger::Manual => false, // Only triggered via TriggerEvent effect
    }
}

/// Apply a single effect to the simulation state
pub fn apply_effect(
    effect: &EventEffect,
    state: &mut SimulationState,
    pending_triggers: &mut Vec<EventId>,
) {
    match effect {
        // === Account Effects ===
        EventEffect::CreateAccount(account) => {
            for asset in &account.assets {
                state
                    .asset_balances
                    .insert((account.account_id, asset.asset_id), asset.initial_value);

                // Initialize history for new account
                state
                    .account_histories
                    .entry(account.account_id)
                    .or_default()
                    .entry(asset.asset_id)
                    .or_default()
                    .push(asset.initial_value);
            }
            state.accounts.insert(account.account_id, account.clone());
        }

        EventEffect::DeleteAccount(account_id) => {
            state.accounts.remove(account_id);
            state
                .asset_balances
                .retain(|(acc_id, _), _| acc_id != account_id);
        }

        // === CashFlow Effects ===
        EventEffect::CreateCashFlow(cf) => {
            let initial_state = cf.state.clone();
            state
                .cash_flows
                .insert(cf.cash_flow_id, (*cf.clone(), initial_state.clone()));

            if initial_state == CashFlowState::Active {
                state
                    .cash_flow_next_date
                    .insert(cf.cash_flow_id, state.current_date);
            }
        }

        EventEffect::ActivateCashFlow(cf_id) => {
            if let Some((_, s)) = state.cash_flows.get_mut(cf_id)
                && *s == CashFlowState::Pending
            {
                *s = CashFlowState::Active;
                // Schedule to run now
                state.cash_flow_next_date.insert(*cf_id, state.current_date);
            }
        }

        EventEffect::PauseCashFlow(cf_id) => {
            if let Some((_, s)) = state.cash_flows.get_mut(cf_id)
                && *s == CashFlowState::Active
            {
                *s = CashFlowState::Paused;
                state.cash_flow_next_date.remove(cf_id);
            }
        }

        EventEffect::ResumeCashFlow(cf_id) => {
            if let Some((_, s)) = state.cash_flows.get_mut(cf_id)
                && *s == CashFlowState::Paused
            {
                *s = CashFlowState::Active;
                state.cash_flow_next_date.insert(*cf_id, state.current_date);
            }
        }

        EventEffect::TerminateCashFlow(cf_id) => {
            if let Some((_, s)) = state.cash_flows.get_mut(cf_id) {
                *s = CashFlowState::Terminated;
                state.cash_flow_next_date.remove(cf_id);
            }
        }

        EventEffect::ModifyCashFlow {
            cash_flow_id,
            new_amount,
            new_repeats,
        } => {
            if let Some((cf, _)) = state.cash_flows.get_mut(cash_flow_id) {
                if let Some(amount) = new_amount {
                    cf.amount = *amount;
                }
                if let Some(repeats) = new_repeats {
                    cf.repeats = repeats.clone();
                }
            }
        }

        // === SpendingTarget Effects ===
        EventEffect::CreateSpendingTarget(st) => {
            let initial_state = st.state.clone();
            state
                .spending_targets
                .insert(st.spending_target_id, (*st.clone(), initial_state.clone()));

            if initial_state == SpendingTargetState::Active {
                state
                    .spending_target_next_date
                    .insert(st.spending_target_id, state.current_date);
            }
        }

        EventEffect::ActivateSpendingTarget(st_id) => {
            if let Some((_, s)) = state.spending_targets.get_mut(st_id)
                && *s == SpendingTargetState::Pending
            {
                *s = SpendingTargetState::Active;
                state
                    .spending_target_next_date
                    .insert(*st_id, state.current_date);
            }
        }

        EventEffect::PauseSpendingTarget(st_id) => {
            if let Some((_, s)) = state.spending_targets.get_mut(st_id)
                && *s == SpendingTargetState::Active
            {
                *s = SpendingTargetState::Paused;
                state.spending_target_next_date.remove(st_id);
            }
        }

        EventEffect::ResumeSpendingTarget(st_id) => {
            if let Some((_, s)) = state.spending_targets.get_mut(st_id)
                && *s == SpendingTargetState::Paused
            {
                *s = SpendingTargetState::Active;
                state
                    .spending_target_next_date
                    .insert(*st_id, state.current_date);
            }
        }

        EventEffect::TerminateSpendingTarget(st_id) => {
            if let Some((_, s)) = state.spending_targets.get_mut(st_id) {
                *s = SpendingTargetState::Terminated;
                state.spending_target_next_date.remove(st_id);
            }
        }

        EventEffect::ModifySpendingTarget {
            spending_target_id,
            new_amount,
        } => {
            if let Some((st, _)) = state.spending_targets.get_mut(spending_target_id)
                && let Some(amount) = new_amount
            {
                st.amount = *amount;
            }
        }

        // === Asset Effects ===
        EventEffect::TransferAsset {
            from_account,
            to_account,
            from_asset_id,
            to_asset_id,
            amount,
        } => {
            let from_key = (*from_account, *from_asset_id);
            let to_key = (*to_account, *to_asset_id);

            let from_balance = state.asset_balances.get(&from_key).copied().unwrap_or(0.0);
            let transfer_amount = amount.unwrap_or(from_balance).min(from_balance);

            if let Some(balance) = state.asset_balances.get_mut(&from_key) {
                *balance -= transfer_amount;
            }

            *state.asset_balances.entry(to_key).or_insert(0.0) += transfer_amount;
        }

        // === Event Chaining ===
        EventEffect::TriggerEvent(event_id) => {
            pending_triggers.push(*event_id);
        }
    }
}

/// Process all events that should trigger on the current date
/// Returns list of (EventId, event name) for logging
pub fn process_events(state: &mut SimulationState) -> Vec<EventId> {
    let mut triggered = Vec::new();
    let mut pending_triggers: Vec<EventId> = Vec::new();

    // Collect events to evaluate (avoid borrow issues)
    let events_to_check: Vec<(EventId, Event)> = state
        .events
        .iter()
        .filter(|(id, event)| {
            // Skip if already triggered and once=true
            if event.once && state.triggered_events.contains_key(id) {
                return false;
            }
            true
        })
        .map(|(id, e)| (*id, e.clone()))
        .collect();

    // Evaluate each event
    for (event_id, event) in events_to_check {
        if evaluate_trigger(&event.trigger, state) {
            // Record trigger
            state.triggered_events.insert(event_id, state.current_date);

            triggered.push(event_id);

            // Apply effects in order
            for effect in &event.effects {
                apply_effect(effect, state, &mut pending_triggers);
            }
        }
    }

    // Process chained event triggers (with recursion protection)
    let mut depth = 0;
    while !pending_triggers.is_empty() && depth < 10 {
        depth += 1;
        let triggers = std::mem::take(&mut pending_triggers);

        for event_id in triggers {
            if let Some(event) = state.events.get(&event_id).cloned() {
                // Skip if already triggered and once=true
                if event.once && state.triggered_events.contains_key(&event_id) {
                    continue;
                }

                state.triggered_events.insert(event_id, state.current_date);
                triggered.push(event_id);

                for effect in &event.effects {
                    apply_effect(effect, state, &mut pending_triggers);
                }
            }
        }
    }

    triggered
}
