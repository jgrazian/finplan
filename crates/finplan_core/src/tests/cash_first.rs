//! Cash first: a withdrawal spends an investment account's own cash before it
//! sells that account's holdings, taxed by the account's status.

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetCoord, AssetId, AssetLot, Cash, Event,
    EventEffect, EventId, EventTrigger, FundingPolicy, IncomeType, InflationProfile,
    InvestmentContainer, LotMethod, RepeatInterval, ReturnProfile, ReturnProfileId,
    SimulationResult, StateEvent, TaxStatus, TransferAmount, WithdrawalOrder, WithdrawalSources,
};
use crate::simulation::simulate;

const ROTH: AccountId = AccountId(1);
const IRA: AccountId = AccountId(2);
const BROKERAGE: AccountId = AccountId(3);
const CHECKING: AccountId = AccountId(4);
const FUND: AssetId = AssetId(1);
const HOLDING: f64 = 100_000.0;

fn investment(account_id: AccountId, tax_status: TaxStatus, cash: f64) -> Account {
    Account {
        account_id,
        flavor: AccountFlavor::Investment(InvestmentContainer {
            tax_status,
            cash: Cash {
                value: cash,
                return_profile_id: ReturnProfileId(0),
            },
            // Half the value is gain, so a taxable sale would show its tax.
            positions: vec![AssetLot {
                asset_id: FUND,
                purchase_date: jiff::civil::date(2020, 1, 1),
                units: HOLDING,
                cost_basis: HOLDING / 2.0,
            }],
            contribution_limit: None,
        }),
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

fn sweep(sources: WithdrawalSources, amount: f64, amount_mode: AmountMode) -> EventEffect {
    EventEffect::Sweep {
        sources,
        to: CHECKING,
        amount: TransferAmount::fixed(amount),
        amount_mode,
        lot_method: LotMethod::Fifo,
        income_type: IncomeType::Taxable,
    }
}

/// Flat prices, no inflation, the three investment accounts (each holding
/// `cash`) and an empty checking account, for someone born in `birth_year`.
fn plan(birth_year: i16, years: usize, cash: [f64; 3], events: Vec<Event>) -> SimulationConfig {
    SimulationConfig {
        start_date: Some(jiff::civil::date(2030, 1, 1)),
        duration_years: years,
        birth_date: Some(jiff::civil::date(birth_year, 1, 1)),
        inflation_profile: InflationProfile::Fixed(0.0),
        return_profiles: HashMap::from([(ReturnProfileId(0), ReturnProfile::Fixed(0.0))]),
        asset_returns: HashMap::from([(FUND, ReturnProfileId(0))]),
        accounts: vec![
            investment(ROTH, TaxStatus::TaxFree, cash[0]),
            investment(IRA, TaxStatus::TaxDeferred, cash[1]),
            investment(BROKERAGE, TaxStatus::Taxable, cash[2]),
            Account {
                account_id: CHECKING,
                flavor: AccountFlavor::Bank(Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(0),
                }),
            },
        ],
        events,
        ..Default::default()
    }
}

/// FNV-1a over the ledger and yearly taxes as `Debug` prints them: every
/// figure at full precision, in order.
fn fingerprint(result: &SimulationResult) -> u64 {
    let text = format!("{:?}{:?}", result.ledger, result.yearly_taxes);
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn a_plan_with_no_investment_cash_runs_as_before() {
    // Every withdrawal path at once: a bracket-filling Net sweep, a single
    // account sweep, a ProRata sweep, a single-asset sweep, RMDs and the
    // funding policy, on accounts whose cash is zero throughout.
    let mut config = plan(
        1957,
        3,
        [0.0; 3],
        vec![
            yearly(
                1,
                3,
                1,
                sweep(
                    WithdrawalSources::Strategy {
                        order: WithdrawalOrder::BracketFilling { ceiling_rate: 0.12 },
                        exclude_accounts: vec![],
                    },
                    20_000.0,
                    AmountMode::Net,
                ),
            ),
            yearly(
                2,
                6,
                1,
                sweep(
                    WithdrawalSources::SingleAccount(BROKERAGE),
                    5_000.0,
                    AmountMode::Gross,
                ),
            ),
            yearly(
                3,
                9,
                1,
                sweep(
                    WithdrawalSources::Strategy {
                        order: WithdrawalOrder::ProRata,
                        exclude_accounts: vec![],
                    },
                    10_000.0,
                    AmountMode::Gross,
                ),
            ),
            yearly(
                4,
                10,
                1,
                sweep(
                    WithdrawalSources::SingleAsset(AssetCoord {
                        account_id: ROTH,
                        asset_id: FUND,
                    }),
                    3_000.0,
                    AmountMode::Net,
                ),
            ),
            yearly(
                5,
                12,
                15,
                EventEffect::ApplyRmd {
                    destination: CHECKING,
                    lot_method: LotMethod::Fifo,
                },
            ),
            Event {
                event_id: EventId(6),
                trigger: EventTrigger::Repeating {
                    interval: RepeatInterval::Monthly,
                    start_condition: None,
                    end_condition: None,
                    max_occurrences: None,
                },
                effects: vec![EventEffect::Expense {
                    from: CHECKING,
                    amount: TransferAmount::fixed(4_000.0),
                }],
                once: false,
            },
        ],
    );
    config.funding = Some(FundingPolicy {
        order: WithdrawalOrder::TaxEfficientEarly,
        exclude_accounts: vec![],
        from: None,
    });
    let result = simulate(&config, 7).unwrap();

    assert!(
        result
            .ledger
            .iter()
            .any(|e| matches!(e.event, StateEvent::RmdWithdrawal { .. }))
    );
    assert!(
        !result
            .ledger
            .iter()
            .any(|e| matches!(e.event, StateEvent::CashWithdrawal { .. })),
        "zero cash draws nothing"
    );
    // Pinned from the engine before cash first: the same ledger, figure for
    // figure.
    assert_eq!(fingerprint(&result), 2_698_878_757_188_578_784);
}

/// A one-off sweep from `sources` into checking on 1 March 2030.
fn once(sources: WithdrawalSources, amount: f64, amount_mode: AmountMode) -> Vec<Event> {
    vec![Event {
        event_id: EventId(1),
        trigger: EventTrigger::Date(jiff::civil::date(2030, 3, 1)),
        effects: vec![sweep(sources, amount, amount_mode)],
        once: true,
    }]
}

fn run(config: &SimulationConfig) -> SimulationResult {
    simulate(config, 7).unwrap()
}

/// Total of the account's `CashWithdrawal` entries.
fn cash_withdrawn(result: &SimulationResult, account: AccountId) -> f64 {
    result
        .ledger
        .iter()
        .filter_map(|e| match e.event {
            StateEvent::CashWithdrawal { account_id, amount } if account_id == account => {
                Some(amount)
            }
            _ => None,
        })
        .sum()
}

/// Value of the account's holding sold.
fn sold(result: &SimulationResult, account: AccountId) -> f64 {
    HOLDING - result.final_asset_balance(account, FUND).unwrap_or(0.0)
}

fn final_cash(result: &SimulationResult, account: AccountId) -> f64 {
    result.final_account_balance(account).unwrap() - HOLDING + sold(result, account)
}

fn has(result: &SimulationResult, matches: impl Fn(&StateEvent) -> bool) -> bool {
    result.ledger.iter().any(|e| matches(&e.event))
}

/// `(gross, tax)` of every `IncomeTax` entry, and the penalties.
fn income_tax(result: &SimulationResult) -> (f64, f64, f64) {
    result
        .ledger
        .iter()
        .fold((0.0, 0.0, 0.0), |(gross, tax, penalty), e| match e.event {
            StateEvent::IncomeTax {
                gross_amount,
                federal_tax,
                state_tax,
            } => (gross + gross_amount, tax + federal_tax + state_tax, penalty),
            StateEvent::EarlyWithdrawalPenalty { penalty_amount, .. } => {
                (gross, tax, penalty + penalty_amount)
            }
            _ => (gross, tax, penalty),
        })
}

#[test]
fn taxable_cash_covers_a_sweep_with_no_sale_and_no_tax() {
    let config = plan(
        1960,
        1,
        [0.0, 0.0, 10_000.0],
        once(
            WithdrawalSources::SingleAccount(BROKERAGE),
            6_000.0,
            AmountMode::Gross,
        ),
    );
    let result = run(&config);

    assert_eq!(cash_withdrawn(&result, BROKERAGE), 6_000.0);
    assert_eq!(final_cash(&result, BROKERAGE), 4_000.0);
    assert_eq!(result.final_account_balance(CHECKING), Some(6_000.0));
    assert!(!has(&result, |e| matches!(e, StateEvent::AssetSale { .. })));
    assert!(!has(&result, StateEvent::is_tax_event));
}

#[test]
fn a_sweep_larger_than_the_cash_sells_holdings_for_the_rest() {
    let config = plan(
        1960,
        1,
        [0.0, 0.0, 10_000.0],
        once(
            WithdrawalSources::Strategy {
                order: WithdrawalOrder::TaxEfficientEarly,
                exclude_accounts: vec![],
            },
            15_000.0,
            AmountMode::Gross,
        ),
    );
    let result = run(&config);

    assert_eq!(cash_withdrawn(&result, BROKERAGE), 10_000.0);
    assert_eq!(final_cash(&result, BROKERAGE), 0.0);
    assert!((sold(&result, BROKERAGE) - 5_000.0).abs() < 0.01);
    // The cash is ledgered before the sale it spares.
    let first = |pred: fn(&StateEvent) -> bool| result.ledger.iter().position(|e| pred(&e.event));
    assert!(
        first(|e| matches!(e, StateEvent::CashWithdrawal { .. }))
            < first(|e| matches!(e, StateEvent::AssetSale { .. }))
    );
}

#[test]
fn tax_deferred_cash_is_ordinary_income_and_penalized_before_59_and_a_half() {
    for (birth_year, penalized) in [(1990, true), (1960, false)] {
        let config = plan(
            birth_year,
            1,
            [0.0, 10_000.0, 0.0],
            once(
                WithdrawalSources::SingleAccount(IRA),
                10_000.0,
                AmountMode::Gross,
            ),
        );
        let result = run(&config);

        assert_eq!(cash_withdrawn(&result, IRA), 10_000.0);
        assert!(!has(&result, |e| matches!(e, StateEvent::AssetSale { .. })));
        let (gross, tax, penalty) = income_tax(&result);
        assert_eq!(gross, 10_000.0, "the whole draw is ordinary income");
        assert!(tax > 0.0);
        if penalized {
            assert!(
                (penalty - 1_000.0).abs() < 1e-9,
                "10% of the gross: {penalty}"
            );
        } else {
            assert_eq!(penalty, 0.0);
        }
        let checking = result.final_account_balance(CHECKING).unwrap();
        assert!((checking - (10_000.0 - tax - penalty)).abs() < 1e-6);
    }
}

#[test]
fn a_net_sweep_from_tax_deferred_cash_grosses_up() {
    // Under 59.5, so the gross-up has the penalty to cover too.
    let config = plan(
        1990,
        1,
        [0.0, 50_000.0, 0.0],
        once(
            WithdrawalSources::SingleAccount(IRA),
            10_000.0,
            AmountMode::Net,
        ),
    );
    let result = run(&config);

    let checking = result.final_account_balance(CHECKING).unwrap();
    assert!(
        (checking - 10_000.0).abs() < 0.01,
        "net arrives: {checking}"
    );
    let drawn = cash_withdrawn(&result, IRA);
    let (gross, tax, penalty) = income_tax(&result);
    assert!(drawn > 10_000.0);
    assert_eq!(gross, drawn);
    assert!((drawn - tax - penalty - 10_000.0).abs() < 0.01);
    assert!(!has(&result, |e| matches!(e, StateEvent::AssetSale { .. })));
}

#[test]
fn an_rmd_counts_the_accounts_cash_first() {
    // 74 in 2031: the RMD is due on the IRA's 2030 year-end balance, cash
    // included, and draws that cash before it sells.
    let config = plan(
        1957,
        2,
        [0.0, 2_000.0, 0.0],
        vec![yearly(
            1,
            12,
            15,
            EventEffect::ApplyRmd {
                destination: CHECKING,
                lot_method: LotMethod::Fifo,
            },
        )],
    );
    let result = run(&config);

    let (required, actual) = result
        .ledger
        .iter()
        .find_map(|e| match e.event {
            StateEvent::RmdWithdrawal {
                required_amount,
                actual_amount,
                ..
            } => Some((required_amount, actual_amount)),
            _ => None,
        })
        .expect("an RMD in 2031");
    assert!((required - 102_000.0 / 25.5).abs() < 1e-6, "{required}");
    assert_eq!(cash_withdrawn(&result, IRA), 2_000.0);
    assert!((sold(&result, IRA) - (required - 2_000.0)).abs() < 1e-6);
    assert!((actual - required).abs() < 1e-6, "cash counts: {actual}");
    let (gross, _, _) = income_tax(&result);
    assert!((gross - required).abs() < 1e-6);
}

#[test]
fn bracket_filling_counts_tax_deferred_cash_against_the_ceiling() {
    let config = plan(
        1960,
        1,
        [0.0, 20_000.0, 0.0],
        once(
            WithdrawalSources::Strategy {
                order: WithdrawalOrder::BracketFilling { ceiling_rate: 0.12 },
                exclude_accounts: vec![],
            },
            80_000.0,
            AmountMode::Gross,
        ),
    );
    let result = run(&config);

    // The 12% bracket ends at $47,150: the first pass draws $20,000 of cash,
    // then sells $27,150. (The brokerage's gains tax leaves a remainder that
    // the IRA's uncapped second pass covers.)
    assert_eq!(cash_withdrawn(&result, IRA), 20_000.0);
    let first_sale = result
        .ledger
        .iter()
        .find_map(|e| match e.event {
            StateEvent::AssetSale {
                account_id,
                proceeds,
                ..
            } if account_id == IRA => Some(proceeds),
            _ => None,
        })
        .unwrap();
    assert!((first_sale - 27_150.0).abs() < 1.0, "IRA sold {first_sale}");
}

#[test]
fn a_single_asset_sweep_sells_and_leaves_the_cash() {
    let config = plan(
        1960,
        1,
        [0.0, 0.0, 10_000.0],
        once(
            WithdrawalSources::SingleAsset(AssetCoord {
                account_id: BROKERAGE,
                asset_id: FUND,
            }),
            6_000.0,
            AmountMode::Gross,
        ),
    );
    let result = run(&config);

    assert_eq!(cash_withdrawn(&result, BROKERAGE), 0.0);
    assert_eq!(final_cash(&result, BROKERAGE), 10_000.0);
    assert!((sold(&result, BROKERAGE) - 6_000.0).abs() < 0.01);
}

#[test]
fn a_second_visit_does_not_draw_the_same_cash_twice() {
    // ProRata's first pass takes the Roth's share, cash first; the taxes on
    // the other shares send it back to the Roth for the rest.
    let config = plan(
        1960,
        1,
        [1_000.0, 0.0, 0.0],
        once(
            WithdrawalSources::Strategy {
                order: WithdrawalOrder::ProRata,
                exclude_accounts: vec![],
            },
            10_000.0,
            AmountMode::Gross,
        ),
    );
    let result = run(&config);

    let roth_draws = result
        .ledger
        .iter()
        .filter(
            |e| matches!(e.event, StateEvent::AssetSale { account_id, .. } if account_id == ROTH),
        )
        .count();
    assert_eq!(roth_draws, 2, "the Roth is visited twice");
    assert_eq!(cash_withdrawn(&result, ROTH), 1_000.0);
    assert_eq!(final_cash(&result, ROTH), 0.0);
    assert!((result.final_account_balance(CHECKING).unwrap() - 10_000.0).abs() < 0.01);
}

#[test]
fn the_funding_policy_spends_investment_cash_before_selling() {
    let mut config = plan(
        1960,
        1,
        [0.0, 0.0, 5_000.0],
        vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Date(jiff::civil::date(2030, 3, 1)),
            effects: vec![EventEffect::Expense {
                from: CHECKING,
                amount: TransferAmount::fixed(8_000.0),
            }],
            once: true,
        }],
    );
    config.funding = Some(FundingPolicy {
        order: WithdrawalOrder::TaxEfficientEarly,
        exclude_accounts: vec![],
        from: None,
    });
    let result = run(&config);

    assert_eq!(cash_withdrawn(&result, BROKERAGE), 5_000.0);
    assert!(
        sold(&result, BROKERAGE) > 3_000.0,
        "the rest, grossed for tax"
    );
    assert!(result.final_account_balance(CHECKING).unwrap().abs() < 0.01);
}
