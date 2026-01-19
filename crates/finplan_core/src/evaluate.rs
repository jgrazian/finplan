use jiff::ToSpan;
use jiff::civil::Date;

use crate::error::{EngineError, StateEventError, TransferEvaluationError, TriggerEventError};
use crate::liquidation::{LiquidationParams, get_current_price, liquidate_investment};
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetCoord, AssetId, CashFlowKind, EventEffect,
    EventId, EventTrigger, IncomeType, RmdTable, StateEvent, TaxStatus, TransferAmount,
    TransferEndpoint, TriggerOffset, WithdrawalOrder, WithdrawalSources,
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
    // Check if this event has been permanently terminated
    if state.event_state.terminated_events.contains(event_id) {
        return Ok(TriggerEvent::NotTriggered);
    }

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
        kind: CashFlowKind,
    },

    CashDebit {
        from: AccountId,
        net_amount: f64,
        kind: CashFlowKind,
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

    EarlyWithdrawalPenalty {
        gross_amount: f64,
        penalty_amount: f64,
        penalty_rate: f64,
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

    // === Balance Operations ===
    AdjustBalance {
        account: AccountId,
        delta: f64, // Positive = increase, negative = decrease
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
                        kind: CashFlowKind::Income,
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
                            kind: CashFlowKind::Income,
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
                            kind: CashFlowKind::Income,
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
                kind: CashFlowKind::Expense,
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

            // Determine the appropriate kind based on whether this is cross-account
            let debit_kind = if is_cross_account {
                CashFlowKind::Contribution
            } else {
                CashFlowKind::InvestmentPurchase
            };

            let mut effects = vec![EvalEvent::CashDebit {
                from: *from,
                net_amount: allowed_amount,
                kind: debit_kind,
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
            from,
            asset_id,
            amount,
            amount_mode,
            lot_method,
        } => {
            let target_amount = evaluate_transfer_amount(
                amount,
                &TransferEndpoint::External,
                &TransferEndpoint::External,
                state,
            )?;

            if target_amount <= 0.0 {
                return Ok(vec![]);
            }

            // Get the investment account
            let account = state
                .portfolio
                .accounts
                .get(from)
                .ok_or(EngineError::AccountNotFound(*from))?;

            let investment = match &account.flavor {
                AccountFlavor::Investment(inv) => inv,
                _ => return Err(EngineError::NotAnInvestmentAccount(*from).into()),
            };

            // Get assets to liquidate (specific asset or all assets in account)
            let assets_to_liquidate: Vec<AssetId> = match asset_id {
                Some(id) => vec![*id],
                None => investment
                    .positions
                    .iter()
                    .map(|lot| lot.asset_id)
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect(),
            };

            let mut remaining = target_amount;
            let mut all_effects = vec![];

            // Liquidate assets in order until target is met
            for asset_id in assets_to_liquidate {
                if remaining < 0.01 {
                    break;
                }

                let asset_coord = AssetCoord {
                    account_id: *from,
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

                // Calculate gross amount to liquidate
                let take_gross = match amount_mode {
                    AmountMode::Gross => remaining.min(available),
                    AmountMode::Net => {
                        // For net mode, estimate gross needed based on actual position data
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
                            current_price
                        };

                        let gain_ratio =
                            ((current_price - avg_cost_per_unit) / current_price).max(0.0);
                        let estimated_tax_rate =
                            state.taxes.config.capital_gains_rate + state.taxes.config.state_rate;
                        let effective_tax_rate = gain_ratio * estimated_tax_rate;
                        let gross_multiplier = 1.0 / (1.0 - effective_tax_rate).max(0.5);

                        let estimated_gross = remaining * gross_multiplier;
                        estimated_gross.min(available)
                    }
                };

                // Liquidate into the source account's cash balance
                let (result, effects) = liquidate_investment(&LiquidationParams {
                    investment,
                    asset_coord,
                    to_account: *from, // Cash stays in source account
                    amount: take_gross,
                    current_price,
                    lot_method: *lot_method,
                    current_date: state.timeline.current_date,
                    tax_config: &state.taxes.config,
                    ytd_ordinary_income: state.taxes.ytd_tax.ordinary_income,
                    early_withdrawal_penalty_applies: state.timeline.is_below_early_withdrawal_age(),
                });

                remaining -= match amount_mode {
                    AmountMode::Gross => result.gross_amount,
                    AmountMode::Net => result.net_proceeds,
                };

                all_effects.extend(effects);
            }

            Ok(all_effects)
        }
        EventEffect::Sweep {
            sources,
            to,
            amount,
            amount_mode,
            lot_method,
            income_type: _, // No longer used - taxation happens during liquidation
        } => {
            // Step 1: Determine source account(s) to liquidate from
            let source_accounts: Vec<AccountId> = match sources {
                WithdrawalSources::SingleAsset(coord) => vec![coord.account_id],
                WithdrawalSources::SingleAccount(id) => vec![*id],
                WithdrawalSources::Custom(list) => list
                    .iter()
                    .map(|coord| coord.account_id)
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect(),
                WithdrawalSources::Strategy {
                    exclude_accounts, ..
                } => state
                    .portfolio
                    .accounts
                    .iter()
                    .filter(|(id, _)| !exclude_accounts.contains(id))
                    .filter_map(|(_, acc)| {
                        if matches!(acc.flavor, AccountFlavor::Investment(_)) {
                            Some(acc.account_id)
                        } else {
                            None
                        }
                    })
                    .collect(),
            };

            let mut all_effects = vec![];
            let mut total_liquidated = 0.0;
            let mut remaining = evaluate_transfer_amount(
                amount,
                &TransferEndpoint::External,
                &TransferEndpoint::External,
                state,
            )?;

            // Step 2: Liquidate from source accounts until target is met
            for from_account in source_accounts {
                if remaining < 0.01 {
                    break;
                }

                let liquidation_effects = evaluate_effect(
                    &EventEffect::AssetSale {
                        from: from_account,
                        asset_id: None, // Liquidate all assets in account
                        amount: TransferAmount::Fixed(remaining),
                        amount_mode: *amount_mode,
                        lot_method: *lot_method,
                    },
                    state,
                )?;

                // Sum up what was liquidated from this account
                let liquidated_from_account: f64 = liquidation_effects
                    .iter()
                    .filter_map(|ev| {
                        if let EvalEvent::CashCredit { net_amount, .. } = ev {
                            Some(*net_amount)
                        } else {
                            None
                        }
                    })
                    .sum();

                total_liquidated += liquidated_from_account;
                remaining -= liquidated_from_account;
                all_effects.extend(liquidation_effects);
            }

            // Step 3: Transfer the liquidated cash to destination
            // AssetSale credits cash to the source investment accounts. If the destination
            // is different, we need to transfer that cash to the destination account.
            if total_liquidated > 0.01 {
                // Build map of source account -> amount credited during liquidation
                let source_amounts: std::collections::HashMap<AccountId, f64> = all_effects
                    .iter()
                    .filter_map(|ev| {
                        if let EvalEvent::CashCredit { to, net_amount, .. } = ev {
                            Some((*to, *net_amount))
                        } else {
                            None
                        }
                    })
                    .fold(std::collections::HashMap::new(), |mut acc, (id, amount)| {
                        *acc.entry(id).or_insert(0.0) += amount;
                        acc
                    });

                // If destination is one of the source accounts, we're done
                // Otherwise, transfer cash from source accounts to destination
                if !source_amounts.contains_key(to) && !source_amounts.is_empty() {
                    // Debit from each source account what was credited to it
                    for (source_account, amount) in &source_amounts {
                        all_effects.push(EvalEvent::CashDebit {
                            from: *source_account,
                            net_amount: *amount,
                            kind: CashFlowKind::Transfer,
                        });
                    }

                    // Credit the destination with the total liquidated amount
                    all_effects.push(EvalEvent::CashCredit {
                        to: *to,
                        net_amount: total_liquidated,
                        kind: CashFlowKind::Transfer,
                    });
                }
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

                // Liquidate and transfer required amount using Sweep
                let sweep = EventEffect::Sweep {
                    sources: WithdrawalSources::SingleAccount(acc.account_id),
                    to: *destination,
                    amount: TransferAmount::Fixed(required_value),
                    amount_mode: AmountMode::Gross,
                    lot_method: *lot_method,
                    income_type: IncomeType::Taxable, // RMDs are taxable income
                };

                let results = evaluate_effect(&sweep, state)?;

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

        EventEffect::AdjustBalance { account, amount } => {
            let delta = evaluate_transfer_amount(
                amount,
                &TransferEndpoint::External,
                &TransferEndpoint::External,
                state,
            )?;

            Ok(vec![EvalEvent::AdjustBalance {
                account: *account,
                delta,
            }])
        }

        EventEffect::CashTransfer { from, to, amount } => {
            let transfer_amount = evaluate_transfer_amount(
                amount,
                &TransferEndpoint::Cash { account_id: *from },
                &TransferEndpoint::External,
                state,
            )?;

            if transfer_amount < 0.01 {
                return Ok(vec![]);
            }

            // Check destination account type
            let dest_account = state
                .portfolio
                .accounts
                .get(to)
                .ok_or(EngineError::AccountNotFound(*to))?;

            match &dest_account.flavor {
                // If destination is a liability, this is a loan payment
                AccountFlavor::Liability(_) => Ok(vec![
                    EvalEvent::CashDebit {
                        from: *from,
                        net_amount: transfer_amount,
                        kind: CashFlowKind::Expense,
                    },
                    EvalEvent::AdjustBalance {
                        account: *to,
                        delta: -transfer_amount, // Negative = reduce debt
                    },
                ]),
                // Otherwise, regular cash transfer
                _ => Ok(vec![
                    EvalEvent::CashDebit {
                        from: *from,
                        net_amount: transfer_amount,
                        kind: CashFlowKind::Transfer,
                    },
                    EvalEvent::CashCredit {
                        to: *to,
                        net_amount: transfer_amount,
                        kind: CashFlowKind::Transfer,
                    },
                ]),
            }
        }
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
                WithdrawalOrder::PenaltyAware => {
                    // Before 59.5: Taxable → TaxFree → TaxDeferred (avoid 10% penalty)
                    // After 59.5: Same as TaxEfficientEarly
                    if state.timeline.is_below_early_withdrawal_age() {
                        investment_accounts.sort_by_key(|(_, _, inv)| match inv.tax_status {
                            TaxStatus::Taxable => 0,
                            TaxStatus::TaxFree => 1,
                            TaxStatus::TaxDeferred => 2, // Last to avoid penalty
                        });
                    } else {
                        // After 59.5, use TaxEfficientEarly order
                        investment_accounts.sort_by_key(|(_, _, inv)| match inv.tax_status {
                            TaxStatus::Taxable => 0,
                            TaxStatus::TaxDeferred => 1,
                            TaxStatus::TaxFree => 2,
                        });
                    }
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
