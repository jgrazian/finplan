//! Strategy sweeps: which accounts a withdrawal order sells from, and how far
//! bracket filling draws tax-deferred money.

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetId, AssetLot, Cash, Event, EventEffect,
    EventId, EventTrigger, IncomeType, InflationProfile, InvestmentContainer, LotMethod,
    ReturnProfile, ReturnProfileId, SimulationResult, TaxStatus, TransferAmount, WithdrawalOrder,
    WithdrawalSources,
};
use crate::simulation::simulate;

// The Roth has the lowest id, so plain account order is never the answer.
const ROTH: AccountId = AccountId(1);
const IRA: AccountId = AccountId(2);
const BROKERAGE: AccountId = AccountId(3);
const CHECKING: AccountId = AccountId(4);
const FUND: AssetId = AssetId(1);
const START: f64 = 100_000.0;

fn investment(account_id: AccountId, tax_status: TaxStatus) -> Account {
    Account {
        account_id,
        flavor: AccountFlavor::Investment(InvestmentContainer {
            tax_status,
            cash: Cash {
                value: 0.0,
                return_profile_id: ReturnProfileId(0),
            },
            // Basis equals value: selling the brokerage realises no gain.
            positions: vec![AssetLot {
                asset_id: FUND,
                purchase_date: jiff::civil::date(2020, 1, 1),
                units: START,
                cost_basis: START,
            }],
            contribution_limit: None,
        }),
    }
}

/// Flat prices and a one-off sweep into checking, for someone born in
/// `birth_year` — 1960 is past 59.5 in 2030, 1990 is not.
fn plan(
    birth_year: i16,
    order: WithdrawalOrder,
    amount: f64,
    exclude: Vec<AccountId>,
) -> SimulationConfig {
    SimulationConfig {
        start_date: Some(jiff::civil::date(2030, 1, 1)),
        duration_years: 1,
        birth_date: Some(jiff::civil::date(birth_year, 1, 1)),
        inflation_profile: InflationProfile::Fixed(0.0),
        return_profiles: HashMap::from([(ReturnProfileId(0), ReturnProfile::Fixed(0.0))]),
        asset_returns: HashMap::from([(FUND, ReturnProfileId(0))]),
        accounts: vec![
            investment(ROTH, TaxStatus::TaxFree),
            investment(IRA, TaxStatus::TaxDeferred),
            investment(BROKERAGE, TaxStatus::Taxable),
            Account {
                account_id: CHECKING,
                flavor: AccountFlavor::Bank(Cash {
                    value: 0.0,
                    return_profile_id: ReturnProfileId(0),
                }),
            },
        ],
        events: vec![Event {
            event_id: EventId(1),
            trigger: EventTrigger::Date(jiff::civil::date(2030, 3, 1)),
            effects: vec![EventEffect::Sweep {
                sources: WithdrawalSources::Strategy {
                    order,
                    exclude_accounts: exclude,
                },
                to: CHECKING,
                amount: TransferAmount::fixed(amount),
                amount_mode: AmountMode::Gross,
                lot_method: LotMethod::Fifo,
                income_type: IncomeType::Taxable,
            }],
            once: true,
        }],
        ..Default::default()
    }
}

/// How much of each account's holding the sweep sold.
fn sold(result: &SimulationResult, account: AccountId) -> f64 {
    START - result.final_asset_balance(account, FUND).unwrap_or(0.0)
}

fn run(config: &SimulationConfig) -> SimulationResult {
    simulate(config, 7).unwrap()
}

#[test]
fn strategy_order_decides_the_first_account_sold() {
    // The first account covers the $30,000 gross; a sweep counts net proceeds
    // toward its target, so taxes on it may reach into the second, but the
    // order's last account is never touched.
    for (order, first, last) in [
        (WithdrawalOrder::TaxEfficientEarly, BROKERAGE, ROTH),
        (WithdrawalOrder::TaxDeferredFirst, IRA, ROTH),
        (WithdrawalOrder::TaxFreeFirst, ROTH, IRA),
        (WithdrawalOrder::PenaltyAware, BROKERAGE, ROTH),
    ] {
        let result = run(&plan(1960, order, 30_000.0, vec![]));
        assert!(
            (sold(&result, first) - 30_000.0).abs() < 1.0,
            "{order:?}: {first:?} sold {}",
            sold(&result, first)
        );
        assert!(
            sold(&result, last).abs() < 1.0,
            "{order:?}: {last:?} sold {}",
            sold(&result, last)
        );
    }
}

#[test]
fn penalty_aware_reaches_the_roth_before_the_ira_under_59_and_a_half() {
    let result = run(&plan(
        1990,
        WithdrawalOrder::PenaltyAware,
        30_000.0,
        vec![BROKERAGE],
    ));
    assert!((sold(&result, ROTH) - 30_000.0).abs() < 1.0);
    assert!(sold(&result, IRA).abs() < 1.0);
}

#[test]
fn bracket_filling_draws_the_ira_to_the_top_of_the_ceiling_bracket() {
    let order = WithdrawalOrder::BracketFilling { ceiling_rate: 0.12 };
    let result = run(&plan(1960, order, 80_000.0, vec![]));

    // The 12% bracket ends where 22% starts: $47,150 in the default brackets.
    assert!(
        (sold(&result, IRA) - 47_150.0).abs() < 1.0,
        "IRA sold {}",
        sold(&result, IRA)
    );
    assert!(
        sold(&result, BROKERAGE) > 1.0,
        "the rest comes from the brokerage"
    );
    assert!(sold(&result, ROTH).abs() < 1.0, "the Roth is untouched");
}

#[test]
fn bracket_filling_counts_income_already_earned_this_year() {
    let order = WithdrawalOrder::BracketFilling { ceiling_rate: 0.12 };
    let mut config = plan(1960, order, 80_000.0, vec![]);
    config.events.push(Event {
        event_id: EventId(2),
        trigger: EventTrigger::Date(jiff::civil::date(2030, 2, 1)),
        effects: vec![EventEffect::Income {
            to: CHECKING,
            amount: TransferAmount::fixed(30_000.0),
            amount_mode: AmountMode::Gross,
            income_type: IncomeType::Taxable,
        }],
        once: true,
    });
    let result = run(&config);

    assert!(
        (sold(&result, IRA) - 17_150.0).abs() < 1.0,
        "only the room left above $30,000 of wages: IRA sold {}",
        sold(&result, IRA)
    );
}

#[test]
fn bracket_filling_leaves_the_ira_alone_under_59_and_a_half() {
    let order = WithdrawalOrder::BracketFilling { ceiling_rate: 0.12 };
    let result = run(&plan(1990, order, 30_000.0, vec![]));
    assert!(sold(&result, IRA).abs() < 1.0);
    assert!((sold(&result, BROKERAGE) - 30_000.0).abs() < 1.0);
}
