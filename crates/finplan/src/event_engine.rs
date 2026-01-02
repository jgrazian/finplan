use crate::model::{
    AccountId, AccountType, AssetId, Event, EventEffect, EventId, EventTrigger, LimitPeriod,
    LotMethod, Record, RecordKind, TransferAmount, TransferEndpoint, TriggerOffset,
    WithdrawalAmountMode, WithdrawalOrder, WithdrawalSources,
};
use crate::simulation_state::SimulationState;
use jiff::ToSpan;

/// Result of liquidating assets from a single source
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields tracked for potential future reporting
struct LiquidationResult {
    gross_amount: f64,
    net_proceeds: f64,
    cost_basis: f64,
    short_term_gain: f64,
    long_term_gain: f64,
    federal_tax: f64,
    state_tax: f64,
}

/// Liquidate assets from a single source with proper lot tracking and tax calculation
/// Returns the result of the liquidation including taxes paid
fn liquidate_from_source(
    src_account: AccountId,
    src_asset: AssetId,
    to_account: AccountId,
    to_asset: AssetId,
    amount: f64,
    lot_method: LotMethod,
    state: &mut SimulationState,
    event_id: EventId,
) -> LiquidationResult {
    let balance = state.asset_balance(src_account, src_asset);
    let actual_amount = amount.min(balance);

    if actual_amount <= 0.001 {
        return LiquidationResult {
            gross_amount: 0.0,
            net_proceeds: 0.0,
            cost_basis: 0.0,
            short_term_gain: 0.0,
            long_term_gain: 0.0,
            federal_tax: 0.0,
            state_tax: 0.0,
        };
    }

    // Check if this is a taxable account
    let account = state.accounts.get(&src_account);
    let is_taxable = account
        .map(|a| matches!(a.account_type, AccountType::Taxable))
        .unwrap_or(false);
    let is_tax_deferred = account
        .map(|a| matches!(a.account_type, AccountType::TaxDeferred))
        .unwrap_or(false);

    if !is_taxable {
        // Non-taxable accounts: simple transfer, no lot tracking needed
        *state
            .asset_balances
            .entry((src_account, src_asset))
            .or_insert(0.0) -= actual_amount;
        *state
            .asset_balances
            .entry((to_account, to_asset))
            .or_insert(0.0) += actual_amount;

        // Tax-deferred accounts have ordinary income tax on withdrawal
        let (federal_tax, state_tax) = if is_tax_deferred {
            let tax_config = &state.tax_config;
            let fed = actual_amount * 0.22;
            let st = actual_amount * tax_config.state_rate;
            state.ytd_tax.ordinary_income += actual_amount;
            state.ytd_tax.federal_tax += fed;
            state.ytd_tax.state_tax += st;
            (fed, st)
        } else {
            (0.0, 0.0)
        };

        state.records.push(Record::new(
            state.current_date,
            RecordKind::Transfer {
                from_account_id: src_account,
                from_asset_id: src_asset,
                to_account_id: to_account,
                to_asset_id: to_asset,
                amount: actual_amount,
                event_id,
            },
        ));

        return LiquidationResult {
            gross_amount: actual_amount,
            net_proceeds: actual_amount - federal_tax - state_tax,
            cost_basis: actual_amount,
            short_term_gain: 0.0,
            long_term_gain: 0.0,
            federal_tax,
            state_tax,
        };
    }

    // Taxable account - need cost basis tracking
    let lots = state
        .asset_lots
        .entry((src_account, src_asset))
        .or_insert_with(Vec::new);

    // If no lots exist, create one with current balance (assume cost basis = current value)
    if lots.is_empty() && balance > 0.0 {
        lots.push(crate::simulation_state::AssetLot {
            purchase_date: state.current_date,
            units: balance,
            cost_basis: balance,
        });
    }

    // Sort lots based on method
    match lot_method {
        LotMethod::Fifo => lots.sort_by_key(|l| l.purchase_date),
        LotMethod::Lifo => lots.sort_by(|a, b| b.purchase_date.cmp(&a.purchase_date)),
        LotMethod::HighestCost => lots.sort_by(|a, b| {
            let a_basis = if a.units > 0.0 {
                a.cost_basis / a.units
            } else {
                0.0
            };
            let b_basis = if b.units > 0.0 {
                b.cost_basis / b.units
            } else {
                0.0
            };
            b_basis
                .partial_cmp(&a_basis)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        LotMethod::LowestCost => lots.sort_by(|a, b| {
            let a_basis = if a.units > 0.0 {
                a.cost_basis / a.units
            } else {
                0.0
            };
            let b_basis = if b.units > 0.0 {
                b.cost_basis / b.units
            } else {
                0.0
            };
            a_basis
                .partial_cmp(&b_basis)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        LotMethod::AverageCost => {
            // Average cost handled specially below
        }
    }

    // Consume lots to satisfy the liquidation amount
    let mut remaining = actual_amount;
    let mut total_cost_basis = 0.0;
    let mut short_term_gain = 0.0;
    let mut long_term_gain = 0.0;
    let mut lots_to_remove = Vec::new();

    if lot_method == LotMethod::AverageCost {
        // Average cost: calculate weighted average
        let total_units: f64 = lots.iter().map(|l| l.units).sum();
        let total_basis: f64 = lots.iter().map(|l| l.cost_basis).sum();
        let avg_basis_per_unit = if total_units > 0.0 {
            total_basis / total_units
        } else {
            0.0
        };

        total_cost_basis = remaining * avg_basis_per_unit;
        let gain = remaining - total_cost_basis;
        // For average cost, treat all as long-term for simplicity
        long_term_gain = gain.max(0.0);

        // Remove units proportionally
        for lot in lots.iter_mut() {
            let proportion = lot.units / total_units;
            let remove = remaining * proportion;
            lot.units -= remove;
            lot.cost_basis -= remove * avg_basis_per_unit;
        }
        lots.retain(|l| l.units > 0.001);
    } else {
        // FIFO, LIFO, HighestCost, LowestCost
        for (idx, lot) in lots.iter_mut().enumerate() {
            if remaining <= 0.001 {
                break;
            }

            let take_amount = remaining.min(lot.units);
            let take_fraction = if lot.units > 0.0 {
                take_amount / lot.units
            } else {
                0.0
            };
            let basis_used = lot.cost_basis * take_fraction;
            total_cost_basis += basis_used;

            let gain = take_amount - basis_used;

            // Determine if short-term or long-term (>1 year)
            let holding_days = (state.current_date - lot.purchase_date).get_days();
            if holding_days >= 365 {
                long_term_gain += gain.max(0.0);
            } else {
                short_term_gain += gain.max(0.0);
            }

            lot.units -= take_amount;
            lot.cost_basis -= basis_used;

            if lot.units <= 0.001 {
                lots_to_remove.push(idx);
            }
            remaining -= take_amount;
        }

        // Remove depleted lots (in reverse order)
        for idx in lots_to_remove.iter().rev() {
            lots.remove(*idx);
        }
    }

    // Calculate taxes on gains using config rates
    let tax_config = &state.tax_config;
    let federal_tax = short_term_gain * 0.22 + long_term_gain * tax_config.capital_gains_rate;
    let state_tax = (short_term_gain + long_term_gain) * tax_config.state_rate;
    let net_proceeds = actual_amount - federal_tax - state_tax;

    // Update balances
    *state
        .asset_balances
        .entry((src_account, src_asset))
        .or_insert(0.0) -= actual_amount;
    *state
        .asset_balances
        .entry((to_account, to_asset))
        .or_insert(0.0) += net_proceeds;

    // Track for year-end tax calculation
    state.ytd_tax.capital_gains += short_term_gain + long_term_gain;
    state.ytd_tax.federal_tax += federal_tax;
    state.ytd_tax.state_tax += state_tax;

    // Record liquidation
    state.records.push(Record::new(
        state.current_date,
        RecordKind::Liquidation {
            from_account_id: src_account,
            from_asset_id: src_asset,
            to_account_id: to_account,
            to_asset_id: to_asset,
            gross_amount: actual_amount,
            cost_basis: total_cost_basis,
            short_term_gain,
            long_term_gain,
            federal_tax,
            state_tax,
            net_proceeds,
            lot_method,
            event_id,
        },
    ));

    LiquidationResult {
        gross_amount: actual_amount,
        net_proceeds,
        cost_basis: total_cost_basis,
        short_term_gain,
        long_term_gain,
        federal_tax,
        state_tax,
    }
}

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
    match sources {
        WithdrawalSources::Single {
            account_id,
            asset_id,
        } => vec![(*account_id, *asset_id)],
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

        EventEffect::Sweep {
            to_account,
            to_asset,
            target,
            sources,
            amount_mode,
            lot_method,
        } => {
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

            // For Net mode, we need to withdraw enough gross to end up with target_amount net
            // This is iterative since taxes depend on the amount withdrawn
            let mut total_gross = 0.0;
            let mut total_net = 0.0;

            // Determine target based on mode
            let net_target = match amount_mode {
                WithdrawalAmountMode::Gross => {
                    // For gross mode, we withdraw target_amount gross
                    // The net will be whatever remains after taxes
                    target_amount // This is actually gross target
                }
                WithdrawalAmountMode::Net => target_amount,
            };

            // Calculate how much gross we need to withdraw
            // For Net mode: estimate initial gross needed (will iterate if needed)
            // For Gross mode: gross_needed = target_amount
            let gross_needed = match amount_mode {
                WithdrawalAmountMode::Gross => target_amount,
                WithdrawalAmountMode::Net => {
                    // Estimate: assume ~25% effective tax rate for initial gross-up
                    // We'll adjust as we go
                    estimate_gross_for_net(&source_list, net_target, state)
                }
            };

            let mut remaining_gross = gross_needed;
            let mut remaining_net = net_target;

            // Withdraw from sources in order until target is met
            for (src_account, src_asset) in source_list {
                let done = match amount_mode {
                    WithdrawalAmountMode::Gross => remaining_gross <= 0.001,
                    WithdrawalAmountMode::Net => remaining_net <= 0.001,
                };
                if done {
                    break;
                }

                let available = state.asset_balance(src_account, src_asset);
                if available <= 0.001 {
                    continue;
                }

                // For Net mode, we may need to withdraw more gross to hit net target
                let take_gross = match amount_mode {
                    WithdrawalAmountMode::Gross => remaining_gross.min(available),
                    WithdrawalAmountMode::Net => {
                        // Estimate gross needed for remaining net
                        let estimated =
                            estimate_gross_for_net_single(src_account, remaining_net, state);
                        estimated.min(available)
                    }
                };

                // Liquidate from this source (handles lot tracking, taxes, records)
                let result = liquidate_from_source(
                    src_account,
                    src_asset,
                    *to_account,
                    *to_asset,
                    take_gross,
                    *lot_method,
                    state,
                    event_id,
                );

                total_gross += result.gross_amount;
                total_net += result.net_proceeds;

                remaining_gross -= result.gross_amount;
                remaining_net -= result.net_proceeds;
            }

            // Record sweep summary
            state.records.push(Record::new(
                state.current_date,
                RecordKind::Sweep {
                    to_account_id: *to_account,
                    to_asset_id: *to_asset,
                    target_amount,
                    actual_gross: total_gross,
                    actual_net: total_net,
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

/// Estimate gross withdrawal needed to achieve a target net amount
/// Takes into account the account types in the source list
fn estimate_gross_for_net(
    sources: &[(AccountId, AssetId)],
    net_target: f64,
    state: &SimulationState,
) -> f64 {
    if sources.is_empty() {
        return net_target;
    }

    // Use the first source to estimate effective tax rate
    let (first_account, _) = sources[0];
    estimate_gross_for_net_single(first_account, net_target, state)
}

/// Estimate gross withdrawal from a single account to achieve target net
fn estimate_gross_for_net_single(
    account_id: AccountId,
    net_target: f64,
    state: &SimulationState,
) -> f64 {
    let tax_config = &state.tax_config;

    if let Some(account) = state.accounts.get(&account_id) {
        match account.account_type {
            AccountType::Taxable => {
                // Capital gains: assume mostly long-term for estimation
                let effective_rate = tax_config.capital_gains_rate + tax_config.state_rate;
                net_target / (1.0 - effective_rate)
            }
            AccountType::TaxDeferred => {
                // Ordinary income tax
                let effective_rate = 0.22 + tax_config.state_rate;
                net_target / (1.0 - effective_rate)
            }
            AccountType::TaxFree => {
                // No tax on Roth withdrawals
                net_target
            }
            AccountType::Illiquid => {
                // Typically can't withdraw from illiquid
                net_target
            }
        }
    } else {
        // Unknown account, assume 25% effective rate
        net_target / 0.75
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
