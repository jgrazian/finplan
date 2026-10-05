//! The yearly cash-flow columns close: last year's net worth, plus the year's
//! income less spending less taxes (`net_cash_flow`), plus its growth
//! (`appreciation`), is this year's net worth — on a plan that moves money
//! every way the engine can.

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetCoord, AssetId, AssetLot, Cash, Event,
    EventEffect, EventId, EventTrigger, Financing, FixedAsset, FundingPolicy, IncomeType,
    InflationProfile, InvestmentContainer, LoanDetail, LotMethod, Repayment, RepeatInterval,
    ReturnProfile, ReturnProfileId, SimulationResult, TaxStatus, TransferAmount, WithdrawalOrder,
    WithdrawalSources,
};
use crate::simulation::simulate;

const CHECKING: AccountId = AccountId(1);
const BROKERAGE: AccountId = AccountId(2);
const IRA: AccountId = AccountId(3);
const ROTH: AccountId = AccountId(4);
const HOUSE: AccountId = AccountId(5);
const MORTGAGE: AccountId = AccountId(6);
const CAR_LOAN: AccountId = AccountId(7);

const FUND: AssetId = AssetId(1);
const STOCK: AssetId = AssetId(2);
const HOME: AssetId = AssetId(3);

const FLAT: ReturnProfileId = ReturnProfileId(0);
const MARKET: ReturnProfileId = ReturnProfileId(1);
const SAVINGS: ReturnProfileId = ReturnProfileId(2);
const HOUSING: ReturnProfileId = ReturnProfileId(3);

fn investment(account_id: AccountId, tax_status: TaxStatus, units: f64) -> Account {
    Account {
        account_id,
        flavor: AccountFlavor::Investment(InvestmentContainer {
            tax_status,
            cash: Cash {
                value: 5_000.0,
                return_profile_id: SAVINGS,
            },
            positions: vec![AssetLot {
                asset_id: FUND,
                purchase_date: jiff::civil::date(2020, 1, 1),
                units,
                cost_basis: units * 40.0,
            }],
            contribution_limit: None,
        }),
    }
}

fn loan(account_id: AccountId, principal: f64, rate: f64, repayment: Option<Repayment>) -> Account {
    Account {
        account_id,
        flavor: AccountFlavor::Liability(LoanDetail {
            principal,
            interest_rate: rate,
            repayment,
            schedule: None,
        }),
    }
}

fn every(id: u16, interval: RepeatInterval, from: jiff::civil::Date, effect: EventEffect) -> Event {
    Event {
        event_id: EventId(id),
        trigger: EventTrigger::Repeating {
            interval,
            start_condition: Some(Box::new(EventTrigger::Date(from))),
            end_condition: None,
            max_occurrences: None,
        },
        effects: vec![effect],
        once: false,
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

fn fixed(amount: f64) -> TransferAmount {
    TransferAmount::fixed(amount)
}

/// Pay and bills, saving into the IRA, a financed home bought and later sold,
/// RSUs vested both ways, a crash, Roth conversions withheld and paid from
/// checking (one on Dec 31), penalized early withdrawals, an inheritance, an
/// extra loan payment, and the funding policy behind it all — with volatile
/// prices, interest on cash, inflation, and a start partway through a year.
fn plan() -> SimulationConfig {
    let d = jiff::civil::date;
    let events = vec![
        every(
            1,
            RepeatInterval::Monthly,
            d(2030, 4, 1),
            EventEffect::Income {
                to: CHECKING,
                amount: fixed(15_000.0),
                amount_mode: AmountMode::Gross,
                income_type: IncomeType::Taxable,
            },
        ),
        every(
            2,
            RepeatInterval::Monthly,
            d(2030, 4, 1),
            EventEffect::Income {
                to: CHECKING,
                amount: fixed(1_000.0),
                amount_mode: AmountMode::Net,
                income_type: IncomeType::Taxable,
            },
        ),
        every(
            3,
            RepeatInterval::Monthly,
            d(2030, 4, 5),
            EventEffect::Expense {
                from: CHECKING,
                amount: fixed(7_000.0),
            },
        ),
        every(
            4,
            RepeatInterval::Monthly,
            d(2030, 4, 10),
            EventEffect::AssetPurchase {
                from: CHECKING,
                to: AssetCoord {
                    account_id: IRA,
                    asset_id: FUND,
                },
                amount: fixed(1_500.0),
            },
        ),
        once(
            5,
            d(2031, 6, 1),
            EventEffect::BuyProperty {
                property: HOUSE,
                price: fixed(600_000.0),
                from: CHECKING,
                financing: Some(Financing {
                    loan: MORTGAGE,
                    down_payment: fixed(120_000.0),
                    term_months: 360,
                }),
            },
        ),
        every(
            6,
            RepeatInterval::Quarterly,
            d(2030, 5, 15),
            EventEffect::RsuVesting {
                to: BROKERAGE,
                asset: AssetCoord {
                    account_id: BROKERAGE,
                    asset_id: STOCK,
                },
                units: 40.0,
                sell_to_cover: false,
                lot_method: LotMethod::Fifo,
            },
        ),
        every(
            7,
            RepeatInterval::Quarterly,
            d(2030, 6, 20),
            EventEffect::RsuVesting {
                to: BROKERAGE,
                asset: AssetCoord {
                    account_id: BROKERAGE,
                    asset_id: STOCK,
                },
                units: 25.0,
                sell_to_cover: true,
                lot_method: LotMethod::Fifo,
            },
        ),
        once(8, d(2033, 4, 2), EventEffect::MarketShock { drop: 0.3 }),
        every(
            9,
            RepeatInterval::Yearly,
            d(2030, 12, 31),
            EventEffect::RothConversion {
                from: IRA,
                to: ROTH,
                amount: fixed(20_000.0),
                pay_tax_from: None,
            },
        ),
        every(
            10,
            RepeatInterval::Yearly,
            d(2031, 7, 1),
            EventEffect::RothConversion {
                from: IRA,
                to: ROTH,
                amount: fixed(15_000.0),
                pay_tax_from: Some(CHECKING),
            },
        ),
        every(
            11,
            RepeatInterval::Yearly,
            d(2032, 2, 1),
            EventEffect::Sweep {
                sources: WithdrawalSources::SingleAccount(IRA),
                to: CHECKING,
                amount: fixed(25_000.0),
                amount_mode: AmountMode::Net,
                lot_method: LotMethod::Fifo,
                income_type: IncomeType::Taxable,
            },
        ),
        once(
            12,
            d(2034, 9, 9),
            EventEffect::AdjustBalance {
                account: CHECKING,
                amount: fixed(80_000.0),
            },
        ),
        once(
            13,
            d(2035, 3, 3),
            EventEffect::CashTransfer {
                from: CHECKING,
                to: MORTGAGE,
                amount: fixed(30_000.0),
            },
        ),
        once(
            14,
            d(2039, 8, 1),
            EventEffect::SellProperty {
                property: HOUSE,
                to: CHECKING,
                selling_cost_rate: 0.06,
                gain_exclusion: 0.0,
                payoff: Some(MORTGAGE),
            },
        ),
        every(
            15,
            RepeatInterval::Yearly,
            d(2030, 12, 31),
            EventEffect::Expense {
                from: CHECKING,
                amount: fixed(4_000.0),
            },
        ),
        once(
            16,
            d(2036, 1, 1),
            EventEffect::AdjustBalance {
                account: CAR_LOAN,
                amount: fixed(10_000.0),
            },
        ),
    ];

    SimulationConfig {
        start_date: Some(jiff::civil::date(2030, 3, 15)),
        duration_years: 12,
        birth_date: Some(jiff::civil::date(1985, 7, 4)),
        inflation_profile: InflationProfile::Fixed(0.025),
        return_profiles: HashMap::from([
            (FLAT, ReturnProfile::None),
            (
                MARKET,
                ReturnProfile::Normal {
                    mean: 0.07,
                    std_dev: 0.18,
                },
            ),
            (SAVINGS, ReturnProfile::Fixed(0.04)),
            (HOUSING, ReturnProfile::Fixed(0.03)),
        ]),
        asset_returns: HashMap::from([(FUND, MARKET), (STOCK, MARKET), (HOME, HOUSING)]),
        asset_prices: HashMap::from([(FUND, 50.0), (STOCK, 120.0), (HOME, 100.0)]),
        accounts: vec![
            Account {
                account_id: CHECKING,
                flavor: AccountFlavor::Bank(Cash {
                    value: 200_000.0,
                    return_profile_id: SAVINGS,
                }),
            },
            investment(BROKERAGE, TaxStatus::Taxable, 4_000.0),
            investment(IRA, TaxStatus::TaxDeferred, 8_000.0),
            investment(ROTH, TaxStatus::TaxFree, 1_000.0),
            Account {
                account_id: HOUSE,
                flavor: AccountFlavor::Property(FixedAsset {
                    asset_id: HOME,
                    value: 0.0,
                    cost_basis: None,
                }),
            },
            loan(MORTGAGE, 0.0, 0.065, None),
            loan(
                CAR_LOAN,
                25_000.0,
                0.08,
                Some(Repayment {
                    from: CHECKING,
                    term_months: 48,
                }),
            ),
        ],
        events,
        funding: Some(FundingPolicy {
            order: WithdrawalOrder::TaxEfficientEarly,
            exclude_accounts: vec![],
            from: None,
        }),
        ..Default::default()
    }
}

fn net_worth(result: &SimulationResult, index: usize) -> f64 {
    result.wealth_snapshots[index]
        .accounts
        .iter()
        .map(|a| a.total_value())
        .sum()
}

/// Net worth at the end of each year, opening with the start.
fn year_ends(result: &SimulationResult) -> (f64, HashMap<i16, f64>) {
    let mut ends = HashMap::new();
    for (index, snapshot) in result.wealth_snapshots.iter().enumerate().skip(1) {
        ends.insert(snapshot.date.year(), net_worth(result, index));
    }
    (net_worth(result, 0), ends)
}

#[test]
fn each_year_closes_from_last_years_net_worth() {
    for seed in [1, 7, 42, 2024] {
        let result = simulate(&plan(), seed).unwrap();
        let (mut previous, ends) = year_ends(&result);
        assert_eq!(result.yearly_cash_flows.len(), 13);

        for flow in &result.yearly_cash_flows {
            let end = ends[&flow.year];
            let explained = previous + flow.net_cash_flow + flow.appreciation;
            assert!(
                (end - explained).abs() < 0.01,
                "seed {seed}, {}: net worth {end:.2}, but {previous:.2} + net {:.2} + growth {:.2} \
                 = {explained:.2} (off by {:.2})",
                flow.year,
                flow.net_cash_flow,
                flow.appreciation,
                end - explained
            );
            assert!((flow.net_cash_flow - (flow.income - flow.expenses - flow.taxes)).abs() < 1e-6);
            previous = end;
        }
    }
}

/// The taxes the columns subtract are the year's tax bill: income and gains
/// tax plus penalties, each counted once.
#[test]
fn taxes_match_the_years_tax_summary() {
    let result = simulate(&plan(), 7).unwrap();
    for flow in &result.yearly_cash_flows {
        let billed = result
            .yearly_taxes
            .iter()
            .find(|t| t.year == flow.year)
            .map_or(0.0, |t| t.total_tax + t.early_withdrawal_penalties);
        assert!(
            (flow.taxes - billed).abs() < 0.01,
            "{}: {} vs {billed}",
            flow.year,
            flow.taxes
        );
    }
}

/// Growth is the market's doing, not the plan's: with every price and rate
/// flat, a year's appreciation is zero however much money moves.
#[test]
fn flat_markets_grow_nothing() {
    let mut config = plan();
    for profile in config.return_profiles.values_mut() {
        *profile = ReturnProfile::None;
    }
    config.events.retain(|e| e.event_id != EventId(8));
    let result = simulate(&config, 1).unwrap();
    for flow in &result.yearly_cash_flows {
        assert!(
            flow.appreciation.abs() < 1e-6,
            "{}: {}",
            flow.year,
            flow.appreciation
        );
    }
}
