//! Goal-seek tests.
//!
//! Every plan here is deterministic — a fixed return, a fixed inflation and a
//! seeded Monte Carlo — so success is a step function of the swept parameter
//! and the answer the bisection converges on is one an arithmetic check can
//! confirm independently.

use crate::analysis::{
    SolveConfig, SolveConstraint, SolveConstraintMetric, SolveMethod, SolveObjective,
    SweepParameter, solve,
};
use crate::config::event_builder::EventBuilder;
use crate::config::{SimulationBuilder, SimulationConfig};
use crate::model::{EventId, MonteCarloConfig};
use crate::simulation::monte_carlo_simulate_with_config;
use crate::tests::snapshots::market_dependent_plan;

/// A pot of cash drawn down by one monthly expense, with no market noise.
///
/// Ten years of withdrawals against a $1.2M balance earning nothing puts the
/// break-even spend at $10,000/month, which every assertion below is measured
/// against.
fn drawdown_plan(monthly_spend: f64) -> SimulationConfig {
    let (config, _) = SimulationBuilder::new()
        .start(2020, 1, 1)
        .years(10)
        .inflation(0.0)
        .bank("Cash", 1_200_000.0)
        .event(
            EventBuilder::expense("Living expenses")
                .from_account("Cash")
                .amount(monthly_spend)
                .monthly(),
        )
        .build();
    config
}

/// The single swept parameter: that expense's amount.
fn spend_parameter(min: f64, max: f64, steps: usize) -> SweepParameter {
    SweepParameter::effect_value(EventId(0), min, max, steps)
}

fn config_for(objective: SolveObjective, parameters: Vec<SweepParameter>) -> SolveConfig {
    SolveConfig {
        parameters,
        objective,
        constraint: SolveConstraint {
            metric: SolveConstraintMetric::SuccessRate,
            min_value: 0.95,
        },
        mc_iterations: 8,
        parallel_batches: 1,
        seed: Some(7),
        tolerance: 25.0,
        max_probes: 20,
    }
}

#[test]
fn bisection_finds_the_largest_spend_that_still_succeeds() {
    let plan = drawdown_plan(6_000.0);
    let config = config_for(
        SolveObjective::MaxParameter,
        vec![spend_parameter(2_000.0, 20_000.0, 6)],
    );

    let results = solve(&plan, &config, None).expect("solve");
    assert_eq!(results.method, SolveMethod::Bisection);

    let best = results.best.expect("a feasible spend exists in 2k..20k");
    let answer = best.values[0];
    // $1.2M over 120 months, to the tolerance the bisection was given.
    assert!(
        (answer - 10_000.0).abs() <= 25.0,
        "expected ~$10,000/month, got {answer}"
    );
    assert!(best.feasible);

    // The answer is the boundary, so a step past it must fail.
    let over = drawdown_plan(answer + 200.0);
    let mc = MonteCarloConfig {
        iterations: 8,
        seed: Some(7),
        ..Default::default()
    };
    let beyond = monte_carlo_simulate_with_config(&over, &mc).expect("simulate");
    assert!(beyond.stats.success_rate < 0.95);
}

#[test]
fn bisection_brackets_narrow_towards_the_answer() {
    let plan = drawdown_plan(6_000.0);
    let config = config_for(
        SolveObjective::MaxParameter,
        vec![spend_parameter(2_000.0, 20_000.0, 6)],
    );

    let results = solve(&plan, &config, None).expect("solve");
    let widths: Vec<f64> = results
        .probes
        .iter()
        .filter_map(|p| p.bracket)
        .map(|(lo, hi)| hi - lo)
        .collect();

    assert!(widths.len() >= 3, "expected several bracketed probes");
    for pair in widths.windows(2) {
        assert!(
            pair[1] <= pair[0] + f64::EPSILON,
            "bracket widened: {pair:?}"
        );
    }
    // Every probe is on one side of the answer, and both sides are represented.
    assert!(results.probes.iter().any(|p| p.feasible));
    assert!(results.probes.iter().any(|p| !p.feasible));
}

#[test]
fn a_range_that_always_succeeds_answers_with_its_own_end() {
    let plan = drawdown_plan(6_000.0);
    let config = config_for(
        SolveObjective::MaxParameter,
        vec![spend_parameter(1_000.0, 4_000.0, 6)],
    );

    let results = solve(&plan, &config, None).expect("solve");
    let best = results.best.expect("the whole range clears the constraint");
    assert!((best.values[0] - 4_000.0).abs() < f64::EPSILON);
    // Nothing to bracket, so the search stops after the one probe it needed.
    assert_eq!(results.probes.len(), 1);
}

#[test]
fn a_range_that_never_succeeds_reports_no_answer() {
    let plan = drawdown_plan(6_000.0);
    let config = config_for(
        SolveObjective::MaxParameter,
        vec![spend_parameter(30_000.0, 60_000.0, 6)],
    );

    let results = solve(&plan, &config, None).expect("solve");
    assert!(results.best.is_none());
    assert!(results.probes.iter().all(|p| !p.feasible));
    // Both ends were still probed, so the caller can say how far off it was.
    assert_eq!(results.probes.len(), 2);
}

#[test]
fn minimising_the_parameter_walks_in_from_the_other_end() {
    // Success rises as the *contribution* falls, so the smallest feasible value
    // is the one at the bottom of the range: the constraint never binds.
    let plan = drawdown_plan(6_000.0);
    let config = config_for(
        SolveObjective::MinParameter,
        vec![spend_parameter(2_000.0, 20_000.0, 6)],
    );

    let results = solve(&plan, &config, None).expect("solve");
    let best = results.best.expect("the low end clears the constraint");
    assert!((best.values[0] - 2_000.0).abs() < f64::EPSILON);
}

#[test]
fn two_parameters_fall_back_to_a_grid_search() {
    let (plan, _) = SimulationBuilder::new()
        .start(2020, 1, 1)
        .years(10)
        .inflation(0.0)
        .bank("Cash", 1_200_000.0)
        .event(
            EventBuilder::expense("Living expenses")
                .from_account("Cash")
                .amount(6_000.0)
                .monthly(),
        )
        .event(
            EventBuilder::expense("Travel")
                .from_account("Cash")
                .amount(1_000.0)
                .monthly(),
        )
        .build();

    let config = config_for(
        SolveObjective::MaxParameter,
        vec![
            SweepParameter::effect_value(EventId(0), 2_000.0, 8_000.0, 3),
            SweepParameter::effect_value(EventId(1), 500.0, 2_000.0, 3),
        ],
    );

    let results = solve(&plan, &config, None).expect("solve");
    assert_eq!(results.method, SolveMethod::GridSearch);
    assert_eq!(results.probes.len(), 9);

    let best = results.best.expect("some combination is affordable");
    assert!(best.feasible);
    // The objective is the first parameter, so no feasible probe beats it there.
    for probe in results.probes.iter().filter(|p| p.feasible) {
        assert!(probe.values[0] <= best.values[0] + f64::EPSILON);
    }
}

#[test]
fn an_empty_range_is_refused_rather_than_searched() {
    let plan = drawdown_plan(6_000.0);
    let config = config_for(
        SolveObjective::MaxParameter,
        vec![spend_parameter(5_000.0, 5_000.0, 6)],
    );
    assert!(solve(&plan, &config, None).is_err());
}

#[test]
fn the_baseline_describes_the_unmodified_plan() {
    let plan = drawdown_plan(6_000.0);
    let config = config_for(
        SolveObjective::MaxParameter,
        vec![spend_parameter(2_000.0, 20_000.0, 6)],
    );

    let results = solve(&plan, &config, None).expect("solve");
    let mc = MonteCarloConfig {
        iterations: config.mc_iterations,
        percentiles: vec![0.05, 0.25, 0.50, 0.75, 0.95],
        compute_mean: false,
        parallel_batches: 1,
        seed: Some(7),
        ..Default::default()
    };
    let direct = monte_carlo_simulate_with_config(&plan, &mc).expect("simulate");
    assert!((results.baseline.success_rate - direct.stats.success_rate).abs() < 1e-9);
    assert!(results.baseline.values.is_empty());
}

#[test]
fn the_error_on_the_answer_shrinks_with_the_sample() {
    let plan = drawdown_plan(6_000.0);
    let mut config = config_for(
        SolveObjective::MaxParameter,
        vec![spend_parameter(2_000.0, 20_000.0, 6)],
    );
    config.mc_iterations = 4;
    let coarse = solve(&plan, &config, None).expect("solve");
    config.mc_iterations = 64;
    let fine = solve(&plan, &config, None).expect("solve");

    match (coarse.constraint_std_error(), fine.constraint_std_error()) {
        (Some(a), Some(b)) => assert!(b <= a, "error grew with the sample: {a} then {b}"),
        // A deterministic plan can land on success = 1.0, whose error is zero
        // at any sample size; that is still the monotone result.
        other => panic!("both solves should report an error: {other:?}"),
    }
}

#[test]
fn a_plan_whose_schedule_follows_the_market_can_be_solved() {
    // Solving reads the first Monte Carlo pass only. This plan's event schedule
    // follows the market, which used to be enough to make the second pass — and
    // with it anything built on `monte_carlo_simulate_with_config` — refuse the
    // plan outright. See `tests::snapshots`.
    let plan = market_dependent_plan();
    let mut config = config_for(
        SolveObjective::MaxParameter,
        vec![SweepParameter::effect_value(
            EventId(0),
            1_000.0,
            12_000.0,
            6,
        )],
    );
    config.mc_iterations = 32;
    config.tolerance = 100.0;

    let results = solve(&plan, &config, None).expect("solve");
    let best = results.best.expect("some spend is affordable");
    assert!(best.feasible);
    assert!(best.values[0] > 1_000.0 && best.values[0] <= 12_000.0);
    // The probes carry the terminal percentiles the first pass produces.
    assert!(
        results
            .probes
            .iter()
            .all(|p| !p.final_percentiles.is_empty())
    );
}
