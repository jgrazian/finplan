//! After-tax ending balance (spec 21): tax-deferred balances valued net of the
//! plan's `deferred_tax_rate`, per path and across a Monte Carlo run.

use crate::analysis::{
    AnalysisMetric, SolveConfig, SolveConstraint, SolveConstraintMetric, SolveObjective,
    SweepParameter, SweepPointData, compute_metrics, solve,
};
use crate::config::{
    AccountBuilder, AssetBuilder, DEFAULT_DEFERRED_TAX_RATE, EventBuilder, SimulationBuilder,
    SimulationConfig, SimulationMetadata,
};
use crate::model::{EventId, MonteCarloConfig, MonteCarloProgress, ReturnProfile, final_net_worth};
use crate::simulation::{monte_carlo_simulate_with_config, monte_carlo_stats_only, simulate};

/// Checking, a brokerage, a Roth and a loan; with `deferred`, a 401(k) too.
/// Flat prices and no inflation, so every balance ends where it starts.
fn plan(deferred: bool, profile: ReturnProfile) -> (SimulationConfig, SimulationMetadata) {
    let mut builder = SimulationBuilder::new()
        .start(2026, 1, 1)
        .years(2)
        .inflation(0.0)
        .asset(AssetBuilder::new("FUND").price(1.0).return_profile(profile))
        .bank("Checking", 10_000.0)
        .account(AccountBuilder::taxable_brokerage("Brokerage").cash(0.0))
        .account(AccountBuilder::roth_ira("Roth").cash(0.0))
        .account(AccountBuilder::loan("Car", 5_000.0, 0.0))
        .position("Brokerage", "FUND", 50_000.0, 50_000.0)
        .position("Roth", "FUND", 20_000.0, 20_000.0);
    if deferred {
        builder = builder
            .account(AccountBuilder::traditional_401k("401k").cash(0.0))
            .position("401k", "FUND", 100_000.0, 100_000.0);
    }
    builder.build()
}

#[test]
fn without_tax_deferred_money_after_tax_is_pre_tax() {
    let (config, _) = plan(false, ReturnProfile::Fixed(0.0));
    assert!(config.tax_deferred_accounts().is_empty());
    let result = simulate(&config, 7).unwrap();
    assert_eq!(
        config.after_tax_final_net_worth(&result),
        final_net_worth(&result)
    );
}

#[test]
fn tax_deferred_money_counts_net_of_the_rate() {
    let (mut config, meta) = plan(true, ReturnProfile::Fixed(0.0));
    let k401 = meta.account_id("401k").unwrap();
    assert_eq!(config.tax_deferred_accounts(), vec![k401]);
    assert_eq!(config.deferred_tax_rate, DEFAULT_DEFERRED_TAX_RATE);

    let result = simulate(&config, 7).unwrap();
    let pre_tax = final_net_worth(&result);
    let deferred = result.final_account_balance(k401).unwrap();
    // 10k + 50k + 20k + 100k - 5k.
    assert!((pre_tax - 175_000.0).abs() < 1e-6, "{pre_tax}");
    assert!((deferred - 100_000.0).abs() < 1e-6, "{deferred}");
    let after_tax = config.after_tax_final_net_worth(&result);
    assert!((after_tax - (pre_tax - 0.24 * deferred)).abs() < 1e-6);
    assert!((after_tax - 151_000.0).abs() < 1e-6, "{after_tax}");

    config.deferred_tax_rate = 0.0;
    assert_eq!(config.after_tax_final_net_worth(&result), pre_tax);
}

#[test]
fn monte_carlo_ranks_after_tax_values_on_their_own() {
    let mc = MonteCarloConfig {
        iterations: 200,
        percentiles: vec![0.05, 0.5, 0.95],
        seed: Some(11),
        ..Default::default()
    };
    let volatile = ReturnProfile::Normal {
        mean: 0.05,
        std_dev: 0.15,
    };

    // No tax-deferred money: the two rankings are the same numbers.
    let (config, _) = plan(false, volatile.clone());
    let summary = monte_carlo_simulate_with_config(&config, &mc).unwrap();
    assert_eq!(
        summary.stats.after_tax_percentile_values,
        summary.stats.percentile_values
    );

    // One fund everywhere, so every path's after-tax value is the same
    // increasing function of its pre-tax value: each percentile maps across.
    let (config, meta) = plan(true, volatile);
    let summary = monte_carlo_simulate_with_config(&config, &mc).unwrap();
    assert_eq!(summary.stats.after_tax_percentile_values.len(), 3);
    // The path that ends at the median, not the drawn one that tracks its band.
    let (_, seeds) = monte_carlo_stats_only(&config, &mc, &MonteCarloProgress::new()).unwrap();
    let median = &simulate(&config, seeds[1].1).unwrap();
    let deferred = median
        .final_account_balance(meta.account_id("401k").unwrap())
        .unwrap();
    let after_tax = summary.stats.after_tax_percentile(0.5).unwrap();
    assert!(deferred > 0.0);
    assert!((after_tax - (final_net_worth(median) - 0.24 * deferred)).abs() < 1e-6);
    assert!(after_tax < summary.stats.percentile_values[1].1);

    // The analysis metric reads the same median (no inflation: real = nominal).
    let point = SweepPointData::from_summary(&summary, 1970);
    assert_eq!(
        point.compute_metric(&AnalysisMetric::AfterTaxEndingBalance, 1970),
        after_tax
    );
    let computed = compute_metrics(&summary, &[AnalysisMetric::AfterTaxEndingBalance], 1970);
    assert_eq!(computed.after_tax_ending_balance, Some(after_tax));
}

#[test]
fn solve_can_maximise_the_after_tax_median() {
    let (plan, _) = SimulationBuilder::new()
        .start(2026, 1, 1)
        .years(5)
        .inflation(0.0)
        .asset(AssetBuilder::new("FUND").price(1.0).fixed_return(0.0))
        .bank("Cash", 200_000.0)
        .account(AccountBuilder::traditional_401k("401k").cash(0.0))
        .position("401k", "FUND", 100_000.0, 100_000.0)
        .event(
            EventBuilder::expense("Living")
                .from_account("Cash")
                .amount(1_000.0)
                .monthly(),
        )
        .build();
    let config = SolveConfig {
        parameters: vec![SweepParameter::effect_value(
            EventId(0),
            1_000.0,
            3_000.0,
            3,
        )],
        objective: SolveObjective::MaxMedianAfterTax,
        constraint: SolveConstraint {
            metric: SolveConstraintMetric::SuccessRate,
            min_value: 0.95,
        },
        mc_iterations: 8,
        parallel_batches: 1,
        seed: Some(7),
        tolerance: 25.0,
        max_probes: 20,
    };
    let results = solve(&plan, &config, None).unwrap();
    let best = results.best.unwrap();
    // Spending least leaves the most: 200k - 60k cash + 76k for the 401(k).
    assert_eq!(best.values, vec![1_000.0]);
    assert_eq!(best.after_tax_percentile(0.5), Some(best.objective_value));
    assert!((best.objective_value - 216_000.0).abs() < 1e-6, "{best:?}");
}
