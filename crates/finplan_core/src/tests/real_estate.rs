//! Real estate: amortizing loans, `BuyProperty`, `SellProperty`, and carrying
//! costs written as an expression over the property's value.

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AssetId, Cash, CashFlowKind, Event, EventEffect, EventId,
    EventTrigger, Financing, FixedAsset, InflationProfile, LoanDetail, Repayment, RepeatInterval,
    ReturnProfile, ReturnProfileId, StateEvent, TaxConfig, TransferAmount, amortized_payment,
};
use crate::simulation::simulate;

const CASH: AccountId = AccountId(1);
const HOUSE: AccountId = AccountId(2);
const LOAN: AccountId = AccountId(3);
const HOUSE_ASSET: AssetId = AssetId(1);
const FLAT: ReturnProfileId = ReturnProfileId(0);
const HOUSE_PROFILE: ReturnProfileId = ReturnProfileId(1);

fn cash(value: f64) -> Account {
    Account {
        account_id: CASH,
        flavor: AccountFlavor::Bank(Cash {
            value,
            return_profile_id: FLAT,
        }),
    }
}

fn house(value: f64) -> Account {
    Account {
        account_id: HOUSE,
        flavor: AccountFlavor::Property(FixedAsset {
            asset_id: HOUSE_ASSET,
            value,
            cost_basis: None,
        }),
    }
}

fn loan(principal: f64, rate: f64, repayment: Option<Repayment>) -> Account {
    Account {
        account_id: LOAN,
        flavor: AccountFlavor::Liability(LoanDetail {
            principal,
            interest_rate: rate,
            repayment,
            schedule: None,
        }),
    }
}

fn config(
    house_return: f64,
    years: usize,
    accounts: Vec<Account>,
    events: Vec<Event>,
) -> SimulationConfig {
    SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: years,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([
            (FLAT, ReturnProfile::None),
            (HOUSE_PROFILE, ReturnProfile::Fixed(house_return)),
        ]),
        asset_returns: HashMap::from([(HOUSE_ASSET, HOUSE_PROFILE)]),
        asset_prices: HashMap::from([(HOUSE_ASSET, 100.0)]),
        accounts,
        events,
        ..Default::default()
    }
}

fn once(id: u16, date: jiff::civil::Date, effect: EventEffect) -> Event {
    Event {
        event_id: EventId(id),
        trigger: EventTrigger::Date(date),
        effects: vec![effect],
        once: true,
    }
}

fn loan_debits(result: &crate::model::SimulationResult) -> Vec<f64> {
    result
        .ledger
        .iter()
        .filter_map(|e| match e.event {
            StateEvent::CashDebit {
                from: CASH,
                amount,
                kind: CashFlowKind::Expense,
            } if e.source_event.is_none() => Some(amount),
            _ => None,
        })
        .collect()
}

/// A loan the plan opens with pays itself off over its term: 120 level
/// payments from the cash account, and nothing owed after.
#[test]
fn opening_loan_amortizes_to_zero_over_its_term() {
    let principal = 300_000.0;
    let rate = 0.05;
    let params = config(
        0.0,
        11,
        vec![
            cash(1_000_000.0),
            loan(
                principal,
                rate,
                Some(Repayment {
                    from: CASH,
                    term_months: 120,
                }),
            ),
        ],
        vec![],
    );

    let result = simulate(&params, 1).unwrap();
    let payments = loan_debits(&result);
    let expected = amortized_payment(principal, rate, 120);

    assert!(
        (payments.len() as i64 - 120).abs() <= 1,
        "expected ~120 payments, got {}",
        payments.len()
    );
    assert!((payments[0] - expected).abs() < 0.01);
    let owed = -result.final_account_balance(LOAN).unwrap();
    assert!(owed < 1.0, "loan should be paid off, still owes ${owed:.2}");
}

/// Buying with a mortgage moves the down payment as a transfer (not an
/// expense), puts the price on the property, draws the loan for the rest, and
/// starts level payments the month after.
#[test]
fn buy_property_with_financing() {
    let price = 1_200_000.0;
    let down = 200_000.0;
    let bought = jiff::civil::date(2026, 6, 1);
    let params = config(
        0.0,
        3,
        vec![cash(600_000.0), house(0.0), loan(0.0, 0.06, None)],
        vec![once(
            0,
            bought,
            EventEffect::BuyProperty {
                property: HOUSE,
                price: TransferAmount::fixed(price),
                from: CASH,
                financing: Some(Financing {
                    loan: LOAN,
                    down_payment: TransferAmount::fixed(down),
                    term_months: 360,
                }),
            },
        )],
    );

    let result = simulate(&params, 1).unwrap();

    let down_payment = result.ledger.iter().find_map(|e| match e.event {
        StateEvent::CashDebit {
            from: CASH,
            amount,
            kind,
        } if e.date == bought => Some((amount, kind)),
        _ => None,
    });
    assert_eq!(down_payment, Some((down, CashFlowKind::Transfer)));

    let house_value = result.final_account_balance(HOUSE).unwrap();
    assert!(
        (house_value - price).abs() < 1.0,
        "house worth ${house_value:.2}"
    );

    let payments = loan_debits(&result);
    let expected = amortized_payment(price - down, 0.06, 360);
    assert!(!payments.is_empty(), "the loan should be making payments");
    assert!(
        (payments[0] - expected).abs() < 0.01,
        "payment ${:.2}",
        payments[0]
    );
    // Funded 2026-06-01: July 2026 through December 2027, before the plan
    // ends on 2028-01-01.
    assert_eq!(payments.len(), 18);
}

/// Selling pays the loan off first, takes selling costs and capital-gains tax
/// on the gain over basis less the exclusion, and leaves the rest in cash.
#[test]
fn sell_property_pays_off_loan_and_taxes_gain() {
    let opening = 400_000.0;
    let growth = 0.05;
    let sold = jiff::civil::date(2030, 1, 1);
    let tax_config = TaxConfig {
        capital_gains_rate: 0.15,
        state_rate: 0.05,
        ..TaxConfig::default()
    };
    let mut params = config(
        growth,
        6,
        vec![cash(0.0), house(opening), loan(100_000.0, 0.0, None)],
        vec![once(
            0,
            sold,
            EventEffect::SellProperty {
                property: HOUSE,
                to: CASH,
                selling_cost_rate: 0.06,
                gain_exclusion: 50_000.0,
                payoff: Some(LOAN),
            },
        )],
    );
    params.tax_config = tax_config;

    let result = simulate(&params, 1).unwrap();

    let value = opening * (1.0 + growth).powi(5);
    let proceeds = value * 0.94;
    let gain = proceeds - opening - 50_000.0;
    let tax = gain * 0.20;
    let expected_cash = proceeds - tax - 100_000.0;

    let final_cash = result.final_account_balance(CASH).unwrap();
    assert!(
        (final_cash - expected_cash).abs() / expected_cash < 0.001,
        "cash expected ${expected_cash:.2}, got ${final_cash:.2}"
    );
    assert!(result.final_account_balance(HOUSE).unwrap().abs() < 0.01);
    assert!(result.final_account_balance(LOAN).unwrap().abs() < 0.01);
}

/// Carrying costs need no effect of their own: a monthly expense of
/// `0.001 * balance(House)` is 1.2% a year of whatever the house is worth.
#[test]
fn carrying_cost_expression_reads_property_value() {
    let opening = 500_000.0;
    let params = config(
        0.0,
        1,
        vec![cash(100_000.0), house(opening)],
        vec![Event {
            event_id: EventId(0),
            trigger: EventTrigger::Repeating {
                interval: RepeatInterval::Monthly,
                start_condition: None,
                end_condition: None,
                max_occurrences: None,
            },
            effects: vec![EventEffect::Expense {
                from: CASH,
                amount: TransferAmount::scaled(0.001, TransferAmount::account_balance(HOUSE)),
            }],
            once: false,
        }],
    );

    let result = simulate(&params, 1).unwrap();
    let first = result
        .ledger
        .iter()
        .find_map(|e| match e.event {
            StateEvent::CashDebit { amount, .. } => Some(amount),
            _ => None,
        })
        .unwrap();
    assert!((first - 500.0).abs() < 0.01, "first month ${first:.2}");
}
