use crate::model::{
    AccountType, CashFlowState, Event, EventEffect, EventId, EventTrigger, Record, RepeatInterval,
    RmdTable, SpendingTarget, SpendingTargetId, SpendingTargetState, TaxConfig, TriggerOffset,
    WithdrawalStrategy,
};
use crate::simulation_state::SimulationState;
use crate::taxes::{calculate_liquidation_tax, gross_up_liquidation_for_net_target};
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
        } => {
            let balance = state.account_balance(*account_id);
            threshold.evaluate(balance)
        }

        EventTrigger::AssetBalance {
            account_id,
            asset_id,
            threshold,
        } => {
            let balance = state.asset_balance(*account_id, *asset_id);
            threshold.evaluate(balance)
        }

        EventTrigger::NetWorth { threshold } => {
            let net_worth = state.net_worth();
            threshold.evaluate(net_worth)
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

        EventTrigger::Repeating { .. } => {
            // Repeating triggers are handled specially in process_events
            // This should not be called directly for scheduling logic
            false
        }

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
            let initial_state = cf.state;
            state
                .cash_flows
                .insert(cf.cash_flow_id, (*cf.clone(), initial_state));

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
                    cf.repeats = *repeats;
                }
            }
        }

        // === SpendingTarget Effects ===
        EventEffect::CreateSpendingTarget(st) => {
            let initial_state = st.state;
            state
                .spending_targets
                .insert(st.spending_target_id, (*st.clone(), initial_state));

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

            if transfer_amount > 0.0 {
                if let Some(balance) = state.asset_balances.get_mut(&from_key) {
                    *balance -= transfer_amount;
                }

                *state.asset_balances.entry(to_key).or_insert(0.0) += transfer_amount;

                // Record the transfer transaction (triggered_by will be set by caller)
                state.records.push(Record::transfer(
                    state.current_date,
                    *from_account,
                    *from_asset_id,
                    *to_account,
                    *to_asset_id,
                    transfer_amount,
                    None, // TODO: pass event_id from caller
                ));
            }
        }

        // === Event Chaining ===
        EventEffect::TriggerEvent(event_id) => {
            pending_triggers.push(*event_id);
        }

        // === RMD Effects ===
        EventEffect::CreateRmdWithdrawal {
            account_id,
            starting_age,
        } => {
            // Register this account for RMD tracking
            state.active_rmd_accounts.insert(*account_id, *starting_age);

            // Calculate RMD amount using IRS table
            let rmd_table = RmdTable::irs_uniform_lifetime_2024();

            if let Some((current_age, _)) = state.current_age() {
                // Use prior year balance if available, otherwise current balance
                let balance_for_rmd = state
                    .prior_year_end_balance(*account_id)
                    .unwrap_or_else(|| state.account_balance(*account_id));

                if let Some(divisor) = state.current_rmd_divisor(&rmd_table) {
                    let rmd_amount = balance_for_rmd / divisor;

                    // Generate unique SpendingTargetId
                    let max_id = state
                        .spending_targets
                        .keys()
                        .map(|id| id.0)
                        .max()
                        .unwrap_or(0);
                    let st_id = SpendingTargetId(max_id + 1);

                    let spending_target = SpendingTarget {
                        spending_target_id: st_id,
                        amount: rmd_amount,
                        net_amount_mode: false,         // RMD is gross amount
                        repeats: RepeatInterval::Never, // One-time; event creates new RMD each year
                        adjust_for_inflation: false,    // RMD recalculates based on balance
                        withdrawal_strategy: WithdrawalStrategy::Sequential {
                            order: vec![*account_id],
                        },
                        exclude_accounts: Vec::new(),
                        state: SpendingTargetState::Active,
                    };

                    // Add to state
                    state.spending_targets.insert(
                        st_id,
                        (spending_target.clone(), SpendingTargetState::Active),
                    );
                    state
                        .spending_target_next_date
                        .insert(st_id, state.current_date);

                    // Record RMD (actual_withdrawn starts at 0, updated as withdrawals occur)
                    state.records.push(Record::rmd(
                        state.current_date,
                        *account_id,
                        current_age,
                        balance_for_rmd,
                        divisor,
                        rmd_amount,
                        0.0, // actual_withdrawn updated later
                        st_id,
                    ));
                }
            }
        }

        // === Cash Management Effects ===
        EventEffect::SweepToAccount {
            target_account_id,
            target_asset_id,
            target_balance,
            funding_sources,
        } => {
            // Get current balance of target account/asset
            let current_balance = state
                .asset_balances
                .get(&(*target_account_id, *target_asset_id))
                .copied()
                .unwrap_or(0.0);

            // Calculate how much we need to add
            let needed = target_balance - current_balance;
            if needed <= 0.0 {
                return; // Already at or above target
            }

            let mut remaining_needed = needed;

            // Try each funding source in order
            for (from_account_id, from_asset_id) in funding_sources {
                if remaining_needed <= 0.0 {
                    break;
                }

                // Get account type for tax calculation
                let account_type = state
                    .accounts
                    .get(from_account_id)
                    .map(|a| &a.account_type)
                    .cloned()
                    .unwrap_or(AccountType::Taxable);

                // Skip illiquid accounts
                if matches!(account_type, AccountType::Illiquid) {
                    continue;
                }

                // Get available balance
                let available = state
                    .asset_balances
                    .get(&(*from_account_id, *from_asset_id))
                    .copied()
                    .unwrap_or(0.0);

                if available <= 0.0 {
                    continue;
                }

                // Calculate how much to liquidate to get remaining_needed net
                // Use default tax config if not available (will be improved when we pass params)
                let tax_config = TaxConfig::default();
                let gross_needed = gross_up_liquidation_for_net_target(
                    remaining_needed,
                    &account_type,
                    &tax_config,
                    state.ytd_tax.ordinary_income,
                )
                .unwrap_or(remaining_needed);

                let gross_to_liquidate = gross_needed.min(available);

                // Calculate actual taxes
                let tax_result = calculate_liquidation_tax(
                    gross_to_liquidate,
                    &account_type,
                    &tax_config,
                    state.ytd_tax.ordinary_income,
                );

                // Update source balance
                if let Some(balance) = state
                    .asset_balances
                    .get_mut(&(*from_account_id, *from_asset_id))
                {
                    *balance -= gross_to_liquidate;
                }

                // Update target balance (receives net amount)
                *state
                    .asset_balances
                    .entry((*target_account_id, *target_asset_id))
                    .or_insert(0.0) += tax_result.net_amount;

                // Track taxes
                match account_type {
                    AccountType::TaxDeferred => {
                        state.ytd_tax.ordinary_income += gross_to_liquidate;
                    }
                    AccountType::Taxable => {
                        state.ytd_tax.capital_gains += tax_result.realized_gain;
                    }
                    _ => {}
                }
                state.ytd_tax.federal_tax += tax_result.federal_tax;
                state.ytd_tax.state_tax += tax_result.state_tax;

                // Record the liquidation
                state.records.push(Record::liquidation(
                    state.current_date,
                    *from_account_id,
                    *from_asset_id,
                    *target_account_id,
                    *target_asset_id,
                    gross_to_liquidate,
                    tax_result.cost_basis,
                    tax_result.realized_gain,
                    tax_result.federal_tax,
                    tax_result.state_tax,
                    tax_result.net_amount,
                    None, // TODO: pass event_id from caller
                ));

                remaining_needed -= tax_result.net_amount;
            }
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
        let should_trigger = match &event.trigger {
            EventTrigger::Repeating {
                interval,
                start_condition,
            } => {
                // Check if this repeating event is active
                let is_active = state
                    .repeating_event_active
                    .get(&event_id)
                    .copied()
                    .unwrap_or(false);

                if !is_active {
                    // Check if start_condition is met (or no condition)
                    let condition_met = match start_condition {
                        None => true,
                        Some(condition) => evaluate_trigger(condition, state),
                    };

                    if condition_met {
                        // Activate the repeating event and schedule NEXT occurrence
                        state.repeating_event_active.insert(event_id, true);
                        let next = state.current_date.saturating_add(interval.span());
                        state.event_next_date.insert(event_id, next);
                        true // Trigger immediately on activation
                    } else {
                        false
                    }
                } else {
                    // Check if scheduled for today
                    if let Some(next_date) = state.event_next_date.get(&event_id) {
                        if state.current_date >= *next_date {
                            // Schedule next occurrence
                            let next = next_date.saturating_add(interval.span());
                            state.event_next_date.insert(event_id, next);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
            }
            other => evaluate_trigger(other, state),
        };

        if should_trigger {
            // Record trigger for once checks and RelativeToEvent
            state.triggered_events.insert(event_id, state.current_date);

            // Record to linear event history for replay
            state
                .records
                .push(Record::event(state.current_date, event_id));

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

                // Record to linear event history for replay
                state
                    .records
                    .push(Record::event(state.current_date, event_id));

                triggered.push(event_id);

                for effect in &event.effects {
                    apply_effect(effect, state, &mut pending_triggers);
                }
            }
        }
    }

    triggered
}
