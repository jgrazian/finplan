use jiff::ToSpan;
use jiff::civil::Date;

use crate::error::{StateEventError, TransferEvaluationError, TriggerEventError};
use crate::liquidation::{LiquidationParams, get_current_price, liquidate_investment};
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetCoord, AssetId, EventEffect, EventId,
    EventTrigger, IncomeType, RmdTable, StateEvent, TaxStatus, TransferAmount, TransferEndpoint,
    TriggerOffset, WithdrawalOrder, WithdrawalSources,
};
use crate::simulation_state::SimulationState;
use crate::taxes::{calculate_federal_marginal_tax, calculate_gross_from_net};

/// Evaluate a TransferAmount expression to get the actual dollar amount
fn evaluate_transfer_amount(
    amount: &TransferAmount,
    from: &TransferEndpoint,
    to: &TransferEndpoint,
    state: &SimulationState,
) -> Result<f64, TransferEvaluationError> {
    match amount {
        TransferAmount::Fixed(amt) => Ok(*amt),

        TransferAmount::SourceBalance => match from {
            TransferEndpoint::Asset { asset_coord } => Ok(state.asset_balance(*asset_coord)?),
            TransferEndpoint::Cash { account_id } => Ok(state.account_cash_balance(*account_id)?),
            TransferEndpoint::External => Err(TransferEvaluationError::ExternalBalanceReference), // External has no balance
        },

        TransferAmount::ZeroTargetBalance => match to {
            TransferEndpoint::Asset { asset_coord } => Ok(state.asset_balance(*asset_coord)?),
            TransferEndpoint::Cash { account_id } => Ok(state.account_cash_balance(*account_id)?),
            TransferEndpoint::External => Err(TransferEvaluationError::ExternalBalanceReference), // External has no balance
        },

        TransferAmount::TargetToBalance(target) => match to {
            TransferEndpoint::Asset { asset_coord } => Ok(state
                .asset_balance(*asset_coord)
                .map(|current| (target - current).max(0.0))?),
            TransferEndpoint::Cash { account_id } => Ok(state
                .account_cash_balance(*account_id)
                .map(|current| (target - current).max(0.0))?),
            TransferEndpoint::External => Err(TransferEvaluationError::ExternalBalanceReference), // External has no balance
        },

        TransferAmount::AssetBalance { asset_coord } => Ok(state.asset_balance(*asset_coord)?),

        TransferAmount::AccountTotalBalance { account_id } => {
            Ok(state.account_balance(*account_id)?)
        }

        TransferAmount::AccountCashBalance { account_id } => {
            Ok(state.account_cash_balance(*account_id)?)
        }

        TransferAmount::Min(left, right) => {
            let left_val = evaluate_transfer_amount(left, from, to, state)?;
            let right_val = evaluate_transfer_amount(right, from, to, state)?;
            Ok(left_val.min(right_val))
        }

        TransferAmount::Max(left, right) => {
            let left_val = evaluate_transfer_amount(left, from, to, state)?;
            let right_val = evaluate_transfer_amount(right, from, to, state)?;
            Ok(left_val.max(right_val))
        }

        TransferAmount::Sub(left, right) => {
            let left_val = evaluate_transfer_amount(left, from, to, state)?;
            let right_val = evaluate_transfer_amount(right, from, to, state)?;
            Ok(left_val - right_val)
        }

        TransferAmount::Add(left, right) => {
            let left_val = evaluate_transfer_amount(left, from, to, state)?;
            let right_val = evaluate_transfer_amount(right, from, to, state)?;
            Ok(left_val + right_val)
        }

        TransferAmount::Mul(left, right) => {
            let left_val = evaluate_transfer_amount(left, from, to, state)?;
            let right_val = evaluate_transfer_amount(right, from, to, state)?;
            Ok(left_val * right_val)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TriggerEvent {
    Triggered,
    NotTriggered,
    NextTriggerDate(Date),
    StartRepeating(Date),
    StopRepeating,
    TriggerRepeating(Date),
}

/// Evaluates whether a trigger condition is met
pub fn evaluate_trigger(
    event_id: &EventId,
    trigger: &EventTrigger,
    state: &SimulationState,
) -> Result<TriggerEvent, TriggerEventError> {
    match trigger {
        EventTrigger::Date(date) => Ok(if state.timeline.current_date >= *date {
            TriggerEvent::Triggered
        } else {
            TriggerEvent::NextTriggerDate(*date)
        }),

        EventTrigger::Age { years, months } => {
            let (current_years, current_months) = state.current_age();
            let target_months = months.unwrap_or(0);

            let remaining_years = *years as i16 - current_years as i16;
            let remaining_months = target_months as i16 - current_months as i16;

            if remaining_years <= 0 && remaining_months <= 0 {
                Ok(TriggerEvent::Triggered)
            } else {
                let trigger_date = state
                    .timeline
                    .current_date
                    .checked_add(remaining_years.years().months(remaining_months))?;

                Ok(TriggerEvent::NextTriggerDate(trigger_date))
            }
        }

        EventTrigger::RelativeToEvent { event_id, offset } => {
            if let Some(trigger_date) = state.event_state.triggered_events.get(event_id) {
                let target_date = match offset {
                    TriggerOffset::Days(d) => trigger_date.checked_add((*d as i64).days()),
                    TriggerOffset::Months(m) => trigger_date.checked_add((*m as i64).months()),
                    TriggerOffset::Years(y) => trigger_date.checked_add((*y as i64).years()),
                }?;

                if state.timeline.current_date >= target_date {
                    Ok(TriggerEvent::Triggered)
                } else {
                    Ok(TriggerEvent::NextTriggerDate(target_date))
                }
            } else {
                Ok(TriggerEvent::NotTriggered)
            }
        }

        EventTrigger::AccountBalance {
            account_id,
            threshold,
        } => {
            let balance = state.account_balance(*account_id)?;
            if threshold.evaluate(balance) {
                Ok(TriggerEvent::Triggered)
            } else {
                Ok(TriggerEvent::NotTriggered)
            }
        }

        EventTrigger::AssetBalance {
            asset_coord,
            threshold,
        } => {
            let balance = state.asset_balance(*asset_coord)?;
            if threshold.evaluate(balance) {
                Ok(TriggerEvent::Triggered)
            } else {
                Ok(TriggerEvent::NotTriggered)
            }
        }

        EventTrigger::NetWorth { threshold } => {
            let net_worth = state.net_worth();
            if threshold.evaluate(net_worth) {
                Ok(TriggerEvent::Triggered)
            } else {
                Ok(TriggerEvent::NotTriggered)
            }
        }

        EventTrigger::And(triggers) => {
            let results: Vec<bool> = triggers
                .iter()
                .map(|t| {
                    evaluate_trigger(event_id, t, state)
                        .map(|eval| matches!(eval, TriggerEvent::Triggered))
                })
                .collect::<Result<Vec<bool>, _>>()?;
            Ok(if results.into_iter().all(|b| b) {
                TriggerEvent::Triggered
            } else {
                TriggerEvent::NotTriggered
            })
        }

        EventTrigger::Or(triggers) => {
            let results: Vec<bool> = triggers
                .iter()
                .map(|t| {
                    evaluate_trigger(event_id, t, state)
                        .map(|eval| matches!(eval, TriggerEvent::Triggered))
                })
                .collect::<Result<Vec<bool>, _>>()?;
            Ok(if results.into_iter().any(|b| b) {
                TriggerEvent::Triggered
            } else {
                TriggerEvent::NotTriggered
            })
        }

        EventTrigger::Repeating {
            interval,
            start_condition,
            end_condition,
        } => {
            // Check if this repeating event has been started and its active status
            let active_status = state.event_state.repeating_event_active.get(event_id);
            let is_started = active_status.is_some(); // Event has been activated at least once
            let is_active = active_status.copied().unwrap_or(false);

            // Check if end_condition is met - if so, terminate the event
            if let Some(end_cond) = end_condition
                && let TriggerEvent::Triggered = evaluate_trigger(event_id, end_cond, state)?
            {
                return Ok(TriggerEvent::StopRepeating);
            }

            if !is_started {
                // Event hasn't been started yet - check if start_condition is met
                let (condition_met, next_try_date) = match start_condition {
                    None => (true, None),
                    Some(condition) => match evaluate_trigger(event_id, condition, state)? {
                        TriggerEvent::Triggered => (true, None),
                        TriggerEvent::NextTriggerDate(date) => (false, Some(date)),
                        TriggerEvent::NotTriggered => (false, None),
                        _ => (false, None),
                    },
                };

                if condition_met {
                    Ok(TriggerEvent::StartRepeating(
                        state.timeline.current_date.saturating_add(interval.span()),
                    ))
                } else {
                    Ok(if let Some(date) = next_try_date {
                        TriggerEvent::NextTriggerDate(date)
                    } else {
                        TriggerEvent::NotTriggered
                    })
                }
            } else if !is_active {
                // Event was started but is now paused - don't trigger
                Ok(TriggerEvent::NotTriggered)
            } else {
                // Active event - check if scheduled for today
                if let Some(next_date) = state.event_state.event_next_date.get(event_id) {
                    if state.timeline.current_date >= *next_date {
                        // Schedule next occurrence
                        let next = next_date.saturating_add(interval.span());
                        Ok(TriggerEvent::TriggerRepeating(next))
                    } else {
                        Ok(TriggerEvent::NotTriggered)
                    }
                } else {
                    Ok(TriggerEvent::NotTriggered)
                }
            }
        }

        EventTrigger::Manual => Ok(TriggerEvent::NotTriggered), // Only triggered via TriggerEvent effect
    }
}

/// Internal state mutations during event evaluation.
/// These are converted to LedgerEntry/StateEvent when recorded.
#[derive(Debug, Clone)]
pub enum EvalEvent {
    // === Account Management ===
    CreateAccount(Account),
    DeleteAccount(AccountId),

    CashCredit {
        to: AccountId,
        net_amount: f64,
    },

    CashDebit {
        from: AccountId,
        net_amount: f64,
    },

    RecordContribution {
        account_id: AccountId,
        amount: f64,
    },

    IncomeTax {
        gross_income_amount: f64,
        federal_tax: f64,
        state_tax: f64,
    },

    ShortTermCapitalGainsTax {
        gross_gain_amount: f64,
        federal_tax: f64,
        state_tax: f64,
    },

    LongTermCapitalGainsTax {
        gross_gain_amount: f64,
        federal_tax: f64,
        state_tax: f64,
    },

    AddAssetLot {
        to: AssetCoord,
        units: f64,
        cost_basis: f64,
    },

    SubtractAssetLot {
        from: AssetCoord,
        lot_date: Date,
        units: f64,
        cost_basis: f64,
        proceeds: f64,
        short_term_gain: f64,
        long_term_gain: f64,
    },

    // === Event Management ===
    TriggerEvent(EventId),
    PauseEvent(EventId),
    ResumeEvent(EventId),
    TerminateEvent(EventId),

    StateEvent(StateEvent),
}

/// Apply a single effect to the simulation state
pub fn evaluate_effect(
    effect: &EventEffect,
    state: &SimulationState,
) -> Result<Vec<EvalEvent>, StateEventError> {
    match effect {
        EventEffect::CreateAccount(account) => Ok(vec![EvalEvent::CreateAccount(account.clone())]),
        EventEffect::DeleteAccount(account_id) => Ok(vec![EvalEvent::DeleteAccount(*account_id)]),
        EventEffect::Income {
            to,
            amount,
            amount_mode,
            income_type,
        } => {
            let calculated_amount = evaluate_transfer_amount(
                amount,
                &TransferEndpoint::External,
                &TransferEndpoint::Cash { account_id: *to },
                state,
            )?;

            // Check contribution limits if depositing to investment account
            let allowed_amount = if let Some(room) = state.contribution_room(*to)? {
                calculated_amount.min(room)
            } else {
                calculated_amount
            };

            // If contribution limit blocks the income, skip it
            if allowed_amount < 0.01 {
                return Ok(vec![]);
            }

            let mut effects = vec![];

            // Track contribution if to an investment account
            if state.contribution_room(*to)?.is_some() {
                effects.push(EvalEvent::RecordContribution {
                    account_id: *to,
                    amount: allowed_amount,
                });
            }

            match (income_type, amount_mode) {
                (IncomeType::TaxFree, _) => {
                    effects.push(EvalEvent::CashCredit {
                        to: *to,
                        net_amount: allowed_amount,
                    });
                    Ok(effects)
                }
                (IncomeType::Taxable, AmountMode::Gross) => {
                    let ytd_income = state.taxes.ytd_tax.ordinary_income;
                    let brackets = &state.taxes.config.federal_brackets;
                    let state_rate = state.taxes.config.state_rate;

                    let federal_tax =
                        calculate_federal_marginal_tax(allowed_amount, ytd_income, brackets);
                    let state_tax = allowed_amount * state_rate;
                    let net_amount = allowed_amount - federal_tax - state_tax;

                    effects.extend(vec![
                        EvalEvent::CashCredit {
                            to: *to,
                            net_amount,
                        },
                        EvalEvent::IncomeTax {
                            gross_income_amount: allowed_amount,
                            federal_tax,
                            state_tax,
                        },
                    ]);
                    Ok(effects)
                }
                (IncomeType::Taxable, AmountMode::Net) => {
                    let ytd_income = state.taxes.ytd_tax.ordinary_income;
                    let brackets = &state.taxes.config.federal_brackets;
                    let state_rate = state.taxes.config.state_rate;

                    // Calculate gross from the net amount received
                    let gross_amount =
                        calculate_gross_from_net(allowed_amount, ytd_income, brackets, state_rate);

                    let federal_tax =
                        calculate_federal_marginal_tax(gross_amount, ytd_income, brackets);
                    let state_tax = gross_amount * state_rate;

                    effects.extend(vec![
                        EvalEvent::CashCredit {
                            to: *to,
                            net_amount: allowed_amount,
                        },
                        EvalEvent::IncomeTax {
                            gross_income_amount: gross_amount,
                            federal_tax,
                            state_tax,
                        },
                    ]);
                    Ok(effects)
                }
            }
        }
        EventEffect::Expense { from, amount } => {
            let calculated_amount = evaluate_transfer_amount(
                amount,
                &TransferEndpoint::Cash { account_id: *from },
                &TransferEndpoint::External,
                state,
            )?;

            Ok(vec![EvalEvent::CashDebit {
                from: *from,
                net_amount: calculated_amount,
            }])
        }
        EventEffect::AssetPurchase { from, to, amount } => {
            let calculated_amount = evaluate_transfer_amount(
                amount,
                &TransferEndpoint::Cash { account_id: *from },
                &TransferEndpoint::Asset { asset_coord: *to },
                state,
            )?;

            // Only check contribution limits if money is coming from a different account
            // (i.e., this represents new money entering the investment account)
            let is_cross_account = *from != to.account_id;

            let allowed_amount = if is_cross_account {
                if let Some(room) = state.contribution_room(to.account_id)? {
                    calculated_amount.min(room)
                } else {
                    calculated_amount
                }
            } else {
                calculated_amount
            };

            // If contribution limit blocks the purchase, skip it
            if allowed_amount < 0.01 {
                return Ok(vec![]);
            }

            let mut effects = vec![EvalEvent::CashDebit {
                from: *from,
                net_amount: allowed_amount,
            }];

            // Track contribution only if this is cross-account (new money entering)
            if is_cross_account && state.contribution_room(to.account_id)?.is_some() {
                effects.push(EvalEvent::RecordContribution {
                    account_id: to.account_id,
                    amount: allowed_amount,
                });
            }

            effects.push(EvalEvent::AddAssetLot {
                to: *to,
                units: allowed_amount / state.current_asset_price(*to)?,
                cost_basis: allowed_amount,
            });

            Ok(effects)
        }
        EventEffect::AssetSale {
            to,
            amount,
            sources,
            amount_mode,
            lot_method,
        } => {
            let target_amount = evaluate_transfer_amount(
                amount,
                &TransferEndpoint::External,
                &TransferEndpoint::Cash { account_id: *to },
                state,
            )?;

            if target_amount <= 0.0 {
                return Ok(vec![]);
            }

            // Resolve withdrawal sources to (account_id, asset_id, investment_container) tuples
            let source_list = resolve_withdrawal_sources_with_containers(sources, state);

            // Track remaining amount needed
            // For Gross mode: remaining gross to withdraw
            // For Net mode: remaining net proceeds needed
            let mut remaining = target_amount;

            let mut all_effects = vec![];

            // Withdraw from sources in order until target is met
            for (account_id, asset_id, investment) in source_list {
                if remaining < 0.01 {
                    break;
                }

                let asset_coord = AssetCoord {
                    account_id,
                    asset_id,
                };

                // Get current price from Market
                let current_price = match get_current_price(
                    &state.portfolio.market,
                    state.timeline.start_date,
                    state.timeline.current_date,
                    asset_id,
                ) {
                    Some(price) => price,
                    None => continue, // Skip if no price available
                };

                // Calculate available value at current price
                let units: f64 = investment
                    .positions
                    .iter()
                    .filter(|lot| lot.asset_id == asset_id)
                    .map(|lot| lot.units)
                    .sum();
                let available = units * current_price;

                if available < 0.01 {
                    continue;
                }

                // Calculate gross amount to withdraw from this source
                let take_gross = match amount_mode {
                    AmountMode::Gross => remaining.min(available),
                    AmountMode::Net => {
                        // For net mode, estimate gross needed based on actual position data
                        // Calculate average cost basis per unit for this asset
                        let total_units: f64 = investment
                            .positions
                            .iter()
                            .filter(|lot| lot.asset_id == asset_id)
                            .map(|lot| lot.units)
                            .sum();
                        let total_basis: f64 = investment
                            .positions
                            .iter()
                            .filter(|lot| lot.asset_id == asset_id)
                            .map(|lot| lot.cost_basis)
                            .sum();

                        let avg_cost_per_unit = if total_units > 0.0 {
                            total_basis / total_units
                        } else {
                            current_price // No gain if no basis info
                        };

                        // Estimate gain ratio: (current_price - avg_cost) / current_price
                        let gain_ratio =
                            ((current_price - avg_cost_per_unit) / current_price).max(0.0);

                        // Estimate tax rate on gains (use capital gains + state rate)
                        let estimated_tax_rate =
                            state.taxes.config.capital_gains_rate + state.taxes.config.state_rate;

                        // For each dollar of gross proceeds:
                        // - gain_ratio of it is taxable gain
                        // - (1 - gain_ratio) is return of basis (not taxed)
                        // - tax = gain_ratio * estimated_tax_rate
                        // So net = gross * (1 - gain_ratio * estimated_tax_rate)
                        // Therefore gross = net / (1 - gain_ratio * estimated_tax_rate)
                        let effective_tax_rate = gain_ratio * estimated_tax_rate;
                        let gross_multiplier = 1.0 / (1.0 - effective_tax_rate).max(0.5);

                        let estimated_gross = remaining * gross_multiplier;
                        estimated_gross.min(available)
                    }
                };

                // Liquidate from this source (handles lot tracking, taxes, records)
                let (result, effects) = liquidate_investment(&LiquidationParams {
                    investment,
                    asset_coord,
                    to_account: *to,
                    amount: take_gross,
                    current_price,
                    lot_method: *lot_method,
                    current_date: state.timeline.current_date,
                    tax_config: &state.taxes.config,
                    ytd_ordinary_income: state.taxes.ytd_tax.ordinary_income,
                });

                // Subtract what we actually got
                remaining -= match amount_mode {
                    AmountMode::Gross => result.gross_amount,
                    AmountMode::Net => result.net_proceeds,
                };

                all_effects.extend(effects);
            }

            Ok(all_effects)
        }
        EventEffect::ApplyRmd {
            destination,
            lot_method,
        } => {
            let rmd_table = RmdTable::irs_uniform_lifetime_2024();

            let (age, _) = state.current_age();
            let Some(rmd_divisor) = rmd_table.divisor_for_age(age) else {
                // TODO: Better handling for ages beyond table
                return Ok(vec![]); // No RMD required for this age
            };

            let mut effects = vec![];
            for acc in state.portfolio.accounts.values() {
                // Only process Investment accounts with Tax-Deferred status
                let _investment = match &acc.flavor {
                    AccountFlavor::Investment(inv) if inv.tax_status == TaxStatus::TaxDeferred => {
                        inv
                    }
                    _ => continue,
                };

                // Calculate required RMD amount
                let Some(prior_balance) = state.prior_year_end_balance(acc.account_id) else {
                    continue;
                };
                let required_value = prior_balance / rmd_divisor;

                // Liquidate required amount
                let sale = EventEffect::AssetSale {
                    to: *destination,
                    amount: TransferAmount::Fixed(required_value),
                    sources: WithdrawalSources::SingleAccount(acc.account_id),
                    amount_mode: AmountMode::Gross,
                    lot_method: *lot_method,
                };

                let results = evaluate_effect(&sale, state)?;

                // Push a RMD marker and then the actual transaction effects
                effects.push(EvalEvent::StateEvent(StateEvent::RmdWithdrawal {
                    account_id: acc.account_id,
                    age,
                    prior_year_balance: prior_balance,
                    divisor: rmd_divisor,
                    required_amount: required_value,
                    actual_amount: results.iter().fold(0.0, |sum, ev| match ev {
                        EvalEvent::CashCredit { net_amount, .. } => sum + *net_amount,
                        _ => sum,
                    }),
                }));
                effects.extend(results);
            }

            Ok(effects)
        }
        EventEffect::TriggerEvent(event_id) => Ok(vec![EvalEvent::TriggerEvent(*event_id)]),
        EventEffect::PauseEvent(event_id) => Ok(vec![EvalEvent::PauseEvent(*event_id)]),
        EventEffect::ResumeEvent(event_id) => Ok(vec![EvalEvent::ResumeEvent(*event_id)]),
        EventEffect::TerminateEvent(event_id) => Ok(vec![EvalEvent::TerminateEvent(*event_id)]),
    }
}

/// Resolve withdrawal sources based on strategy or custom list
/// Only Investment accounts (with InvestmentContainer) are considered for withdrawals
pub fn resolve_withdrawal_sources(
    sources: &WithdrawalSources,
    state: &SimulationState,
) -> Vec<AssetCoord> {
    match sources {
        WithdrawalSources::SingleAsset(asset_coord) => vec![*asset_coord],
        WithdrawalSources::SingleAccount(account_id) => {
            // Get all positions from this account if it's an Investment account
            if let Some(account) = state.portfolio.accounts.get(account_id)
                && let AccountFlavor::Investment(inv) = &account.flavor
            {
                inv.positions
                    .iter()
                    .map(|lot| AssetCoord {
                        account_id: *account_id,
                        asset_id: lot.asset_id,
                    })
                    .collect()
            } else {
                vec![]
            }
        }
        WithdrawalSources::Custom(list) => list.clone(),
        WithdrawalSources::Strategy {
            order,
            exclude_accounts,
        } => {
            // Filter to only Investment accounts (the only ones with positions to sell)
            let mut investment_accounts: Vec<_> = state
                .portfolio
                .accounts
                .iter()
                .filter(|(id, _)| !exclude_accounts.contains(id))
                .filter_map(|(id, acc)| {
                    if let AccountFlavor::Investment(inv) = &acc.flavor {
                        Some((id, acc, inv))
                    } else {
                        None
                    }
                })
                .collect();

            // Sort by tax status according to the withdrawal strategy
            match order {
                WithdrawalOrder::TaxEfficientEarly => {
                    investment_accounts.sort_by_key(|(_, _, inv)| match inv.tax_status {
                        TaxStatus::Taxable => 0,
                        TaxStatus::TaxDeferred => 1,
                        TaxStatus::TaxFree => 2,
                    });
                }
                WithdrawalOrder::TaxDeferredFirst => {
                    investment_accounts.sort_by_key(|(_, _, inv)| match inv.tax_status {
                        TaxStatus::TaxDeferred => 0,
                        TaxStatus::Taxable => 1,
                        TaxStatus::TaxFree => 2,
                    });
                }
                WithdrawalOrder::TaxFreeFirst => {
                    investment_accounts.sort_by_key(|(_, _, inv)| match inv.tax_status {
                        TaxStatus::TaxFree => 0,
                        TaxStatus::Taxable => 1,
                        TaxStatus::TaxDeferred => 2,
                    });
                }
                WithdrawalOrder::ProRata => {
                    // Pro-rata: return all accounts (proportional withdrawal handled in caller)
                }
            }

            // Flatten to AssetCoord pairs from positions in each investment account
            investment_accounts
                .iter()
                .flat_map(|(_acc_id, acc, inv)| {
                    inv.positions
                        .iter()
                        .map(|lot| AssetCoord {
                            account_id: acc.account_id,
                            asset_id: lot.asset_id,
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        }
    }
}
use crate::model::InvestmentContainer;

/// Resolve withdrawal sources with their InvestmentContainers for direct liquidation
/// Returns tuples of (AccountId, AssetId, &InvestmentContainer)
fn resolve_withdrawal_sources_with_containers<'a>(
    sources: &WithdrawalSources,
    state: &'a SimulationState,
) -> Vec<(AccountId, AssetId, &'a InvestmentContainer)> {
    match sources {
        WithdrawalSources::SingleAsset(asset_coord) => {
            // Find the investment container for this asset_coord
            if let Some(account) = state.portfolio.accounts.get(&asset_coord.account_id)
                && let AccountFlavor::Investment(inv) = &account.flavor
            {
                return vec![(asset_coord.account_id, asset_coord.asset_id, inv)];
            }
            vec![]
        }
        WithdrawalSources::SingleAccount(account_id) => {
            // Get all positions from this account if it's an Investment account
            if let Some(account) = state.portfolio.accounts.get(account_id)
                && let AccountFlavor::Investment(inv) = &account.flavor
            {
                return inv
                    .positions
                    .iter()
                    .map(|lot| (*account_id, lot.asset_id, inv))
                    .collect();
            }
            vec![]
        }
        WithdrawalSources::Custom(list) => list
            .iter()
            .filter_map(|coord| {
                state
                    .portfolio
                    .accounts
                    .get(&coord.account_id)
                    .and_then(|acc| {
                        if let AccountFlavor::Investment(inv) = &acc.flavor {
                            Some((coord.account_id, coord.asset_id, inv))
                        } else {
                            None
                        }
                    })
            })
            .collect(),
        WithdrawalSources::Strategy {
            order,
            exclude_accounts,
        } => {
            // Filter to only Investment accounts
            let mut investment_accounts: Vec<_> = state
                .portfolio
                .accounts
                .iter()
                .filter(|(id, _)| !exclude_accounts.contains(id))
                .filter_map(|(_, acc)| {
                    if let AccountFlavor::Investment(inv) = &acc.flavor {
                        Some((acc.account_id, inv))
                    } else {
                        None
                    }
                })
                .collect();

            // Sort by tax status according to the withdrawal strategy
            match order {
                WithdrawalOrder::TaxEfficientEarly => {
                    investment_accounts.sort_by_key(|(_, inv)| match inv.tax_status {
                        TaxStatus::Taxable => 0,
                        TaxStatus::TaxDeferred => 1,
                        TaxStatus::TaxFree => 2,
                    });
                }
                WithdrawalOrder::TaxDeferredFirst => {
                    investment_accounts.sort_by_key(|(_, inv)| match inv.tax_status {
                        TaxStatus::TaxDeferred => 0,
                        TaxStatus::Taxable => 1,
                        TaxStatus::TaxFree => 2,
                    });
                }
                WithdrawalOrder::TaxFreeFirst => {
                    investment_accounts.sort_by_key(|(_, inv)| match inv.tax_status {
                        TaxStatus::TaxFree => 0,
                        TaxStatus::Taxable => 1,
                        TaxStatus::TaxDeferred => 2,
                    });
                }
                WithdrawalOrder::ProRata => {
                    // Pro-rata: return all accounts (proportional withdrawal handled in caller)
                }
            }

            // Flatten to (AccountId, AssetId, InvestmentContainer) tuples
            investment_accounts
                .into_iter()
                .flat_map(|(account_id, inv)| {
                    // Deduplicate asset_ids in positions
                    let asset_ids: std::collections::HashSet<_> =
                        inv.positions.iter().map(|lot| lot.asset_id).collect();

                    asset_ids
                        .into_iter()
                        .map(move |asset_id| (account_id, asset_id, inv))
                })
                .collect()
        }
    }
}
