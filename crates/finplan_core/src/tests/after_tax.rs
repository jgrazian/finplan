//! After-tax ending balance (spec 21): final net worth less the income tax the
//! path's tax-deferred balances still owe (`deferred_exit_tax`), per path and
//! across a Monte Carlo run.

use crate::analysis::{
    AnalysisMetric, SolveConfig, SolveConstraint, SolveConstraintMetric, SolveObjective,
    SweepParameter, SweepPointData, compute_metrics, solve,
};
use crate::config::{
    AccountBuilder, AssetBuilder, EventBuilder, SimulationBuilder, SimulationConfig,
    SimulationMetadata,
};
use crate::model::{
    EventId, MonteCarloConfig, MonteCarloProgress, ReturnProfile, after_tax_final_net_worth,
    final_net_worth,
};
use crate::simulation::{monte_carlo_simulate_with_config, monte_carlo_stats_only, simulate};

/// Checking, a brokerage, a Roth and a loan; with `deferred`, a 401(k) too.
/// Flat prices and no inflation, so every balance ends where it starts. The
/// default tax config: 2024 single brackets, no deduction, 5% state.
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
    assert_eq!(result.deferred_exit_tax, 0.0);
    assert_eq!(after_tax_final_net_worth(&result), final_net_worth(&result));
}

#[test]
fn pre_tax_money_owes_its_withdrawal_tax_over_ten_years() {
    let (config, meta) = plan(true, ReturnProfile::Fixed(0.0));
    let k401 = meta.account_id("401k").unwrap();
    assert_eq!(config.tax_deferred_accounts(), vec![k401]);

    let result = simulate(&config, 7).unwrap();
    let pre_tax = final_net_worth(&result);
    // 10k + 50k + 20k + 100k - 5k.
    assert!((pre_tax - 175_000.0).abs() < 1e-6, "{pre_tax}");
    // $10k a year with no other income: 10% federal + 5% state, ten times.
    assert!(
        (result.deferred_exit_tax - 15_000.0).abs() < 1e-6,
        "{}",
        result.deferred_exit_tax
    );
    assert!((after_tax_final_net_worth(&result) - 160_000.0).abs() < 1e-6);
}

#[test]
fn the_exit_tax_follows_the_plans_brackets_deduction_and_income() {
    let tax = |config: &SimulationConfig| simulate(config, 7).unwrap().deferred_exit_tax;

    // The deduction shelters each year's $10k from federal tax: state only.
    let (mut deducted, _) = plan(true, ReturnProfile::Fixed(0.0));
    deducted.tax_config.standard_deduction = 14_600.0;
    assert!(
        (tax(&deducted) - 5_000.0).abs() < 1e-6,
        "{}",
        tax(&deducted)
    );

    // A bigger balance climbs the brackets: $100k a year reaches 22%.
    let (big, _) = SimulationBuilder::new()
        .start(2026, 1, 1)
        .years(2)
        .inflation(0.0)
        .asset(AssetBuilder::new("FUND").price(1.0).fixed_return(0.0))
        .account(AccountBuilder::traditional_401k("401k").cash(0.0))
        .position("401k", "FUND", 1_000_000.0, 1_000_000.0)
        .build();
    // 1,160 + 4,266 + 11,627 federal and 5,000 state, a year.
    assert!((tax(&big) - 220_530.0).abs() < 1e-6, "{}", tax(&big));

    // The plan's last full year of income sits under each year's share.
    let (earning, _) = SimulationBuilder::new()
        .start(2026, 1, 1)
        .years(2)
        .inflation(0.0)
        .asset(AssetBuilder::new("FUND").price(1.0).fixed_return(0.0))
        .bank("Checking", 0.0)
        .account(AccountBuilder::traditional_401k("401k").cash(0.0))
        .position("401k", "FUND", 100_000.0, 100_000.0)
        .event(
            EventBuilder::income("Salary")
                .to_account("Checking")
                .amount(5_000.0)
                .gross()
                .monthly(),
        )
        .build();
    // $60k of salary in 2027: every withdrawn dollar lands at 22%.
    assert!((tax(&earning) - 27_000.0).abs() < 1e-6, "{}", tax(&earning));
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
    assert!((after_tax - (final_net_worth(median) - median.deferred_exit_tax)).abs() < 1e-6);
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
        also: Vec::new(),
        mc_iterations: 8,
        parallel_batches: 1,
        seed: Some(7),
        tolerance: 25.0,
        max_probes: 20,
    };
    let results = solve(&plan, &config, None).unwrap();
    let best = results.best.unwrap();
    // Spending least leaves the most: 200k - 60k cash, and the 401(k)'s
    // 100k less 15k of withdrawal tax.
    assert_eq!(best.values, vec![1_000.0]);
    assert_eq!(best.after_tax_percentile(0.5), Some(best.objective_value));
    assert!((best.objective_value - 225_000.0).abs() < 1e-6, "{best:?}");
}
