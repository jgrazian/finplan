//! Roth conversions: pre-tax money moved into a Roth in kind, taxed as
//! ordinary income; the RMD-year guard; and the five-year rule on
//! conversions withdrawn before 59½.
//!
//! What the five-year rule models: each conversion is a tranche on the Roth;
//! a withdrawal before 59½ consumes tranches oldest first (contributions are
//! not modeled, so none come first) and pays the early-withdrawal penalty on
//! the part drawn from a tranche fewer than five tax years old. Earnings,
//! which in law come out last and before 59½ are taxed and penalized, are
//! not: a withdrawal beyond the tranches is still tax- and penalty-free.

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetId, AssetLot, Cash, Event, EventEffect,
    EventId, EventTrigger, IncomeType, InflationProfile, InvestmentContainer, LotMethod,
    RepeatInterval, ReturnProfile, ReturnProfileId, SimulationResult, StateEvent, TaxStatus,
    TransferAmount, WarningKind, WithdrawalSources,
};
use crate::simulation::simulate;
use crate::taxes::calculate_federal_marginal_tax;

const ROTH: AccountId = AccountId(1);
const IRA: AccountId = AccountId(2);
const BROKERAGE: AccountId = AccountId(3);
const CHECKING: AccountId = AccountId(4);
const FUND: AssetId = AssetId(1);
const PRICE: f64 = 50.0;

fn lot(year: i16, units: f64) -> AssetLot {
    AssetLot {
        asset_id: FUND,
        purchase_date: jiff::civil::date(year, 1, 1),
        units,
        cost_basis: units * PRICE / 2.0,
    }
}

fn investment(
    account_id: AccountId,
    tax_status: TaxStatus,
    cash: f64,
    lots: Vec<AssetLot>,
) -> Account {
    Account {
        account_id,
        flavor: AccountFlavor::Investment(InvestmentContainer {
            tax_status,
            cash: Cash {
                value: cash,
                return_profile_id: ReturnProfileId(0),
            },
            positions: lots,
            contribution_limit: None,
        }),
    }
}

/// Flat prices at $50 a unit, no inflation, a 15k standard deduction. The
/// IRA holds `ira_cash` and two lots of 1,000 units ($50k each, the 2015 one
/// older); the Roth `roth_lots`; the brokerage 2,000 units; checking $50k.
fn plan(
    birth_year: i16,
    years: usize,
    ira_cash: f64,
    roth_lots: Vec<AssetLot>,
    events: Vec<Event>,
) -> SimulationConfig {
    let mut config = SimulationConfig {
        start_date: Some(jiff::civil::date(2030, 1, 1)),
        duration_years: years,
        birth_date: Some(jiff::civil::date(birth_year, 1, 1)),
        inflation_profile: InflationProfile::Fixed(0.0),
        return_profiles: HashMap::from([(ReturnProfileId(0), ReturnProfile::Fixed(0.0))]),
        asset_returns: HashMap::from([(FUND, ReturnProfileId(0))]),
        asset_prices: HashMap::from([(FUND, PRICE)]),
        accounts: vec![
            investment(ROTH, TaxStatus::TaxFree, 0.0, roth_lots),
            investment(
                IRA,
                TaxStatus::TaxDeferred,
                ira_cash,
                vec![lot(2020, 1_000.0), lot(2015, 1_000.0)],
            ),
            investment(BROKERAGE, TaxStatus::Taxable, 0.0, vec![lot(2018, 2_000.0)]),
            Account {
                account_id: CHECKING,
                flavor: AccountFlavor::Bank(Cash {
                    value: 50_000.0,
                    return_profile_id: ReturnProfileId(0),
                }),
            },
        ],
        events,
        ..Default::default()
    };
    config.tax_config.standard_deduction = 15_000.0;
    config
}

fn on(id: u16, date: jiff::civil::Date, effect: EventEffect) -> Event {
    Event {
        event_id: EventId(id),
        trigger: EventTrigger::Date(date),
        effects: vec![effect],
        once: true,
    }
}

fn yearly(id: u16, month: i8, day: i8, effect: EventEffect) -> Event {
    Event {
        event_id: EventId(id),
        trigger: EventTrigger::Repeating {
            interval: RepeatInterval::Yearly,
            start_condition: Some(Box::new(EventTrigger::Date(jiff::civil::date(
                2030, month, day,
            )))),
            end_condition: None,
            max_occurrences: None,
        },
        effects: vec![effect],
        once: false,
    }
}

fn convert(amount: TransferAmount, pay_tax_from: Option<AccountId>) -> EventEffect {
    EventEffect::RothConversion {
        from: IRA,
        to: ROTH,
        amount,
        pay_tax_from,
    }
}

/// Year-end, as the template schedules it: Dec 30, since year-end balances
/// (the next RMD's base) are captured as Dec 31 begins.
fn year_end() -> jiff::civil::Date {
    jiff::civil::date(2030, 12, 30)
}

/// The `(amount, tax, withheld)` of every conversion, with its year.
fn conversions(result: &SimulationResult) -> Vec<(i16, f64, f64, f64)> {
    result
        .ledger
        .iter()
        .filter_map(|e| match e.event {
            StateEvent::RothConversion {
                amount,
                tax,
                withheld,
                ..
            } => Some((e.date.year(), amount, tax, withheld)),
            _ => None,
        })
        .collect()
}

fn penalties(result: &SimulationResult) -> Vec<(f64, f64)> {
    result
        .ledger
        .iter()
        .filter_map(|e| match e.event {
            StateEvent::EarlyWithdrawalPenalty {
                gross_amount,
                penalty_amount,
                ..
            } => Some((gross_amount, penalty_amount)),
            _ => None,
        })
        .collect()
}

fn final_value(result: &SimulationResult, account: AccountId) -> f64 {
    result
        .wealth_snapshots
        .last()
        .unwrap()
        .accounts
        .iter()
        .find(|a| a.account_id == account)
        .unwrap()
        .total_value()
}

fn ordinary_income(result: &SimulationResult, year: i16) -> f64 {
    result
        .yearly_taxes
        .iter()
        .find(|t| t.year == year)
        .map_or(0.0, |t| t.ordinary_income)
}

#[test]
fn a_year_end_bracket_room_conversion_fills_income_to_the_ceiling() {
    let config = plan(
        1965,
        1,
        0.0,
        vec![],
        vec![
            on(
                1,
                jiff::civil::date(2030, 3, 1),
                EventEffect::Income {
                    to: CHECKING,
                    amount: TransferAmount::fixed(30_000.0),
                    amount_mode: AmountMode::Gross,
                    income_type: IncomeType::Taxable,
                },
            ),
            on(
                2,
                year_end(),
                convert(TransferAmount::bracket_room(0.22), Some(CHECKING)),
            ),
        ],
    );
    let result = simulate(&config, 1).unwrap();

    // The 22% bracket ends at 100,525 of taxable income: 115,525 gross with
    // the 15k deduction folded in.
    let ceiling = 100_525.0 + 15_000.0;
    assert!((ordinary_income(&result, 2030) - ceiling).abs() < 0.01);
    let [(_, amount, _, withheld)] = conversions(&result)[..] else {
        panic!("one conversion: {:?}", conversions(&result));
    };
    assert!((amount - (ceiling - 30_000.0)).abs() < 0.01);
    assert_eq!(withheld, 0.0);
}

#[test]
fn a_conversion_moves_cash_first_then_lots_in_kind_oldest_first() {
    let config = plan(
        1965,
        1,
        10_000.0,
        vec![],
        vec![on(
            1,
            year_end(),
            convert(TransferAmount::fixed(75_000.0), Some(CHECKING)),
        )],
    );
    let result = simulate(&config, 1).unwrap();

    let moves: Vec<(AccountId, AccountId, i16, f64, f64)> = result
        .ledger
        .iter()
        .filter_map(|e| match e.event {
            StateEvent::AssetLotMoved {
                from,
                to,
                lot_date,
                units,
                value,
                ..
            } => Some((from, to, lot_date.year(), units, value)),
            _ => None,
        })
        .collect();
    // $10k of cash, then the whole 2015 lot, then 300 units of the 2020 one.
    assert_eq!(moves.len(), 2);
    assert_eq!((moves[0].0, moves[0].1, moves[0].2), (IRA, ROTH, 2015));
    assert!((moves[0].3 - 1_000.0).abs() < 1e-9);
    assert_eq!(moves[1].2, 2020);
    assert!((moves[1].3 - 300.0).abs() < 1e-6);
    assert!((moves[0].4 + moves[1].4 - 65_000.0).abs() < 0.01);

    // Nothing was sold or withdrawn: no sale, no cash withdrawal, and the
    // cash-flow summary counts no withdrawal.
    assert!(!result.ledger.iter().any(|e| matches!(
        e.event,
        StateEvent::AssetSale { .. } | StateEvent::CashWithdrawal { .. }
    )));
    assert!(
        result
            .yearly_cash_flows
            .iter()
            .all(|y| y.withdrawals == 0.0)
    );

    assert!((final_value(&result, ROTH) - 75_000.0).abs() < 0.01);
    assert!((final_value(&result, IRA) - 35_000.0).abs() < 0.01);
}

#[test]
fn a_conversion_is_capped_at_what_the_account_holds() {
    let config = plan(
        1965,
        1,
        0.0,
        vec![],
        vec![on(
            1,
            year_end(),
            convert(TransferAmount::fixed(1_000_000.0), Some(CHECKING)),
        )],
    );
    let result = simulate(&config, 1).unwrap();
    assert!((conversions(&result)[0].1 - 100_000.0).abs() < 0.01);
    assert!(final_value(&result, IRA).abs() < 0.01);
    assert!((final_value(&result, ROTH) - 100_000.0).abs() < 0.01);
}

#[test]
fn tax_paid_from_another_account_leaves_the_conversion_whole() {
    // Born 1980: 50, so a withheld tax would be penalized.
    let config = plan(
        1980,
        1,
        0.0,
        vec![],
        vec![on(
            1,
            year_end(),
            convert(TransferAmount::fixed(40_000.0), Some(CHECKING)),
        )],
    );
    let result = simulate(&config, 1).unwrap();

    let expected_tax = calculate_federal_marginal_tax(
        40_000.0,
        0.0,
        &crate::model::TaxConfig::brackets_with_deduction(
            &config.tax_config.federal_brackets,
            15_000.0,
        ),
    ) + 40_000.0 * config.tax_config.state_rate;
    let [(_, amount, tax, withheld)] = conversions(&result)[..] else {
        panic!("one conversion");
    };
    assert_eq!((amount, withheld), (40_000.0, 0.0));
    assert!((tax - expected_tax).abs() < 0.01);
    assert!(penalties(&result).is_empty());
    assert!((final_value(&result, ROTH) - 40_000.0).abs() < 0.01);
    assert!((final_value(&result, CHECKING) - (50_000.0 - expected_tax)).abs() < 0.01);
}

#[test]
fn tax_paid_from_a_brokerage_sells_its_holdings_when_its_cash_is_short() {
    let config = plan(
        1965,
        1,
        0.0,
        vec![],
        vec![on(
            1,
            year_end(),
            convert(TransferAmount::fixed(40_000.0), Some(BROKERAGE)),
        )],
    );
    let result = simulate(&config, 1).unwrap();
    let tax = conversions(&result)[0].2;
    assert!(result.ledger.iter().any(
        |e| matches!(e.event, StateEvent::AssetSale { account_id, .. } if account_id == BROKERAGE)
    ));
    assert!((final_value(&result, ROTH) - 40_000.0).abs() < 0.01);
    // The brokerage paid the tax, plus the gains tax on what it sold.
    assert!(final_value(&result, BROKERAGE) < 100_000.0 - tax + 0.01);
    assert!(final_value(&result, BROKERAGE) > 100_000.0 - 2.0 * tax);
}

#[test]
fn withholding_before_59_pays_the_penalty_on_the_withheld_part() {
    let config = plan(
        1980,
        1,
        0.0,
        vec![],
        vec![on(
            1,
            year_end(),
            convert(TransferAmount::fixed(40_000.0), None),
        )],
    );
    let result = simulate(&config, 1).unwrap();

    let [(_, amount, tax, withheld)] = conversions(&result)[..] else {
        panic!("one conversion");
    };
    assert_eq!(amount, 40_000.0);
    // The withheld part pays the tax and the penalty on itself.
    assert!((withheld - tax / 0.9).abs() < 0.01);
    let [(penalized, penalty)] = penalties(&result)[..] else {
        panic!("one penalty: {:?}", penalties(&result));
    };
    assert!((penalized - withheld).abs() < 0.01);
    assert!((penalty - 0.1 * withheld).abs() < 0.01);
    assert!((withheld - tax - penalty).abs() < 0.01);
    assert!((final_value(&result, ROTH) - (40_000.0 - withheld)).abs() < 0.01);
    assert!((final_value(&result, IRA) - 60_000.0).abs() < 0.01);
    // Checking paid nothing.
    assert!((final_value(&result, CHECKING) - 50_000.0).abs() < 0.01);
}

#[test]
fn withholding_after_59_withholds_only_the_tax() {
    let config = plan(
        1965,
        1,
        0.0,
        vec![],
        vec![on(
            1,
            year_end(),
            convert(TransferAmount::fixed(40_000.0), None),
        )],
    );
    let result = simulate(&config, 1).unwrap();
    let (_, _, tax, withheld) = conversions(&result)[0];
    assert!((withheld - tax).abs() < 1e-9);
    assert!(penalties(&result).is_empty());
    assert!((final_value(&result, ROTH) - (40_000.0 - tax)).abs() < 0.01);
}

fn rmd() -> EventEffect {
    EventEffect::ApplyRmd {
        destination: CHECKING,
        lot_method: LotMethod::Fifo,
    }
}

/// The base of the 2031 RMD (the IRA's balance at the end of 2030), with
/// $40k converted on `day` of December 2030, or nothing converted.
fn rmd_base_after_converting_on(day: Option<i8>) -> f64 {
    // Born 1955: 75. The run has no 2029 balance, so 2030 owes no RMD.
    let mut events = vec![on(1, jiff::civil::date(2031, 6, 1), rmd())];
    if let Some(day) = day {
        events.push(on(
            2,
            jiff::civil::date(2030, 12, day),
            convert(TransferAmount::fixed(40_000.0), Some(CHECKING)),
        ));
    }
    let result = simulate(&plan(1955, 2, 0.0, vec![], events), 1).unwrap();
    result
        .ledger
        .iter()
        .find_map(|e| match e.event {
            StateEvent::RmdWithdrawal {
                account_id,
                prior_year_balance,
                ..
            } if account_id == IRA => Some(prior_year_balance),
            _ => None,
        })
        .expect("the 2031 RMD")
}

#[test]
fn a_dec_30_conversion_comes_out_of_next_years_rmd_base() {
    let none = rmd_base_after_converting_on(None);
    assert!((none - 100_000.0).abs() < 0.01);
    let dec_30 = rmd_base_after_converting_on(Some(30));
    assert!((dec_30 - 60_000.0).abs() < 0.01, "{dec_30}");
    // The year-end balance is taken once Dec 31's own events have run, so a
    // Dec 31 conversion is out of the base too.
    let dec_31 = rmd_base_after_converting_on(Some(31));
    assert!((dec_31 - 60_000.0).abs() < 0.01, "{dec_31}");
}

#[test]
fn an_rmd_year_skips_the_conversion_until_the_rmd_is_taken() {
    // Born 1955: 75 throughout, so an RMD is due every year with a prior
    // year-end balance (from 2031; the run has none for 2029).
    let conversion = convert(TransferAmount::fixed(10_000.0), Some(CHECKING));

    // Converting before the RMD: 2030 converts, 2031 and 2032 are skipped.
    let before = simulate(
        &plan(
            1955,
            3,
            0.0,
            vec![],
            vec![
                yearly(1, 12, 1, conversion.clone()),
                yearly(2, 12, 15, rmd()),
            ],
        ),
        1,
    )
    .unwrap();
    let years: Vec<i16> = conversions(&before).iter().map(|c| c.0).collect();
    assert_eq!(years, vec![2030]);
    let skipped: Vec<_> = before
        .warnings
        .iter()
        .filter(|w| w.kind == WarningKind::EffectSkipped)
        .collect();
    assert_eq!(skipped.len(), 2);
    assert!(skipped.iter().all(|w| w.account_id == Some(IRA)
        && w.event_id == Some(EventId(1))
        && w.message.contains("RMD")));

    // The RMD first: every year converts.
    let after = simulate(
        &plan(
            1955,
            3,
            0.0,
            vec![],
            vec![yearly(1, 12, 15, conversion), yearly(2, 12, 1, rmd())],
        ),
        1,
    )
    .unwrap();
    let years: Vec<i16> = conversions(&after).iter().map(|c| c.0).collect();
    assert_eq!(years, vec![2030, 2031, 2032]);
    assert!(after.warnings.is_empty(), "{:?}", after.warnings);
}

/// Born 1980 (50 in 2030): convert $20k mid-2030, then withdraw `amount`
/// from the Roth on June 1 of `year`. The Roth starts with `roth_lots`.
fn ladder(year: i16, amount: f64, roth_lots: Vec<AssetLot>) -> SimulationResult {
    let config = plan(
        1980,
        (year - 2030 + 1) as usize,
        0.0,
        roth_lots,
        vec![
            on(
                1,
                jiff::civil::date(2030, 6, 1),
                convert(TransferAmount::fixed(20_000.0), Some(CHECKING)),
            ),
            on(
                2,
                jiff::civil::date(year, 6, 1),
                EventEffect::Sweep {
                    sources: WithdrawalSources::SingleAccount(ROTH),
                    to: CHECKING,
                    amount: TransferAmount::fixed(amount),
                    amount_mode: AmountMode::Gross,
                    lot_method: LotMethod::Fifo,
                    income_type: IncomeType::TaxFree,
                },
            ),
        ],
    );
    simulate(&config, 1).unwrap()
}

#[test]
fn a_conversion_withdrawn_inside_five_years_before_59_pays_the_penalty() {
    let result = ladder(2033, 10_000.0, vec![]);
    let [(penalized, penalty)] = penalties(&result)[..] else {
        panic!("one penalty: {:?}", penalties(&result));
    };
    assert!((penalized - 10_000.0).abs() < 0.01);
    assert!((penalty - 1_000.0).abs() < 0.01);
}

#[test]
fn a_conversion_withdrawn_after_five_years_pays_no_penalty() {
    // Converted in 2030: from January 1, 2035 it is five years old.
    let result = ladder(2035, 10_000.0, vec![]);
    assert!(penalties(&result).is_empty());
    let result = ladder(2034, 10_000.0, vec![]);
    assert_eq!(penalties(&result).len(), 1);
}

#[test]
fn only_the_conversions_share_of_a_withdrawal_is_penalized() {
    // The Roth already holds $10k of its own (earnings, for the model). A
    // $30k withdrawal consumes the $20k conversion and is penalized on that
    // alone: earnings are not penalized in this version.
    let result = ladder(2033, 30_000.0, vec![lot(2010, 200.0)]);
    let [(penalized, _)] = penalties(&result)[..] else {
        panic!("one penalty: {:?}", penalties(&result));
    };
    assert!((penalized - 20_000.0).abs() < 0.01);
}

#[test]
fn a_conversion_from_an_account_that_is_not_tax_deferred_fails() {
    let config = plan(
        1965,
        1,
        0.0,
        vec![],
        vec![on(
            1,
            year_end(),
            EventEffect::RothConversion {
                from: BROKERAGE,
                to: ROTH,
                amount: TransferAmount::fixed(10_000.0),
                pay_tax_from: None,
            },
        )],
    );
    let result = simulate(&config, 1).unwrap();
    assert!(conversions(&result).is_empty());
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::EvaluationFailed)
    );
}
