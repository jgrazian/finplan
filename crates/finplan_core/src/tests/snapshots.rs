//! Where each year's wealth snapshot lands.
//!
//! A simulation records wealth at three kinds of moment: the start, each year
//! end, and the final date. The year-end one used to be stamped with whatever
//! checkpoint the clock happened to be on when 31 December came into view —
//! the 1st of December on a plan with monthly bills, and something else again
//! on a plan whose events trigger off balances or net worth, because those
//! trigger dates follow the market and become checkpoints of their own.
//!
//! That made the date market-dependent, and everything measured across
//! iterations depends on it lining up.

use std::collections::BTreeSet;

use crate::config::asset_builder::AssetBuilder;
use crate::config::event_builder::EventBuilder;
use crate::config::{SimulationBuilder, SimulationConfig};
use crate::model::{
    AccountId, BalanceThreshold, Event, EventEffect, EventId, EventTrigger, MonteCarloConfig,
    RepeatInterval, ReturnProfile, TransferAmount,
};
use crate::simulation::{monte_carlo_simulate_with_config, simulate};

/// A plan whose event schedule follows the market.
///
/// The top-up starts the first time net worth crosses a threshold, which a
/// volatile portfolio reaches on a different day in every iteration. From then
/// on its monthly repeats are checkpoints at dates no two seeds agree on.
pub(super) fn market_dependent_plan() -> SimulationConfig {
    let (mut config, _) = SimulationBuilder::new()
        .start(2020, 1, 1)
        .years(10)
        .inflation(0.0)
        .asset(
            AssetBuilder::new("VTI")
                .price(100.0)
                .return_profile(ReturnProfile::Normal {
                    mean: 0.07,
                    std_dev: 0.18,
                }),
        )
        .brokerage("Brokerage", 0.0)
        .position("Brokerage", "VTI", 5_000.0, 500_000.0)
        .bank("Cash", 50_000.0)
        .event(
            EventBuilder::expense("Living expenses")
                .from_account("Cash")
                .amount(4_000.0)
                .monthly(),
        )
        .build();

    config.events.push(Event {
        event_id: EventId(99),
        trigger: EventTrigger::Repeating {
            interval: RepeatInterval::Monthly,
            start_condition: Some(Box::new(EventTrigger::NetWorth {
                threshold: BalanceThreshold::GreaterThanOrEqual(700_000.0),
            })),
            end_condition: None,
            max_occurrences: None,
        },
        effects: vec![EventEffect::CashTransfer {
            from: AccountId(0),
            to: AccountId(1),
            amount: TransferAmount::fixed(10_000.0),
        }],
        once: false,
    });
    config
}

fn grid(config: &SimulationConfig, seed: u64) -> BTreeSet<jiff::civil::Date> {
    simulate(config, seed)
        .expect("simulate")
        .wealth_snapshots
        .iter()
        .map(|s| s.date)
        .collect()
}

#[test]
fn every_year_end_snapshot_lands_on_december_31() {
    let config = market_dependent_plan();
    let dates: Vec<_> = grid(&config, 0).into_iter().collect();

    assert_eq!(
        *dates.first().expect("a starting snapshot"),
        config.start_date.expect("start")
    );
    // Everything between the start and the final date is a year end.
    for date in &dates[1..dates.len() - 1] {
        assert_eq!(
            (date.month(), date.day()),
            (12, 31),
            "{date} is not a year end"
        );
    }
}

#[test]
fn the_snapshot_grid_does_not_move_with_the_market() {
    let config = market_dependent_plan();
    let base = grid(&config, 0);
    assert!(base.len() > 2, "the plan should span several years");

    for seed in 1..200u64 {
        assert_eq!(
            grid(&config, seed),
            base,
            "seed {seed} snapshotted on different dates"
        );
    }
}

#[test]
fn the_real_envelope_can_be_measured_across_a_market_dependent_plan() {
    // The second Monte Carlo pass builds a real-dollar envelope by reading one
    // date across every iteration, so it refuses a grid that does not line up.
    // This is the failure the date fix exists to prevent, at the level a Results
    // run actually hits it.
    let config = market_dependent_plan();
    let mc = MonteCarloConfig {
        iterations: 200,
        seed: Some(3),
        percentiles: vec![0.05, 0.5, 0.95],
        ..Default::default()
    };

    let summary = monte_carlo_simulate_with_config(&config, &mc).expect("a measurable envelope");
    let real = summary.real_net_worth.expect("real net worth");
    assert!(!real.points.is_empty());
    for point in &real.points {
        assert!(point.p5 <= point.p50 && point.p50 <= point.p95);
    }
}

#[test]
fn a_year_end_snapshot_is_valued_on_the_day_it_is_dated() {
    // The clock used to move only after the snapshot was taken, so the year's
    // balances — already compounded to 31 December — were priced at the
    // previous checkpoint's asset prices. A fixed-return asset makes the
    // difference between the two dates arithmetic rather than a matter of luck.
    let (config, _) = SimulationBuilder::new()
        .start(2020, 1, 1)
        .years(3)
        .inflation(0.0)
        .asset(
            AssetBuilder::new("VTI")
                .price(100.0)
                .return_profile(ReturnProfile::Fixed(0.10)),
        )
        .brokerage("Brokerage", 0.0)
        .position("Brokerage", "VTI", 1_000.0, 100_000.0)
        .event(
            EventBuilder::expense("Token")
                .from_account("Brokerage")
                .amount(1.0)
                .monthly(),
        )
        .build();

    let result = simulate(&config, 0).expect("simulate");
    let first_year_end = result
        .wealth_snapshots
        .iter()
        .find(|s| (s.date.month(), s.date.day()) == (12, 31))
        .expect("a year-end snapshot");

    let value: f64 = first_year_end
        .accounts
        .iter()
        .map(crate::model::AccountSnapshot::total_value)
        .sum();
    // A year of 10% on $100,000, less twelve dollars of token spending. Priced
    // a month early it would be nearer $99,200.
    assert!(
        (value - 109_988.0).abs() < 200.0,
        "expected a full year of growth, got {value}"
    );
}
