//! Roth conversions: pre-tax money moved into a Roth in kind, taxed as
//! ordinary income in the year it is converted.

use crate::error::{AccountTypeError, LookupError, StateEventError, TransferEvaluationError};
use crate::evaluate::{EvalEvent, evaluate_effect_into};
use crate::expression::EvaluationContext;
use crate::liquidation::get_current_price;
use crate::model::{
    AccountFlavor, AccountId, AmountMode, AssetCoord, AssetLot, CashFlowKind, EventEffect,
    InvestmentContainer, LotMethod, TaxStatus, TransferAmount, TransferEndpoint,
};
use crate::simulation_state::SimulationState;
use crate::taxes::calculate_federal_marginal_tax;

/// What a conversion skipped in an RMD year says.
const RMD_FIRST: &str = "Roth conversion skipped: the account owes this year's RMD, which has \
     not been taken. Take the RMD first: order the RMD event before the conversion, or fire it \
     earlier in the year.";

/// The investment account `id`, if it has the tax status `status`.
fn investment_with(
    state: &SimulationState,
    id: AccountId,
    status: TaxStatus,
) -> Result<&InvestmentContainer, StateEventError> {
    match state.portfolio.accounts.get(&id).map(|a| &a.flavor) {
        Some(AccountFlavor::Investment(inv)) if inv.tax_status == status => Ok(inv),
        Some(_) => Err(AccountTypeError::InvalidAccountType(id).into()),
        None => Err(LookupError::AccountNotFound(id).into()),
    }
}

/// Evaluate a `RothConversion`.
///
/// Pushes, in order: the conversion's record; for a withheld tax, the part
/// kept back as a distribution (the account's cash, then lots sold); the
/// cash moved; each lot moved in kind, oldest first; the income tax on the
/// whole gross and, before 59½, the penalty on the part withheld; and for a
/// tax paid from another account, that account's sale (if its cash is short)
/// and the tax debit.
pub(crate) fn evaluate_roth_conversion_into(
    from: AccountId,
    to: AccountId,
    amount: &TransferAmount,
    pay_tax_from: Option<AccountId>,
    state: &SimulationState,
    out: &mut Vec<EvalEvent>,
) -> Result<(), StateEventError> {
    let source = investment_with(state, from, TaxStatus::TaxDeferred)?;
    investment_with(state, to, TaxStatus::TaxFree)?;
    let payer = match pay_tax_from {
        None => None,
        Some(id) => match state.portfolio.accounts.get(&id).map(|a| &a.flavor) {
            Some(AccountFlavor::Bank(_)) => Some((id, None)),
            Some(AccountFlavor::Investment(inv)) if inv.tax_status == TaxStatus::Taxable => {
                Some((id, Some(inv)))
            }
            Some(_) => return Err(AccountTypeError::InvalidAccountType(id).into()),
            None => return Err(LookupError::AccountNotFound(id).into()),
        },
    };

    // An RMD cannot be converted, and converting first would understate
    // next year's RMD base.
    if state.owes_untaken_rmd(from) {
        out.push(EvalEvent::EffectSkipped {
            account_id: Some(from),
            message: RMD_FIRST,
        });
        return Ok(());
    }

    let requested = amount
        .expression()
        .evaluate(
            &EvaluationContext::new(state)
                .with_endpoints(
                    TransferEndpoint::External,
                    TransferEndpoint::Cash { account_id: to },
                )
                .with_source_account(from),
        )
        .map_err(TransferEvaluationError::Expression)?;
    if requested < 0.01 {
        return Ok(());
    }

    // What the account holds now: its cash, then its lots oldest first.
    let cash = source.cash.value.max(0.0);
    let mut lots: Vec<(AssetLot, f64)> = source
        .positions
        .iter()
        .filter(|lot| lot.units > 0.0)
        .filter_map(|lot| {
            get_current_price(
                &state.portfolio.market,
                state.timeline.start_date,
                state.timeline.current_date,
                lot.asset_id,
            )
            .ok()
            .filter(|price| *price > 0.0)
            .map(|price| (*lot, price))
        })
        .collect();
    lots.sort_by_key(|(lot, _)| lot.purchase_date);
    let holdings: f64 = lots.iter().map(|(lot, price)| lot.units * price).sum();
    let gross = requested.min(cash + holdings);
    if gross < 0.01 {
        return Ok(());
    }

    // The whole gross is ordinary income; no penalty on what converts.
    let config = &state.taxes.config;
    let federal_tax = calculate_federal_marginal_tax(
        gross,
        state.taxes.ytd_tax.ordinary_income,
        &config.federal_brackets,
    );
    let state_tax = gross * config.state_rate;
    let tax = federal_tax + state_tax;

    // Withheld, the tax is kept back as a distribution. Before 59½ that
    // distribution is early and pays the penalty, which is withheld too:
    // `withheld = tax + rate * withheld`.
    let rate = config.early_withdrawal_penalty_rate;
    let penalized = pay_tax_from.is_none() && state.timeline.is_below_early_withdrawal_age();
    let withheld = match (pay_tax_from, penalized) {
        (Some(_), _) => 0.0,
        (None, true) if rate < 1.0 => tax / (1.0 - rate),
        (None, _) => tax,
    }
    .min(gross);

    out.push(EvalEvent::RothConversion {
        from,
        to,
        amount: gross,
        tax,
        withheld,
    });

    // Cash first: the withheld part leaves as a distribution, the rest
    // moves to the Roth's cash.
    let cash_used = gross.min(cash);
    let cash_withheld = withheld.min(cash_used);
    if cash_withheld > 0.0 {
        out.push(EvalEvent::CashWithdrawal {
            from,
            amount: cash_withheld,
        });
    }
    let cash_moved = cash_used - cash_withheld;
    if cash_moved > 0.0 {
        out.push(EvalEvent::CashDebit {
            from,
            net_amount: cash_moved,
            kind: CashFlowKind::Transfer,
        });
        out.push(EvalEvent::CashCredit {
            to,
            net_amount: cash_moved,
            kind: CashFlowKind::Transfer,
        });
    }

    // Then holdings, oldest lot first: sold for what is still to withhold,
    // moved in kind for the rest.
    let mut to_sell = withheld - cash_withheld;
    let mut to_move = gross - cash_used - to_sell;
    for (lot, price) in lots {
        if to_sell + to_move < 0.005 {
            break;
        }
        let coord = AssetCoord {
            account_id: from,
            asset_id: lot.asset_id,
        };
        let mut units_left = lot.units;
        // A share of the lot, by value, with its share of the basis. Whole
        // when `value` covers what is left, so no dust stays behind.
        let take = |units_left: &mut f64, value: f64| -> (f64, f64, f64) {
            let left_value = *units_left * price;
            let (units, value) = if value >= left_value - 1e-6 {
                (*units_left, left_value)
            } else {
                (value / price, value)
            };
            *units_left -= units;
            (units, lot.cost_basis * units / lot.units, value)
        };
        if to_sell > 0.0 {
            let (units, cost_basis, proceeds) = take(&mut units_left, to_sell);
            to_sell -= proceeds;
            out.push(EvalEvent::SubtractAssetLot {
                from: coord,
                lot_date: lot.purchase_date,
                units,
                cost_basis,
                proceeds,
                short_term_gain: 0.0,
                long_term_gain: 0.0,
            });
        }
        if to_move > 0.0 && units_left > 1e-9 {
            let (units, cost_basis, value) = take(&mut units_left, to_move);
            to_move -= value;
            out.push(EvalEvent::MoveAssetLot {
                from: coord,
                to,
                lot_date: lot.purchase_date,
                units,
                cost_basis,
                value,
            });
        }
    }

    out.push(EvalEvent::IncomeTax {
        gross_income_amount: gross,
        federal_tax,
        state_tax,
    });
    if penalized && withheld > 0.0 {
        out.push(EvalEvent::EarlyWithdrawalPenalty {
            gross_amount: withheld,
            penalty_amount: withheld * rate,
            penalty_rate: rate,
        });
    }

    // Paid from another account: its cash, selling its holdings for what
    // the cash does not cover (as a Net sweep would). A bank that runs short
    // is the funding policy's to cover.
    if let Some((payer, investment)) = payer
        && tax > 0.005
    {
        if let Some(investment) = investment {
            let short = tax - investment.cash.value.max(0.0);
            if short > 0.005 {
                let sale = EventEffect::AssetSale {
                    from: payer,
                    asset_id: None,
                    amount: TransferAmount::fixed(short),
                    amount_mode: AmountMode::Net,
                    lot_method: LotMethod::Fifo,
                };
                evaluate_effect_into(&sale, state, out)?;
            }
        }
        out.push(EvalEvent::CashDebit {
            from: payer,
            net_amount: tax,
            kind: CashFlowKind::Tax,
        });
    }

    Ok(())
}
