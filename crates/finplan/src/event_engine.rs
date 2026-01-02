use crate::model::{
    AccountType, Event, EventEffect, EventId, EventTrigger, LimitPeriod, LotMethod, Record,
    RecordKind, TransferAmount, TransferEndpoint, TriggerOffset,
};
use crate::simulation_state::SimulationState;
use jiff::ToSpan;

/// Evaluate a TransferAmount expression to get the actual dollar amount
fn evaluate_transfer_amount(
    amount: &TransferAmount,
    from: Option<&TransferEndpoint>,
    to: Option<&TransferEndpoint>,
    state: &SimulationState,
) -> f64 {
    match amount {
        TransferAmount::Fixed(amt) => *amt,

        TransferAmount::SourceBalance => {
            if let Some(TransferEndpoint::Asset {
                account_id,
                asset_id,
            }) = from
            {
                state.asset_balance(*account_id, *asset_id)
            } else {
                0.0 // External source has infinite balance conceptually
            }
        }

        TransferAmount::ZeroTargetBalance => {
            if let Some(TransferEndpoint::Asset {
                account_id,
                asset_id,
            }) = to
            {
                let balance = state.asset_balance(*account_id, *asset_id);
                balance
            } else {
                0.0
            }
        }

        TransferAmount::TargetToBalance(target) => {
            if let Some(TransferEndpoint::Asset {
                account_id,
                asset_id,
            }) = to
            {
                let current = state.asset_balance(*account_id, *asset_id);
                (target - current).max(0.0)
            } else {
                0.0
            }
        }

        TransferAmount::AssetBalance {
            account_id,
            asset_id,
        } => state.asset_balance(*account_id, *asset_id),

        TransferAmount::AccountBalance { account_id } => state.account_balance(*account_id),

        TransferAmount::Min(left, right) => {
            let left_val = evaluate_transfer_amount(left, from, to, state);
            let right_val = evaluate_transfer_amount(right, from, to, state);
            left_val.min(right_val)
        }

        TransferAmount::Max(left, right) => {
            let left_val = evaluate_transfer_amount(left, from, to, state);
            let right_val = evaluate_transfer_amount(right, from, to, state);
            left_val.max(right_val)
        }

        TransferAmount::Sub(left, right) => {
            let left_val = evaluate_transfer_amount(left, from, to, state);
            let right_val = evaluate_transfer_amount(right, from, to, state);
            left_val - right_val
        }

        TransferAmount::Add(left, right) => {
            let left_val = evaluate_transfer_amount(left, from, to, state);
            let right_val = evaluate_transfer_amount(right, from, to, state);
            left_val + right_val
        }

        TransferAmount::Mul(left, right) => {
            let left_val = evaluate_transfer_amount(left, from, to, state);
            let right_val = evaluate_transfer_amount(right, from, to, state);
            left_val * right_val
        }
    }
}

/// Resolve withdrawal sources based on strategy or custom list
fn resolve_withdrawal_sources(
    sources: &crate::model::WithdrawalSources,
    state: &SimulationState,
) -> Vec<(crate::model::AccountId, crate::model::AssetId)> {
    use crate::model::{WithdrawalOrder, WithdrawalSources};

    match sources {
        WithdrawalSources::Custom(list) => list.clone(),
        WithdrawalSources::Strategy {
            order,
            exclude_accounts,
        } => {
            let mut accounts: Vec<_> = state
                .accounts
                .iter()
                .filter(|(id, _)| !exclude_accounts.contains(id))
                .collect();

            // Sort by strategy
            match order {
                WithdrawalOrder::TaxEfficientEarly => {
                    accounts.sort_by_key(|(_, acc)| match acc.account_type {
                        AccountType::Taxable => 0,
                        AccountType::TaxDeferred => 1,
                        AccountType::TaxFree => 2,
                        AccountType::Illiquid => 3,
                    });
                }
                WithdrawalOrder::TaxDeferredFirst => {
                    accounts.sort_by_key(|(_, acc)| match acc.account_type {
                        AccountType::TaxDeferred => 0,
                        AccountType::Taxable => 1,
                        AccountType::TaxFree => 2,
                        AccountType::Illiquid => 3,
                    });
                }
                WithdrawalOrder::TaxFreeFirst => {
                    accounts.sort_by_key(|(_, acc)| match acc.account_type {
                        AccountType::TaxFree => 0,
                        AccountType::Taxable => 1,
                        AccountType::TaxDeferred => 2,
                        AccountType::Illiquid => 3,
                    });
                }
                WithdrawalOrder::ProRata => {
                    // Pro-rata: return all accounts (proportional withdrawal handled in caller)
                }
            }

            // Flatten to (AccountId, AssetId) pairs
            accounts
                .iter()
                .flat_map(|(acc_id, acc)| acc.assets.iter().map(|asset| (**acc_id, asset.asset_id)))
                .collect()
        }
    }
}

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
    event_id: EventId,
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

        // === New Money Movement Effects ===
        EventEffect::Transfer {
            from,
            to,
            amount,
            adjust_for_inflation,
            limits,
        } => {
            // Calculate the actual amount to transfer
            let mut calculated_amount =
                evaluate_transfer_amount(amount, Some(from), Some(to), state);

            // Apply inflation adjustment if requested
            if *adjust_for_inflation {
                let year_index = state.dates.len();
                if year_index > 0 && year_index <= state.cumulative_inflation.len() {
                    calculated_amount *= state.cumulative_inflation[year_index - 1];
                }
            }

            // Apply flow limits if specified
            if let Some(flow_limits) = limits {
                let accumulated = match flow_limits.period {
                    LimitPeriod::Yearly => {
                        state.event_flow_ytd.get(&event_id).copied().unwrap_or(0.0)
                    }
                    LimitPeriod::Lifetime => state
                        .event_flow_lifetime
                        .get(&event_id)
                        .copied()
                        .unwrap_or(0.0),
                };

                // Cap amount to not exceed limit
                let remaining = (flow_limits.limit - accumulated).max(0.0);
                calculated_amount = calculated_amount.min(remaining);

                // Update accumulators
                match flow_limits.period {
                    LimitPeriod::Yearly => {
                        *state.event_flow_ytd.entry(event_id).or_insert(0.0) += calculated_amount;
                        let current_year = state.current_date.year();
                        state
                            .event_flow_last_period_key
                            .insert(event_id, current_year);
                    }
                    LimitPeriod::Lifetime => {
                        *state.event_flow_lifetime.entry(event_id).or_insert(0.0) +=
                            calculated_amount;
                    }
                }
            }

            // Execute the transfer
            if calculated_amount > 0.0 {
                match (from, to) {
                    (
                        TransferEndpoint::External,
                        TransferEndpoint::Asset {
                            account_id,
                            asset_id,
                        },
                    ) => {
                        // Income: external -> asset
                        let balance = state
                            .asset_balances
                            .entry((*account_id, *asset_id))
                            .or_insert(0.0);
                        *balance += calculated_amount;

                        // Record as income
                        state.records.push(Record::new(
                            state.current_date,
                            RecordKind::Income {
                                to_account_id: *account_id,
                                to_asset_id: *asset_id,
                                amount: calculated_amount,
                                event_id,
                            },
                        ));

                        // Track for taxes if applicable
                        if let Some(account) = state.accounts.get(account_id) {
                            if matches!(
                                account.account_type,
                                AccountType::Taxable | AccountType::TaxDeferred
                            ) {
                                state.ytd_tax.ordinary_income += calculated_amount;
                            }
                        }
                    }

                    (
                        TransferEndpoint::Asset {
                            account_id,
                            asset_id,
                        },
                        TransferEndpoint::External,
                    ) => {
                        // Expense: asset -> external
                        let balance = state
                            .asset_balances
                            .entry((*account_id, *asset_id))
                            .or_insert(0.0);
                        let actual_amount = calculated_amount.min(*balance);
                        *balance -= actual_amount;

                        // Record as expense
                        state.records.push(Record::new(
                            state.current_date,
                            RecordKind::Expense {
                                from_account_id: *account_id,
                                from_asset_id: *asset_id,
                                amount: actual_amount,
                                event_id,
                            },
                        ));
                    }

                    (
                        TransferEndpoint::Asset {
                            account_id: from_acc,
                            asset_id: from_asset,
                        },
                        TransferEndpoint::Asset {
                            account_id: to_acc,
                            asset_id: to_asset,
                        },
                    ) => {
                        // Internal transfer: asset -> asset
                        let from_balance = state
                            .asset_balances
                            .entry((*from_acc, *from_asset))
                            .or_insert(0.0);
                        let actual_amount = calculated_amount.min(*from_balance);
                        *from_balance -= actual_amount;

                        let to_balance = state
                            .asset_balances
                            .entry((*to_acc, *to_asset))
                            .or_insert(0.0);
                        *to_balance += actual_amount;

                        // Record as transfer
                        state.records.push(Record::new(
                            state.current_date,
                            RecordKind::Transfer {
                                from_account_id: *from_acc,
                                from_asset_id: *from_asset,
                                to_account_id: *to_acc,
                                to_asset_id: *to_asset,
                                amount: actual_amount,
                                event_id,
                            },
                        ));
                    }

                    (TransferEndpoint::External, TransferEndpoint::External) => {
                        // Invalid: external -> external
                        eprintln!("WARNING: Invalid transfer from External to External");
                    }
                }
            }
        }

        EventEffect::Liquidate {
            from_account,
            from_asset,
            to_account,
            to_asset,
            amount,
            lot_method,
        } => {
            // Calculate amount to liquidate
            let from_endpoint = TransferEndpoint::Asset {
                account_id: *from_account,
                asset_id: *from_asset,
            };
            let calculated_amount =
                evaluate_transfer_amount(amount, Some(&from_endpoint), None, state);

            if calculated_amount <= 0.0 {
                return;
            }

            // Get current balance
            let balance = state.asset_balance(*from_account, *from_asset);
            let actual_amount = calculated_amount.min(balance);

            // Check if this is a taxable account - if not, treat as simple transfer
            let account = state.accounts.get(from_account);
            let is_taxable = account
                .map(|a| matches!(a.account_type, AccountType::Taxable))
                .unwrap_or(false);

            if !is_taxable {
                // Simple transfer without cost basis tracking
                *state
                    .asset_balances
                    .entry((*from_account, *from_asset))
                    .or_insert(0.0) -= actual_amount;
                *state
                    .asset_balances
                    .entry((*to_account, *to_asset))
                    .or_insert(0.0) += actual_amount;

                state.records.push(Record::new(
                    state.current_date,
                    RecordKind::Transfer {
                        from_account_id: *from_account,
                        from_asset_id: *from_asset,
                        to_account_id: *to_account,
                        to_asset_id: *to_asset,
                        amount: actual_amount,
                        event_id,
                    },
                ));
                return;
            }

            // Taxable account - need cost basis tracking
            let lots = state
                .asset_lots
                .entry((*from_account, *from_asset))
                .or_insert_with(Vec::new);

            // Sort lots based on method
            match lot_method {
                LotMethod::Fifo => lots.sort_by_key(|l| l.purchase_date),
                LotMethod::Lifo => lots.sort_by(|a, b| b.purchase_date.cmp(&a.purchase_date)),
                LotMethod::HighestCost => lots.sort_by(|a, b| {
                    let a_basis_per_unit = if a.units > 0.0 {
                        a.cost_basis / a.units
                    } else {
                        0.0
                    };
                    let b_basis_per_unit = if b.units > 0.0 {
                        b.cost_basis / b.units
                    } else {
                        0.0
                    };
                    b_basis_per_unit
                        .partial_cmp(&a_basis_per_unit)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }),
                LotMethod::LowestCost => lots.sort_by(|a, b| {
                    let a_basis_per_unit = if a.units > 0.0 {
                        a.cost_basis / a.units
                    } else {
                        0.0
                    };
                    let b_basis_per_unit = if b.units > 0.0 {
                        b.cost_basis / b.units
                    } else {
                        0.0
                    };
                    a_basis_per_unit
                        .partial_cmp(&b_basis_per_unit)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }),
                LotMethod::AverageCost => {
                    // For average cost, we'll compute average and treat it as one lot conceptually
                }
            }

            // Consume lots to satisfy the liquidation amount
            let mut remaining = actual_amount;
            let mut total_cost_basis = 0.0;
            let mut short_term_gain = 0.0;
            let mut long_term_gain = 0.0;
            let mut lots_to_remove = Vec::new();

            for (idx, lot) in lots.iter_mut().enumerate() {
                if remaining <= 0.001 {
                    break;
                }

                let lot_value = lot.units; // Assuming units represent dollar value for simplicity
                let take_amount = remaining.min(lot_value);
                let take_fraction = if lot_value > 0.0 {
                    take_amount / lot_value
                } else {
                    0.0
                };

                let basis_used = lot.cost_basis * take_fraction;
                total_cost_basis += basis_used;

                let gain = take_amount - basis_used;

                // Determine if short-term or long-term
                let holding_days = (state.current_date - lot.purchase_date).get_days();
                if holding_days >= 365 {
                    long_term_gain += gain;
                } else {
                    short_term_gain += gain;
                }

                // Reduce lot
                lot.units -= take_amount;
                lot.cost_basis -= basis_used;

                if lot.units <= 0.001 {
                    lots_to_remove.push(idx);
                }

                remaining -= take_amount;
            }

            // Remove depleted lots (in reverse order to maintain indices)
            for idx in lots_to_remove.iter().rev() {
                lots.remove(*idx);
            }

            // Calculate taxes on gains
            let federal_tax = if short_term_gain > 0.0 || long_term_gain > 0.0 {
                // Simplified tax calculation - should use full tax system
                short_term_gain * 0.22 + long_term_gain * 0.15 // Placeholder rates
            } else {
                0.0
            };

            let state_tax = if short_term_gain > 0.0 || long_term_gain > 0.0 {
                (short_term_gain + long_term_gain) * 0.05 // Placeholder rate
            } else {
                0.0
            };

            let net_proceeds = actual_amount - federal_tax - state_tax;

            // Update balances
            *state
                .asset_balances
                .entry((*from_account, *from_asset))
                .or_insert(0.0) -= actual_amount;
            *state
                .asset_balances
                .entry((*to_account, *to_asset))
                .or_insert(0.0) += net_proceeds;

            // Track capital gains for year-end tax calculation
            state.ytd_tax.capital_gains += short_term_gain + long_term_gain;
            state.ytd_tax.federal_tax += federal_tax;
            state.ytd_tax.state_tax += state_tax;

            // Record liquidation
            state.records.push(Record::new(
                state.current_date,
                RecordKind::Liquidation {
                    from_account_id: *from_account,
                    from_asset_id: *from_asset,
                    to_account_id: *to_account,
                    to_asset_id: *to_asset,
                    gross_amount: actual_amount,
                    cost_basis: total_cost_basis,
                    short_term_gain,
                    long_term_gain,
                    federal_tax,
                    state_tax,
                    net_proceeds,
                    lot_method: *lot_method,
                    event_id,
                },
            ));
        }

        EventEffect::Sweep {
            to_account,
            to_asset,
            target,
            sources,
            amount_mode,
            lot_method: _lot_method, // TODO: Use lot_method for taxable account withdrawals
        } => {
            use crate::model::WithdrawalAmountMode;

            // Calculate target amount
            let to_endpoint = TransferEndpoint::Asset {
                account_id: *to_account,
                asset_id: *to_asset,
            };
            let target_amount = evaluate_transfer_amount(target, None, Some(&to_endpoint), state);

            if target_amount <= 0.0 {
                return;
            }

            // Resolve withdrawal sources
            let source_list = resolve_withdrawal_sources(sources, state);

            // For simplicity, we'll use Gross mode for now
            // TODO: Implement proper Net mode with tax gross-up calculation
            let gross_needed = match amount_mode {
                WithdrawalAmountMode::Gross => target_amount,
                WithdrawalAmountMode::Net => {
                    // Simplified: assume 20% tax rate for gross-up
                    // In reality, should use proper tax calculation
                    target_amount / 0.8
                }
            };

            let mut remaining = gross_needed;
            let mut total_withdrawn = 0.0;

            // Withdraw from sources in order until target is met
            for (src_account, src_asset) in source_list {
                if remaining <= 0.001 {
                    break;
                }

                let available = state.asset_balance(src_account, src_asset);
                if available <= 0.001 {
                    continue;
                }

                let take = remaining.min(available);

                // Check if source account is taxable - use Liquidate for tax tracking
                let account = state.accounts.get(&src_account);
                let is_taxable = account
                    .map(|a| matches!(a.account_type, AccountType::Taxable))
                    .unwrap_or(false);

                if is_taxable {
                    // Use liquidation logic for taxable accounts
                    let from_endpoint = TransferEndpoint::Asset {
                        account_id: src_account,
                        asset_id: src_asset,
                    };
                    let take_amount = evaluate_transfer_amount(
                        &TransferAmount::Fixed(take),
                        Some(&from_endpoint),
                        None,
                        state,
                    );

                    // Simplified liquidation (reusing some logic from Liquidate effect)
                    // In a real implementation, we'd extract this into a helper function
                    let balance = state.asset_balance(src_account, src_asset);
                    let actual_amount = take_amount.min(balance);

                    // Simple transfer for now (not implementing full lot tracking in Sweep)
                    *state
                        .asset_balances
                        .entry((src_account, src_asset))
                        .or_insert(0.0) -= actual_amount;
                    *state
                        .asset_balances
                        .entry((*to_account, *to_asset))
                        .or_insert(0.0) += actual_amount;

                    total_withdrawn += actual_amount;
                    remaining -= actual_amount;
                } else {
                    // Non-taxable: simple transfer
                    *state
                        .asset_balances
                        .entry((src_account, src_asset))
                        .or_insert(0.0) -= take;
                    *state
                        .asset_balances
                        .entry((*to_account, *to_asset))
                        .or_insert(0.0) += take;

                    total_withdrawn += take;
                    remaining -= take;
                }
            }

            // Record sweep
            let actual_net = total_withdrawn; // Simplified - should account for taxes
            state.records.push(Record::new(
                state.current_date,
                RecordKind::Sweep {
                    to_account_id: *to_account,
                    to_asset_id: *to_asset,
                    target_amount,
                    actual_gross: total_withdrawn,
                    actual_net,
                    amount_mode: *amount_mode,
                    event_id,
                },
            ));
        }

        // === Event Control Effects ===
        EventEffect::PauseEvent(event_id) => {
            // Mark repeating event as inactive
            state.repeating_event_active.insert(*event_id, false);
            state.event_next_date.remove(event_id);
        }

        EventEffect::ResumeEvent(event_id) => {
            // Mark repeating event as active
            state.repeating_event_active.insert(*event_id, true);
            state.event_next_date.insert(*event_id, state.current_date);
        }

        EventEffect::TerminateEvent(event_id) => {
            // Remove event from active tracking
            state.repeating_event_active.remove(event_id);
            state.event_next_date.remove(event_id);
            // Mark it as triggered if it was a once event
            state.triggered_events.insert(*event_id, state.current_date);
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
            // TODO: Implement RMD withdrawal using new Sweep effect
            eprintln!("WARNING: RMD CreateRmdWithdrawal effect needs reimplementation");
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
                end_condition,
            } => {
                // Check if this repeating event is active
                let is_active = state
                    .repeating_event_active
                    .get(&event_id)
                    .copied()
                    .unwrap_or(false);

                // Check if end_condition is met - if so, terminate the event
                if let Some(end_cond) = end_condition {
                    if evaluate_trigger(end_cond, state) {
                        state.repeating_event_active.remove(&event_id);
                        state.event_next_date.remove(&event_id);
                        continue; // Skip this event
                    }
                }

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
                apply_effect(effect, state, event_id, &mut pending_triggers);
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
                    apply_effect(effect, state, event_id, &mut pending_triggers);
                }
            }
        }
    }

    triggered
}
