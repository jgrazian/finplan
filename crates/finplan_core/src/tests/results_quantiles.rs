use super::RealAccumulator;
use crate::config::SimulationConfig;
use crate::model::*;
use crate::simulation::{monte_carlo_simulate_with_config, simulate};
use jiff::civil::date;
use rand::{RngCore, SeedableRng};
use std::collections::HashMap;

fn fixture(values: [f64; 3], inflation: f64) -> SimulationResult {
    SimulationResult {
        wealth_snapshots: [date(2026, 6, 1), date(2026, 12, 31), date(2027, 6, 1)]
            .into_iter()
            .zip(values)
            .map(|(date, value)| WealthSnapshot {
                date,
                accounts: vec![AccountSnapshot {
                    account_id: AccountId(0),
                    flavor: AccountSnapshotFlavor::Bank(value),
                }],
            })
            .collect(),
        cumulative_inflation: vec![1.0, inflation],
        yearly_taxes: vec![],
        yearly_cash_flows: vec![],
        ledger: vec![],
        warnings: vec![],
    }
}

#[test]
fn crossing_paths_real_ranks_interpolation_and_warning_paths() {
    let a = fixture([100.0, 300.0, 200.0], 4.0);
    let b = fixture([100.0, 100.0, 100.0], 1.0);
    let mut c = fixture([100.0, -50.0, 300.0], 2.0);
    c.warnings.push(SimulationWarning {
        date: date(2026, 12, 31),
        event_id: None,
        message: "shortfall".into(),
        kind: WarningKind::CashShortfall,
    });
    let mut acc = RealAccumulator::new(&a);
    acc.accumulate(&a).unwrap();
    let mut other = RealAccumulator::new(&a);
    other.accumulate(&b).unwrap();
    other.accumulate(&c).unwrap();
    acc.merge(other);
    let real = acc.finish().unwrap();
    assert_eq!(real.base_date, date(2026, 6, 1));
    assert_eq!(real.num_iterations, 3);
    for point in &real.points {
        assert!(point.p5 <= point.p50 && point.p50 <= point.p95);
        assert!(point.p10 <= point.p25 && point.p25 <= point.p50);
        assert!(point.p50 <= point.p75 && point.p75 <= point.p90);
    }
    assert_eq!(real.points[1].p5, -35.0);
    assert_eq!(real.points[1].p50, 100.0);
    assert!((real.points[1].p95 - 280.0).abs() < 1e-10);
    let last = real.points.last().unwrap();
    assert_eq!((last.p5, last.p50, last.p95), (55.0, 100.0, 145.0));
    assert_eq!((last.p25, last.p75), (75.0, 125.0));
    assert_eq!(
        (real.terminal.min, real.terminal.mean, real.terminal.max),
        (50.0, 100.0, 150.0)
    );
    assert!((real.terminal.std_dev - (5000.0_f64 / 3.0).sqrt()).abs() < 1e-10);
    // Nominal median is A, but real median is B. Dividing nominal mean by A's
    // inflation would give 50, not 100. Neither approximation is permissible.
    assert_ne!(last.p50, final_net_worth(&a) / 4.0);
}

#[test]
fn singleton_duplicate_terminal_and_invalid_observations() {
    let mut path = fixture([100.0, 100.0, -20.0], 2.0);
    let terminal = path.wealth_snapshots.last().unwrap().clone();
    path.wealth_snapshots.last_mut().unwrap().accounts[0].flavor =
        AccountSnapshotFlavor::Bank(999.0);
    path.wealth_snapshots.push(terminal);
    let mut acc = RealAccumulator::new(&path);
    acc.accumulate(&path).unwrap();
    let real = acc.finish().unwrap();
    assert_eq!(real.points.len(), 3);
    assert_eq!(real.terminal.std_dev, 0.0);
    assert_eq!(
        (real.points[2].p5, real.points[2].p50, real.points[2].p95),
        (-10.0, -10.0, -10.0)
    );
    for factor in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let bad = fixture([100.0, 0.0, 1.0], factor);
        assert!(RealAccumulator::new(&bad).accumulate(&bad).is_err());
    }
    let bad = fixture([100.0, f64::NAN, 1.0], 1.0);
    assert!(RealAccumulator::new(&bad).accumulate(&bad).is_err());
    let mut missing = path.clone();
    missing.wealth_snapshots.remove(0);
    assert!(RealAccumulator::new(&path).accumulate(&missing).is_err());
}

#[test]
fn stochastic_inflation_aggregates_every_iteration_before_selecting_paths() {
    let params = SimulationConfig {
        start_date: Some(date(2026, 6, 1)),
        duration_years: 4,
        accounts: vec![Account {
            account_id: AccountId(0),
            flavor: AccountFlavor::Bank(Cash {
                value: 100.0,
                return_profile_id: ReturnProfileId(0),
            }),
        }],
        return_profiles: HashMap::from([(
            ReturnProfileId(0),
            ReturnProfile::Normal {
                mean: 0.07,
                std_dev: 0.2,
            },
        )]),
        inflation_profile: InflationProfile::Normal {
            mean: 0.1,
            std_dev: 0.2,
        },
        ..Default::default()
    };
    let config = MonteCarloConfig {
        iterations: 40,
        parallel_batches: 1,
        seed: Some(42),
        compute_mean: false,
        ..Default::default()
    };
    let summary = monte_carlo_simulate_with_config(&params, &config).unwrap();
    let real = summary.real_net_worth.unwrap();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);
    let paths: Vec<_> = (0..config.iterations)
        .map(|_| simulate(&params, rng.next_u64()).unwrap())
        .collect();
    assert_eq!(real.num_iterations, paths.len());
    for point in &real.points {
        let mut observations: Vec<_> = paths
            .iter()
            .map(|path| {
                let snapshot = path
                    .wealth_snapshots
                    .iter()
                    .rev()
                    .find(|s| s.date == point.date)
                    .unwrap();
                let nominal: f64 = snapshot
                    .accounts
                    .iter()
                    .map(AccountSnapshot::total_value)
                    .sum();
                nominal / path.cumulative_inflation[(point.date.year() - 2026) as usize]
            })
            .collect();
        observations.sort_by(f64::total_cmp);
        for (p, measured) in [(0.05, point.p5), (0.5, point.p50), (0.95, point.p95)] {
            let h = (observations.len() - 1) as f64 * p;
            let lo = h.floor() as usize;
            let expected =
                observations[lo] + (observations[h.ceil() as usize] - observations[lo]) * h.fract();
            assert!((measured - expected).abs() < 1e-10);
        }
    }
    let mut nominal: Vec<_> = paths.iter().collect();
    nominal.sort_by(|a, b| final_net_worth(a).total_cmp(&final_net_worth(b)));
    for (p, path) in &summary.percentile_runs {
        assert_eq!(
            final_net_worth(path),
            final_net_worth(nominal[(paths.len() as f64 * p).floor() as usize])
        );
    }
    let real_terminals: Vec<_> = paths
        .iter()
        .map(|p| final_net_worth(p) / p.cumulative_inflation[4])
        .collect();
    let mean = real_terminals.iter().sum::<f64>() / paths.len() as f64;
    assert!((real.terminal.mean - mean).abs() < 1e-10);
    let nominal_median = nominal[paths.len() / 2];
    assert!(
        (real.points.last().unwrap().p50
            - final_net_worth(nominal_median) / nominal_median.cumulative_inflation[4])
            .abs()
            > 1e-3
    );
}

#[test]
fn invalid_mc_observations_fail_instead_of_retrying_or_dropping() {
    let mut params = SimulationConfig::default();
    params.accounts.push(Account {
        account_id: AccountId(0),
        flavor: AccountFlavor::Bank(Cash {
            value: f64::NAN,
            return_profile_id: ReturnProfileId(0),
        }),
    });
    params
        .return_profiles
        .insert(ReturnProfileId(0), ReturnProfile::Fixed(0.0));
    let config = MonteCarloConfig {
        iterations: 1,
        ..Default::default()
    };
    assert!(monte_carlo_simulate_with_config(&params, &config).is_err());
}
