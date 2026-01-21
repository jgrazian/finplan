//! Criterion benchmarks for finplan_core simulation
//!
//! Run with: cargo bench -p finplan_core

use std::collections::HashMap;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use finplan_core::config::SimulationConfig;
use finplan_core::metrics::InstrumentationConfig;
use finplan_core::model::{
    Account, AccountFlavor, AccountId, AssetId, AssetLot, BalanceThreshold, Cash, Event,
    EventEffect, EventId, EventTrigger, IncomeType, InflationProfile, InvestmentContainer,
    MonteCarloConfig, RepeatInterval, ReturnProfile, ReturnProfileId, TaxStatus, TransferAmount,
};
use finplan_core::simulation::{monte_carlo_simulate_with_config, simulate, simulate_with_metrics};

fn create_basic_config(duration_years: usize) -> SimulationConfig {
    SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years,
        birth_date: Some(jiff::civil::date(1980, 6, 15)),
        inflation_profile: InflationProfile::Fixed(0.03),
        return_profiles: HashMap::from([
            (ReturnProfileId(0), ReturnProfile::Fixed(0.07)),
            (ReturnProfileId(1), ReturnProfile::Fixed(0.02)),
        ]),
        asset_returns: HashMap::from([(AssetId(1), ReturnProfileId(0))]),
        events: vec![],
        accounts: vec![Account {
            account_id: AccountId(1),
            flavor: AccountFlavor::Investment(InvestmentContainer {
                tax_status: TaxStatus::Taxable,
                cash: Cash {
                    value: 10_000.0,
                    return_profile_id: ReturnProfileId(1),
                },
                positions: vec![AssetLot {
                    asset_id: AssetId(1),
                    purchase_date: jiff::civil::date(2020, 1, 1),
                    units: 100_000.0,
                    cost_basis: 80_000.0,
                }],
                contribution_limit: None,
            }),
        }],
        ..Default::default()
    }
}

fn create_monthly_events_config() -> SimulationConfig {
    let mut config = create_basic_config(30);

    // Add checking account for cash flows
    config.accounts.push(Account {
        account_id: AccountId(2),
        flavor: AccountFlavor::Bank(Cash {
            value: 5_000.0,
            return_profile_id: ReturnProfileId(1),
        }),
    });

    // Monthly income
    config.events.push(Event {
        event_id: EventId(1),
        trigger: EventTrigger::Repeating {
            interval: RepeatInterval::Monthly,
            start_condition: None,
            end_condition: None,
        },
        effects: vec![EventEffect::Income {
            to: AccountId(2),
            amount: TransferAmount::Fixed(8_000.0),
            amount_mode: finplan_core::model::AmountMode::Gross,
            income_type: IncomeType::Taxable,
        }],
        once: false,
    });

    // Monthly expense
    config.events.push(Event {
        event_id: EventId(2),
        trigger: EventTrigger::Repeating {
            interval: RepeatInterval::Monthly,
            start_condition: None,
            end_condition: None,
        },
        effects: vec![EventEffect::Expense {
            from: AccountId(2),
            amount: TransferAmount::Fixed(5_000.0),
        }],
        once: false,
    });

    config
}

fn create_account_balance_trigger_safe_config() -> SimulationConfig {
    let mut config = create_basic_config(10);

    // Add checking and savings accounts
    config.accounts.push(Account {
        account_id: AccountId(2),
        flavor: AccountFlavor::Bank(Cash {
            value: 2_000.0,
            return_profile_id: ReturnProfileId(1),
        }),
    });

    config.accounts.push(Account {
        account_id: AccountId(3),
        flavor: AccountFlavor::Bank(Cash {
            value: 20_000.0,
            return_profile_id: ReturnProfileId(1),
        }),
    });

    // Monthly expense that may overdraw checking
    config.events.push(Event {
        event_id: EventId(1),
        trigger: EventTrigger::Repeating {
            interval: RepeatInterval::Monthly,
            start_condition: None,
            end_condition: None,
        },
        effects: vec![EventEffect::Expense {
            from: AccountId(2),
            amount: TransferAmount::Fixed(500.0),
        }],
        once: false,
    });

    // Safe sweep: once: true - only triggers once per threshold crossing
    config.events.push(Event {
        event_id: EventId(2),
        trigger: EventTrigger::AccountBalance {
            account_id: AccountId(2),
            threshold: BalanceThreshold::LessThanOrEqual(1_000.0),
        },
        effects: vec![EventEffect::Income {
            to: AccountId(2),
            amount: TransferAmount::Fixed(3_000.0),
            amount_mode: finplan_core::model::AmountMode::Net,
            income_type: IncomeType::TaxFree,
        }],
        once: true, // SAFE: only fires once
    });

    config
}

fn bench_basic_simulation(c: &mut Criterion) {
    let config = create_basic_config(30);

    c.bench_function("basic_30yr_simulation", |b| {
        b.iter(|| simulate(black_box(&config), black_box(42)))
    });
}

fn bench_monthly_events(c: &mut Criterion) {
    let config = create_monthly_events_config();

    c.bench_function("monthly_events_30yr", |b| {
        b.iter(|| simulate(black_box(&config), black_box(42)))
    });
}

fn bench_account_balance_trigger(c: &mut Criterion) {
    let safe_config = create_account_balance_trigger_safe_config();
    let instrumentation = InstrumentationConfig::with_limit(100);

    c.bench_function("account_balance_safe_10yr", |b| {
        b.iter(|| {
            simulate_with_metrics(
                black_box(&safe_config),
                black_box(42),
                black_box(&instrumentation),
            )
        })
    });
}

fn bench_monte_carlo(c: &mut Criterion) {
    let mut group = c.benchmark_group("monte_carlo");
    let config = create_basic_config(30);

    for iterations in [100, 500, 1000].iter() {
        let mc_config = MonteCarloConfig {
            iterations: *iterations,
            percentiles: vec![0.05, 0.25, 0.50, 0.75, 0.95],
            compute_mean: true,
        };

        group.bench_with_input(
            BenchmarkId::new("iterations", iterations),
            iterations,
            |b, _| {
                b.iter(|| {
                    monte_carlo_simulate_with_config(black_box(&config), black_box(&mc_config))
                })
            },
        );
    }

    group.finish();
}

fn bench_instrumented_vs_normal(c: &mut Criterion) {
    let mut group = c.benchmark_group("instrumented_comparison");
    let config = create_monthly_events_config();

    group.bench_function("normal_simulate", |b| {
        b.iter(|| simulate(black_box(&config), black_box(42)))
    });

    let instrumentation = InstrumentationConfig::default();
    group.bench_function("instrumented_simulate", |b| {
        b.iter(|| {
            simulate_with_metrics(
                black_box(&config),
                black_box(42),
                black_box(&instrumentation),
            )
        })
    });

    let disabled = InstrumentationConfig::disabled();
    group.bench_function("instrumented_disabled", |b| {
        b.iter(|| simulate_with_metrics(black_box(&config), black_box(42), black_box(&disabled)))
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_basic_simulation,
    bench_monthly_events,
    bench_account_balance_trigger,
    bench_monte_carlo,
    bench_instrumented_vs_normal,
);
criterion_main!(benches);
