use crate::model::{
    AccountId, AccountType, AssetId, Event, EventEffect, EventId, EventTrigger, LimitPeriod,
    LotMethod, Record, RecordKind, RmdTable, TaxInfo, TransactionSource, TransferAmount,
    TransferEndpoint, TriggerOffset, WithdrawalAmountMode, WithdrawalOrder, WithdrawalSources,
};
use crate::simulation_state::{AssetLot, SimulationState};
use crate::taxes::{
    calculate_marginal_tax, calculate_realized_gains_tax, calculate_tax_deferred_withdrawal_tax,
    consume_lots, gross_up_for_net_target_with_lots,
};
use jiff::ToSpan;

/// Result of liquidating assets from a single source
#[derive(Debug, Clone, Default)]
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

/// Liquidate assets from a single source with proper lot tracking and tax calculation.
///
/// This function handles:
/// - Taxable accounts: Lot-based cost basis tracking with short/long-term gain distinction
/// - Tax-deferred accounts: Ordinary income taxation
/// - Tax-free accounts: No taxation
///
/// Returns the result of the liquidation including taxes paid.
fn liquidate_from_source(
    src_account: AccountId,
    src_asset: AssetId,
    to_account: AccountId,
    to_asset: AssetId,
    amount: f64,
    lot_method: LotMethod,
    state: &mut SimulationState,
    source: TransactionSource,
) -> LiquidationResult {
    let balance = state.asset_balance(src_account, src_asset);
    let actual_amount = amount.min(balance);

    if actual_amount <= 0.001 {
        return LiquidationResult::default();
    }

    let account = state.accounts.get(&src_account);
    let account_type = account
        .map(|a| a.account_type)
        .unwrap_or(AccountType::Taxable);

    match account_type {
        AccountType::Taxable => liquidate_taxable(
            src_account,
            src_asset,
            to_account,
            to_asset,
            actual_amount,
            lot_method,
            state,
            source,
        ),
        AccountType::TaxDeferred => liquidate_tax_deferred(
            src_account,
            src_asset,
            to_account,
            to_asset,
            actual_amount,
            lot_method,
            state,
            source,
        ),
        AccountType::TaxFree => liquidate_tax_free(
            src_account,
            src_asset,
            to_account,
            to_asset,
            actual_amount,
            state,
            source,
        ),
        AccountType::Illiquid => {
            // Cannot liquidate from illiquid accounts
            eprintln!("Cannot liquidate from illiquid account: {:?}", src_account);
            LiquidationResult::default()
        }
    }
}

/// Liquidate from a taxable account with full lot tracking
fn liquidate_taxable(
    src_account: AccountId,
    src_asset: AssetId,
    to_account: AccountId,
    to_asset: AssetId,
    amount: f64,
    lot_method: LotMethod,
    state: &mut SimulationState,
    source: TransactionSource,
) -> LiquidationResult {
    // Get balance first before borrowing lots
    let balance = state.asset_balance(src_account, src_asset);

    // Get or create lots for this asset
    let lots = state
        .asset_lots
        .entry((src_account, src_asset))
        .or_insert_with(Vec::new);

    // If no lots exist but there's a balance, create a lot with basis = value
    // (This handles legacy data or accounts initialized without cost basis)
    if lots.is_empty() && balance > 0.0 {
        lots.push(AssetLot {
            purchase_date: state.current_date,
            units: balance,
            cost_basis: balance,
        });
    }

    // Consume lots using the unified tax module function
    let lot_result = consume_lots(lots, amount, lot_method, state.current_date);

    // Calculate taxes on the realized gains using unified tax module
    let tax_result = calculate_realized_gains_tax(
        lot_result.short_term_gain,
        lot_result.long_term_gain,
        &state.tax_config,
        state.ytd_tax.ordinary_income,
    );

    let gross_amount = lot_result.amount_consumed;
    let net_proceeds = gross_amount - tax_result.total_tax;

    // Update balances
    *state
        .asset_balances
        .entry((src_account, src_asset))
        .or_insert(0.0) -= gross_amount;
    *state
        .asset_balances
        .entry((to_account, to_asset))
        .or_insert(0.0) += net_proceeds;

    // Track YTD taxes - short-term gains add to ordinary income for bracket purposes
    state.ytd_tax.ordinary_income += lot_result.short_term_gain;
    state.ytd_tax.capital_gains += lot_result.short_term_gain + lot_result.long_term_gain;
    state.ytd_tax.federal_tax += tax_result.federal_tax;
    state.ytd_tax.state_tax += tax_result.state_tax;

    // Record the transfer
    state.records.push(Record::new(
        state.current_date,
        RecordKind::Transfer {
            from_account_id: src_account,
            from_asset_id: src_asset,
            to_account_id: to_account,
            to_asset_id: to_asset,
            gross_amount,
            net_amount: net_proceeds,
            tax_info: Some(Box::new(TaxInfo {
                cost_basis: lot_result.cost_basis,
                short_term_gain: lot_result.short_term_gain,
                long_term_gain: lot_result.long_term_gain,
                federal_tax: tax_result.federal_tax,
                state_tax: tax_result.state_tax,
                lot_method,
            })),
            source: Box::new(source),
        },
    ));

    LiquidationResult {
        gross_amount,
        net_proceeds,
        cost_basis: lot_result.cost_basis,
        short_term_gain: lot_result.short_term_gain,
        long_term_gain: lot_result.long_term_gain,
        federal_tax: tax_result.federal_tax,
        state_tax: tax_result.state_tax,
    }
}

/// Liquidate from a tax-deferred account (Traditional IRA, 401k)
fn liquidate_tax_deferred(
    src_account: AccountId,
    src_asset: AssetId,
    to_account: AccountId,
    to_asset: AssetId,
    amount: f64,
    lot_method: LotMethod,
    state: &mut SimulationState,
    source: TransactionSource,
) -> LiquidationResult {
    // Calculate ordinary income tax using unified tax module
    let tax_result = calculate_tax_deferred_withdrawal_tax(
        amount,
        &state.tax_config,
        state.ytd_tax.ordinary_income,
    );

    let net_amount = tax_result.net_amount;

    // Update balances
    *state
        .asset_balances
        .entry((src_account, src_asset))
        .or_insert(0.0) -= amount;
    *state
        .asset_balances
        .entry((to_account, to_asset))
        .or_insert(0.0) += net_amount;

    // Track YTD taxes
    state.ytd_tax.ordinary_income += amount;
    state.ytd_tax.federal_tax += tax_result.federal_tax;
    state.ytd_tax.state_tax += tax_result.state_tax;

    // Record the transfer
    state.records.push(Record::new(
        state.current_date,
        RecordKind::Transfer {
            from_account_id: src_account,
            from_asset_id: src_asset,
            to_account_id: to_account,
            to_asset_id: to_asset,
            gross_amount: amount,
            net_amount,
            tax_info: Some(Box::new(TaxInfo {
                cost_basis: amount, // Tax-deferred has no meaningful cost basis
                short_term_gain: 0.0,
                long_term_gain: 0.0,
                federal_tax: tax_result.federal_tax,
                state_tax: tax_result.state_tax,
                lot_method,
            })),
            source: Box::new(source),
        },
    ));

    LiquidationResult {
        gross_amount: amount,
        net_proceeds: net_amount,
        cost_basis: amount,
        short_term_gain: 0.0,
        long_term_gain: 0.0,
        federal_tax: tax_result.federal_tax,
        state_tax: tax_result.state_tax,
    }
}

/// Liquidate from a tax-free account (Roth IRA)
fn liquidate_tax_free(
    src_account: AccountId,
    src_asset: AssetId,
    to_account: AccountId,
    to_asset: AssetId,
    amount: f64,
    state: &mut SimulationState,
    source: TransactionSource,
) -> LiquidationResult {
    // No taxes on Roth withdrawals (assuming qualified)
    *state
        .asset_balances
        .entry((src_account, src_asset))
        .or_insert(0.0) -= amount;
    *state
        .asset_balances
        .entry((to_account, to_asset))
        .or_insert(0.0) += amount;

    // Track for reporting (even though tax-free)
    state.ytd_tax.tax_free_withdrawals += amount;

    // Record the transfer (no tax info needed)
    state.records.push(Record::new(
        state.current_date,
        RecordKind::Transfer {
            from_account_id: src_account,
            from_asset_id: src_asset,
            to_account_id: to_account,
            to_asset_id: to_asset,
            gross_amount: amount,
            net_amount: amount,
            tax_info: None,
            source: Box::new(source),
        },
    ));

    LiquidationResult {
        gross_amount: amount,
        net_proceeds: amount,
        cost_basis: amount,
        short_term_gain: 0.0,
        long_term_gain: 0.0,
        federal_tax: 0.0,
        state_tax: 0.0,
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

                // Update accumulators only if we're actually transferring something
                if calculated_amount > 0.0 {
                    match flow_limits.period {
                        LimitPeriod::Yearly => {
                            *state.event_flow_ytd.entry(event_id).or_insert(0.0) +=
                                calculated_amount;
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
            }

            // Execute the transfer
            if calculated_amount <= 0.0 {
                return;
            }

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
                            gross_amount: actual_amount,
                            net_amount: actual_amount,
                            tax_info: None,
                            source: Box::new(TransactionSource::Event(event_id)),
                        },
                    ));
                }

                (TransferEndpoint::External, TransferEndpoint::External) => {
                    // Invalid: external -> external
                    eprintln!("WARNING: Invalid transfer from External to External");
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

            // Track remaining amount needed
            // For Gross mode: remaining gross to withdraw
            // For Net mode: remaining net proceeds needed
            let mut remaining = target_amount;

            // Withdraw from sources in order until target is met
            for (src_account, src_asset) in source_list {
                if remaining < 0.01 {
                    break;
                }

                let available = state.asset_balance(src_account, src_asset);
                if available < 0.01 {
                    continue;
                }

                // Calculate gross amount to withdraw from this source
                let take_gross = match amount_mode {
                    WithdrawalAmountMode::Gross => remaining.min(available),
                    WithdrawalAmountMode::Net => {
                        // Calculate exact gross needed for remaining net using lot data
                        let gross_needed = calculate_gross_for_net(
                            src_account,
                            src_asset,
                            remaining,
                            *lot_method,
                            state,
                        );
                        gross_needed.min(available)
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
                    TransactionSource::Sweep {
                        event_id,
                        target_amount,
                        amount_mode: *amount_mode,
                    },
                );

                // Subtract what we actually got
                remaining -= match amount_mode {
                    WithdrawalAmountMode::Gross => result.gross_amount,
                    WithdrawalAmountMode::Net => result.net_proceeds,
                };
            }

            if remaining > 0.01 {
                eprintln!("Failed to fully meet sweep target: remaining {}", remaining);
            }
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
            // Mark event as inactive (permanently) - keep in map so it doesn't re-activate
            state.repeating_event_active.insert(*event_id, false);
            state.event_next_date.remove(event_id);
            // Mark it as triggered if it was a once event
            state.triggered_events.insert(*event_id, state.current_date);
        }

        // === Event Chaining ===
        EventEffect::TriggerEvent(event_id) => {
            pending_triggers.push(*event_id);
        }

        // === RMD Effects ===
        EventEffect::ApplyRmd {
            to_account,
            to_asset,
            starting_age,
        } => {
            // Check if person has reached RMD age
            let current_age = state.current_age();

            let Some((years, _months)) = current_age else {
                eprintln!("WARNING: Cannot apply RMD - current age unknown");
                return;
            };

            if years < *starting_age {
                return; // Not yet at RMD age
            }

            // Calculate RMD amount using IRS Uniform Lifetime Table
            let rmd_table = RmdTable::irs_uniform_lifetime_2024();

            // Find all tax-deferred accounts eligible for RMD
            let eligible_accounts: Vec<(AccountId, AssetId)> = state
                .accounts
                .iter()
                .filter(|(_, acc)| matches!(acc.account_type, AccountType::TaxDeferred))
                .flat_map(|(id, acc)| acc.assets.iter().map(move |asset| (*id, asset.asset_id)))
                .collect();

            // Process RMD for each eligible account/asset
            for (src_account, src_asset) in eligible_accounts {
                apply_rmd_to_account(
                    src_account,
                    src_asset,
                    *to_account,
                    *to_asset,
                    years,
                    &rmd_table,
                    state,
                    event_id,
                );
            }
        }
    }
}

/// Apply RMD withdrawal to a single tax-deferred account using liquidation logic
fn apply_rmd_to_account(
    src_account: AccountId,
    src_asset: AssetId,
    to_account: AccountId,
    to_asset: AssetId,
    age: u8,
    rmd_table: &RmdTable,
    state: &mut SimulationState,
    event_id: EventId,
) {
    // Get prior year balance and divisor
    let prior_year_balance = state.prior_year_end_balance(src_account);
    let irs_divisor = rmd_table.divisor_for_age(age);

    let (prior_balance, divisor) = match (prior_year_balance, irs_divisor) {
        (Some(pb), Some(d)) => (pb, d),
        _ => {
            eprintln!(
                "WARNING: Cannot apply RMD for account {:?} asset {:?} at age {} - missing prior balance or divisor",
                src_account, src_asset, age
            );
            return;
        }
    };

    if prior_balance <= 0.0 {
        return; // No RMD needed for empty accounts
    }

    let rmd_amount = prior_balance / divisor;
    let balance = state.asset_balance(src_account, src_asset);
    let actual_amount = rmd_amount.min(balance);

    if actual_amount <= 0.0 {
        return; // Nothing to withdraw
    }

    // Use liquidate_from_source for consistent tax handling
    // This handles the withdrawal, taxes, and deposit to destination
    let _result = liquidate_from_source(
        src_account,
        src_asset,
        to_account,
        to_asset,
        actual_amount,
        LotMethod::Fifo, // RMDs use FIFO by default
        state,
        TransactionSource::Rmd {
            event_id,
            age,
            prior_year_balance: prior_balance,
            irs_divisor: divisor,
            required_amount: rmd_amount,
        },
    );
}

/// Calculate the gross withdrawal needed from a single account to achieve a target net amount.
/// For taxable accounts, uses actual lot data for precise calculation.
/// For tax-deferred accounts, uses marginal tax rate calculation.
/// For tax-free accounts, gross equals net.
fn calculate_gross_for_net(
    account_id: AccountId,
    asset_id: AssetId,
    net_target: f64,
    lot_method: LotMethod,
    state: &SimulationState,
) -> f64 {
    let Some(account) = state.accounts.get(&account_id) else {
        // Unknown account, assume 25% effective rate
        eprintln!(
            "WARNING: Unknown account {:?} when calculating gross for net, assuming 25% effective tax rate",
            account_id
        );
        return net_target / 0.75;
    };

    match account.account_type {
        AccountType::Taxable => {
            // Use actual lot data for taxable accounts
            if let Some(lots) = state.asset_lots.get(&(account_id, asset_id)) {
                if !lots.is_empty() {
                    return gross_up_for_net_target_with_lots(
                        net_target,
                        lots,
                        lot_method,
                        state.current_date,
                        &state.tax_config,
                        state.ytd_tax.ordinary_income,
                    );
                }
            }
            // No lots - assume no gain (basis = value), so no tax
            net_target
        }
        AccountType::TaxDeferred => {
            // Binary search for gross amount that yields target net after tax
            let mut low = net_target;
            let mut high = net_target * 2.0;

            for _ in 0..30 {
                let mid = (low + high) / 2.0;
                let tax = calculate_marginal_tax(
                    mid,
                    state.ytd_tax.ordinary_income,
                    &state.tax_config.federal_brackets,
                ) + mid * state.tax_config.state_rate;
                let net = mid - tax;

                if (net - net_target).abs() < 0.01 {
                    return mid;
                }
                if net < net_target {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            (low + high) / 2.0
        }
        AccountType::TaxFree => {
            // No tax, gross equals net
            net_target
        }
        AccountType::Illiquid => {
            // Can't withdraw from illiquid, return target as-is
            net_target
        }
    }
}

/// Process all events that should trigger on the current date
/// Returns list of (EventId, event name) for logging
/// Helper to determine event priority (lower = higher priority = processed first)
/// Control events (Pause/Resume/Terminate) should be processed before other events
fn event_priority(event: &Event) -> u8 {
    // Check if any effect is a control effect
    for effect in &event.effects {
        match effect {
            EventEffect::PauseEvent(_)
            | EventEffect::ResumeEvent(_)
            | EventEffect::TerminateEvent(_) => return 0, // Highest priority
            _ => {}
        }
    }
    1 // Normal priority
}

pub fn process_events(state: &mut SimulationState) -> Vec<EventId> {
    let mut triggered = Vec::new();
    let mut pending_triggers: Vec<EventId> = Vec::new();

    // Collect events to evaluate (avoid borrow issues)
    let mut events_to_check: Vec<(EventId, Event)> = state
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

    // Sort events so control events (Pause/Resume/Terminate) are processed first
    events_to_check.sort_by_key(|(_, event)| event_priority(event));

    // Evaluate each event
    for (event_id, event) in events_to_check {
        let should_trigger = match &event.trigger {
            EventTrigger::Repeating {
                interval,
                start_condition,
                end_condition,
            } => {
                // Check if this repeating event has been started and its active status
                let active_status = state.repeating_event_active.get(&event_id);
                let is_started = active_status.is_some(); // Event has been activated at least once
                let is_active = active_status.copied().unwrap_or(false);

                // Check if end_condition is met - if so, terminate the event
                if let Some(end_cond) = end_condition {
                    if evaluate_trigger(end_cond, state) {
                        state.repeating_event_active.remove(&event_id);
                        state.event_next_date.remove(&event_id);
                        continue; // Skip this event
                    }
                }

                if !is_started {
                    // Event hasn't been started yet - check if start_condition is met
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
                } else if !is_active {
                    // Event was started but is now paused - don't trigger
                    false
                } else {
                    // Active event - check if scheduled for today
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
