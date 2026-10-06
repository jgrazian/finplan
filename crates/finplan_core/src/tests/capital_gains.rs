//! Capital gains and the year's running income: short-term gains fill the
//! ordinary brackets, long-term gains stack on top in their own schedule,
//! every sale in an effect sees the ones before it, and a net sale arrives
//! at the net asked for.

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetCoord, AssetId, AssetLot, Cash, Event,
    EventEffect, EventId, EventTrigger, IncomeType, InflationProfile, InvestmentContainer,
    LotMethod, ReturnProfile, ReturnProfileId, TaxConfig, TaxStatus, TaxSummary, TransferAmount,
    WithdrawalSources,
};
use crate::simulation::simulate;
use crate::taxes::calculate_federal_tax;

const FIRST: AccountId = AccountId(1);
const SECOND: AccountId = AccountId(2);
const CHECKING: AccountId = AccountId(9);
const FUND: AssetId = AssetId(1);
const BONDS: AssetId = AssetId(2);

/// A lot of `value` at a flat price of 1, bought on the first of the month.
fn lot(asset_id: AssetId, year: i16, month: i8, value: f64, basis: f64) -> AssetLot {
    AssetLot {
        asset_id,
        purchase_date: jiff::civil::date(year, month, 1),
        units: value,
        cost_basis: basis,
    }
}

fn investment(account_id: AccountId, tax_status: TaxStatus, positions: Vec<AssetLot>) -> Account {
    Account {
        account_id,
        flavor: AccountFlavor::Investment(InvestmentContainer {
            tax_status,
            cash: Cash {
                value: 0.0,
                return_profile_id: ReturnProfileId(0),
            },
            positions,
            contribution_limit: None,
        }),
    }
}

/// A sweep into checking on the first of `month`, 2030.
fn sweep(id: u16, month: i8, sources: WithdrawalSources, amount: f64, mode: AmountMode) -> Event {
    Event {
        event_id: EventId(id),
        trigger: EventTrigger::Date(jiff::civil::date(2030, month, 1)),
        effects: vec![EventEffect::Sweep {
            sources,
            to: CHECKING,
            amount: TransferAmount::fixed(amount),
            amount_mode: mode,
            lot_method: LotMethod::Fifo,
            income_type: IncomeType::Taxable,
        }],
        once: true,
    }
}

/// Flat prices and no inflation for one year, someone past 59½, and the
/// default (2024 single, no deduction) tax config. Returns what checking
/// received and the year's taxes.
fn run(mut accounts: Vec<Account>, events: Vec<Event>) -> (f64, TaxSummary) {
    accounts.push(Account {
        account_id: CHECKING,
        flavor: AccountFlavor::Bank(Cash {
            value: 0.0,
            return_profile_id: ReturnProfileId(0),
        }),
    });
    let config = SimulationConfig {
        start_date: Some(jiff::civil::date(2030, 1, 1)),
        duration_years: 1,
        birth_date: Some(jiff::civil::date(1960, 1, 1)),
        inflation_profile: InflationProfile::Fixed(0.0),
        return_profiles: HashMap::from([(ReturnProfileId(0), ReturnProfile::Fixed(0.0))]),
        asset_returns: HashMap::from([(FUND, ReturnProfileId(0)), (BONDS, ReturnProfileId(0))]),
        accounts,
        events,
        ..Default::default()
    };
    let result = simulate(&config, 7).unwrap();
    (
        result.final_account_balance(CHECKING).unwrap(),
        result.yearly_taxes.first().copied().unwrap_or_default(),
    )
}

fn ordinary_tax(income: f64) -> f64 {
    calculate_federal_tax(income, &TaxConfig::default().federal_brackets)
}

#[test]
fn short_term_gains_count_as_ordinary_income_for_later_income() {
    // $50,000 of short-term gain in March, then $50,000 from an IRA in April:
    // the IRA dollars are taxed from $50,000 up, not from zero.
    let (_, taxes) = run(
        vec![
            investment(
                FIRST,
                TaxStatus::Taxable,
                vec![lot(FUND, 2029, 12, 100_000.0, 50_000.0)],
            ),
            investment(
                SECOND,
                TaxStatus::TaxDeferred,
                vec![lot(FUND, 2020, 1, 100_000.0, 100_000.0)],
            ),
        ],
        vec![
            sweep(
                1,
                3,
                WithdrawalSources::SingleAccount(FIRST),
                100_000.0,
                AmountMode::Gross,
            ),
            sweep(
                2,
                4,
                WithdrawalSources::SingleAccount(SECOND),
                50_000.0,
                AmountMode::Gross,
            ),
        ],
    );
    assert!(
        (taxes.federal_tax - ordinary_tax(100_000.0)).abs() < 0.01,
        "federal {}",
        taxes.federal_tax
    );
}

#[test]
fn long_term_gains_inside_the_zero_band_owe_no_federal_tax() {
    let (_, taxes) = run(
        vec![investment(
            FIRST,
            TaxStatus::Taxable,
            vec![lot(FUND, 2020, 1, 80_000.0, 40_000.0)],
        )],
        vec![sweep(
            1,
            3,
            WithdrawalSources::SingleAccount(FIRST),
            80_000.0,
            AmountMode::Gross,
        )],
    );
    assert!((taxes.capital_gains - 40_000.0).abs() < 0.01);
    assert!(
        taxes.federal_tax.abs() < 0.01,
        "federal {}",
        taxes.federal_tax
    );
}

#[test]
fn one_sale_across_two_funds_taxes_the_second_after_the_first() {
    let (_, taxes) = run(
        vec![investment(
            FIRST,
            TaxStatus::TaxDeferred,
            vec![
                lot(FUND, 2020, 1, 50_000.0, 50_000.0),
                lot(BONDS, 2020, 1, 50_000.0, 50_000.0),
            ],
        )],
        vec![sweep(
            1,
            3,
            WithdrawalSources::SingleAccount(FIRST),
            100_000.0,
            AmountMode::Gross,
        )],
    );
    assert!(
        (taxes.federal_tax - ordinary_tax(100_000.0)).abs() < 0.01,
        "federal {}",
        taxes.federal_tax
    );
}

#[test]
fn one_sweep_across_two_accounts_taxes_the_second_after_the_first() {
    let ira = |id| {
        investment(
            id,
            TaxStatus::TaxDeferred,
            vec![lot(FUND, 2020, 1, 50_000.0, 50_000.0)],
        )
    };
    let (_, taxes) = run(
        vec![ira(FIRST), ira(SECOND)],
        vec![sweep(
            1,
            3,
            WithdrawalSources::Custom(vec![
                AssetCoord {
                    account_id: FIRST,
                    asset_id: FUND,
                },
                AssetCoord {
                    account_id: SECOND,
                    asset_id: FUND,
                },
            ]),
            100_000.0,
            AmountMode::Gross,
        )],
    );
    assert!(
        (taxes.federal_tax - ordinary_tax(100_000.0)).abs() < 0.01,
        "federal {}",
        taxes.federal_tax
    );
}

#[test]
fn a_net_sale_arrives_at_the_net_from_every_kind_of_account() {
    for (status, basis) in [
        (TaxStatus::TaxDeferred, 100_000.0),
        (TaxStatus::TaxFree, 50_000.0),
        // Gains taxed in the 0% band and then at 15%.
        (TaxStatus::Taxable, 20_000.0),
    ] {
        let (received, _) = run(
            vec![investment(
                FIRST,
                status,
                vec![lot(FUND, 2020, 1, 100_000.0, basis)],
            )],
            vec![sweep(
                1,
                3,
                WithdrawalSources::SingleAccount(FIRST),
                60_000.0,
                AmountMode::Net,
            )],
        );
        assert!(
            (received - 60_000.0).abs() < 0.05,
            "{status:?}: received {received}"
        );
    }
}
