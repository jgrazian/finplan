//! `MarketShock`: a one-time drop in market asset prices that leaves cash,
//! property and liabilities alone.

use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::model::{
    Account, AccountFlavor, AccountId, AssetId, AssetLot, Cash, Event, EventEffect, EventId,
    EventTrigger, FixedAsset, InflationProfile, InvestmentContainer, LoanDetail, ReturnProfile,
    ReturnProfileId, StateEvent, TaxStatus,
};
use crate::simulation::simulate;

const CASH: AccountId = AccountId(1);
const BROKERAGE: AccountId = AccountId(2);
const HOUSE: AccountId = AccountId(3);
const LOAN: AccountId = AccountId(4);
const STOCK: AssetId = AssetId(1);
const HOUSE_ASSET: AssetId = AssetId(2);
const FLAT: ReturnProfileId = ReturnProfileId(0);
const GROWTH: ReturnProfileId = ReturnProfileId(1);

fn accounts() -> Vec<Account> {
    vec![
        Account {
            account_id: CASH,
            flavor: AccountFlavor::Bank(Cash {
                value: 50_000.0,
                return_profile_id: FLAT,
            }),
        },
        Account {
            account_id: BROKERAGE,
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 0.0,
                    return_profile_id: FLAT,
                },
                positions: vec![AssetLot {
                    asset_id: STOCK,
                    purchase_date: jiff::civil::date(2024, 1, 1),
                    units: 1_000.0,
                    cost_basis: 100_000.0,
                }],
                contribution_limit: None,
            }),
        },
        Account {
            account_id: HOUSE,
            flavor: AccountFlavor::Property(FixedAsset {
                asset_id: HOUSE_ASSET,
                value: 400_000.0,
                cost_basis: None,
            }),
        },
        Account {
            account_id: LOAN,
            flavor: AccountFlavor::Liability(LoanDetail {
                principal: 200_000.0,
                interest_rate: 0.0,
                repayment: None,
                schedule: None,
            }),
        },
    ]
}

fn config(stock_return: f64, events: Vec<Event>) -> SimulationConfig {
    SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 4,
        inflation_profile: InflationProfile::None,
        return_profiles: HashMap::from([
            (FLAT, ReturnProfile::None),
            (GROWTH, ReturnProfile::Fixed(stock_return)),
        ]),
        asset_returns: HashMap::from([(STOCK, GROWTH), (HOUSE_ASSET, GROWTH)]),
        asset_prices: HashMap::from([(STOCK, 100.0), (HOUSE_ASSET, 400_000.0)]),
        accounts: accounts(),
        events,
        ..Default::default()
    }
}

fn shock(drop: f64) -> Event {
    Event {
        event_id: EventId(1),
        trigger: EventTrigger::Date(jiff::civil::date(2026, 1, 1)),
        effects: vec![EventEffect::MarketShock { drop }],
        once: true,
    }
}

/// A 30% shock leaves the brokerage at 70% of the unshocked path, and the
/// cash, the house and the loan exactly where they would have been.
#[test]
fn shock_marks_down_market_assets_only() {
    let base = simulate(&config(0.05, vec![]), 7).unwrap();
    let shocked = simulate(&config(0.05, vec![shock(0.3)]), 7).unwrap();

    let brokerage_base = base.final_account_balance(BROKERAGE).unwrap();
    let brokerage_shocked = shocked.final_account_balance(BROKERAGE).unwrap();
    assert!(
        (brokerage_shocked / brokerage_base - 0.7).abs() < 1e-9,
        "brokerage {brokerage_shocked} vs {brokerage_base}"
    );

    for account in [CASH, HOUSE, LOAN] {
        let a = base.final_account_balance(account).unwrap();
        let b = shocked.final_account_balance(account).unwrap();
        assert!((a - b).abs() < 1e-6, "{account:?}: {a} vs {b}");
    }
}

/// The shock is on the ledger with the assets it hit, and returns keep
/// compounding from the lower level (the ratio holds through plan end).
#[test]
fn shock_is_recorded_on_the_ledger() {
    let shocked = simulate(&config(0.0, vec![shock(0.5)]), 1).unwrap();
    let entries: Vec<_> = shocked
        .ledger
        .iter()
        .filter_map(|e| match &e.event {
            StateEvent::MarketShock { drop, assets } => Some((e.date, *drop, assets.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(entries.len(), 1);
    let (date, drop, assets) = &entries[0];
    assert_eq!(*date, jiff::civil::date(2026, 1, 1));
    assert!((drop - 0.5).abs() < 1e-12);
    assert_eq!(assets, &vec![STOCK]);

    let brokerage = shocked.final_account_balance(BROKERAGE).unwrap();
    assert!((brokerage - 50_000.0).abs() < 1e-6, "brokerage {brokerage}");
}

/// A zero (or nonsensical) drop is a no-op.
#[test]
fn zero_drop_is_a_no_op() {
    let base = simulate(&config(0.05, vec![]), 3).unwrap();
    let shocked = simulate(&config(0.05, vec![shock(0.0)]), 3).unwrap();
    let a = base.final_account_balance(BROKERAGE).unwrap();
    let b = shocked.final_account_balance(BROKERAGE).unwrap();
    assert!((a - b).abs() < 1e-9);
    assert!(
        !shocked
            .ledger
            .iter()
            .any(|e| matches!(e.event, StateEvent::MarketShock { .. }))
    );
}
