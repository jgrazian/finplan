//! The batch API: a Monte Carlo run is a plan of batches, and the result
//! depends on that plan alone, never on who ran the batches or in what order.
use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::error::SimulationError;
use crate::model::{
    Account, AccountFlavor, AccountId, AmountMode, AssetId, AssetLot, Cash, ConvergenceConfig,
    ConvergenceMetric, Event, EventEffect, EventId, EventTrigger, IncomeType, InflationProfile,
    InvestmentContainer, MonteCarloConfig, MonteCarloProgress, MonteCarloSummary, RepeatInterval,
    ReturnProfile, ReturnProfileId, TaxStatus, TransferAmount,
};
use crate::simulation::{
    BatchOutput, BatchSpec, MonteCarloCoordinator, base_seed, merge_batches,
    monte_carlo_simulate_with_config, monte_carlo_stats_only, prepare_run, run_batch,
};
use serde::{Serialize, de::DeserializeOwned};

/// A stochastic plan with a monthly deficit, so some paths fail the funding
/// check and every accumulator (mean, real quantiles, funding) has content.
fn plan() -> SimulationConfig {
    let monthly = |event_id: u16, effect: EventEffect| Event {
        event_id: EventId(event_id),
        trigger: EventTrigger::Repeating {
            interval: RepeatInterval::Monthly,
            start_condition: None,
            end_condition: None,
            max_occurrences: None,
        },
        effects: vec![effect],
        once: false,
    };
    SimulationConfig {
        start_date: Some(jiff::civil::date(2025, 1, 1)),
        duration_years: 12,
        birth_date: Some(jiff::civil::date(1980, 6, 15)),
        inflation_profile: InflationProfile::Normal {
            mean: 0.03,
            std_dev: 0.01,
        },
        return_profiles: HashMap::from([
            (
                ReturnProfileId(0),
                ReturnProfile::Normal {
                    mean: 0.07,
                    std_dev: 0.15,
                },
            ),
            (ReturnProfileId(1), ReturnProfile::Fixed(0.02)),
        ]),
        asset_returns: HashMap::from([(AssetId(1), ReturnProfileId(0))]),
        accounts: vec![
            Account {
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
                        units: 1_000.0,
                        cost_basis: 80_000.0,
                    }],
                    contribution_limit: None,
                }),
            },
            Account {
                account_id: AccountId(2),
                flavor: AccountFlavor::Bank(Cash {
                    value: 5_000.0,
                    return_profile_id: ReturnProfileId(1),
                }),
            },
        ],
        events: vec![
            monthly(
                1,
                EventEffect::Income {
                    to: AccountId(2),
                    amount: TransferAmount::fixed(9_000.0),
                    amount_mode: AmountMode::Gross,
                    income_type: IncomeType::Taxable,
                },
            ),
            monthly(
                2,
                EventEffect::Expense {
                    from: AccountId(2),
                    amount: TransferAmount::fixed(7_200.0),
                },
            ),
        ],
        ..Default::default()
    }
}

fn fixed_plan(parallel_batches: usize) -> MonteCarloConfig {
    MonteCarloConfig {
        iterations: 203,
        parallel_batches,
        seed: Some(7),
        ..Default::default()
    }
}

fn converging_plan() -> MonteCarloConfig {
    MonteCarloConfig {
        iterations: 40,
        batch_size: 10,
        parallel_batches: 4,
        seed: Some(11),
        convergence: Some(ConvergenceConfig {
            metric: ConvergenceMetric::Median,
            relative_threshold: 0.0001,
            max_iterations: 600,
        }),
        ..Default::default()
    }
}

/// Never converges, so the last round is cut short by `max_iterations` and
/// splits unevenly across the batches.
fn capped_plan() -> MonteCarloConfig {
    MonteCarloConfig {
        convergence: Some(ConvergenceConfig {
            metric: ConvergenceMetric::SuccessRate,
            relative_threshold: 0.0,
            max_iterations: 137,
        }),
        ..converging_plan()
    }
}

/// Converges on the running mean's standard error, which needs no history.
fn mean_plan() -> MonteCarloConfig {
    MonteCarloConfig {
        convergence: Some(ConvergenceConfig {
            metric: ConvergenceMetric::Mean,
            relative_threshold: 0.002,
            max_iterations: 600,
        }),
        ..converging_plan()
    }
}

/// Everything a summary carries, in a form that compares exactly.
fn json(summary: &MonteCarloSummary) -> serde_json::Value {
    serde_json::to_value(summary).unwrap()
}

/// First place two JSON values differ, so a failure names a field rather than
/// printing two summaries of hundreds of kilobytes.
fn first_difference(path: &str, a: &serde_json::Value, b: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            x.keys()
                .chain(y.keys())
                .find_map(|k| match (x.get(k), y.get(k)) {
                    (Some(l), Some(r)) => first_difference(&format!("{path}.{k}"), l, r),
                    _ => Some(format!("{path}.{k} present on one side only")),
                })
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .enumerate()
            .find_map(|(i, (l, r))| first_difference(&format!("{path}[{i}]"), l, r)),
        _ => (a != b).then(|| format!("{path}: {a} != {b}")),
    }
}

fn assert_same(left: &serde_json::Value, right: &serde_json::Value) {
    if let Some(difference) = first_difference("$", left, right) {
        panic!("summaries differ at {difference}");
    }
}

/// Through JSON and back, as a batch crosses the boundary to a worker.
fn wire<T: Serialize + DeserializeOwned>(value: &T) -> T {
    serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap()
}

/// Drive a plan the way a browser would: the coordinator hands out each round,
/// `workers` workers each run their share of its batches (every spec and
/// output crossing a JSON boundary), and the outputs reach the coordinator
/// newest-first. Returns the summary and how many rounds it took.
fn run_on_workers(mc: &MonteCarloConfig, workers: usize) -> (MonteCarloSummary, usize) {
    let params = plan();
    let prepared = prepare_run(&params, mc).unwrap();
    let mut coordinator = MonteCarloCoordinator::new(mc, base_seed(mc).unwrap()).unwrap();
    let mut rounds = 0;
    while let Some(round) = coordinator.next_round() {
        rounds += 1;
        let mut outputs = Vec::new();
        for worker in 0..workers {
            for spec in round.iter().filter(|s| s.index % workers == worker) {
                outputs.push(wire(&run_batch(&prepared, &wire(spec), None).unwrap()));
            }
        }
        outputs.reverse();
        coordinator.absorb(outputs).unwrap();
    }
    (coordinator.finish(&params, true).unwrap(), rounds)
}

fn assert_batches_equal_monolithic(mc: &MonteCarloConfig) -> usize {
    let whole = monte_carlo_simulate_with_config(&plan(), mc).unwrap();
    let (batched, rounds) = run_on_workers(mc, 1);
    assert_same(&json(&whole), &json(&batched));
    rounds
}

#[test]
fn fixed_plan_run_batch_by_batch_equals_the_monolithic_run() {
    for parallel_batches in [1, 4, 7] {
        let rounds = assert_batches_equal_monolithic(&fixed_plan(parallel_batches));
        assert_eq!(rounds, 1);
    }
}

#[test]
fn converging_plans_run_batch_by_batch_equal_the_monolithic_run() {
    let converged = assert_batches_equal_monolithic(&converging_plan());
    assert!(
        converged > 1,
        "should take several rounds, took {converged}"
    );

    // The ceiling cuts the last round short and splits it unevenly.
    let capped = monte_carlo_simulate_with_config(&plan(), &capped_plan()).unwrap();
    assert_eq!(capped.stats.num_iterations, 137);
    assert_eq!(capped.stats.converged, Some(false));
    assert!(assert_batches_equal_monolithic(&capped_plan()) > 2);

    assert_batches_equal_monolithic(&mean_plan());
}

#[test]
fn worker_count_does_not_change_the_result() {
    for mc in [fixed_plan(4), converging_plan()] {
        let one = json(&run_on_workers(&mc, 1).0);
        for workers in [2, 3, 8] {
            assert_eq!(
                one,
                json(&run_on_workers(&mc, workers).0),
                "{workers} workers"
            );
        }
    }
}

#[test]
fn the_batch_plan_is_an_input_and_changes_the_result() {
    let few = run_on_workers(&fixed_plan(2), 1).0;
    let many = run_on_workers(&fixed_plan(7), 1).0;
    for run in [&few, &many] {
        assert_eq!(run.stats.num_iterations, 203);
        assert!((0.0..=1.0).contains(&run.stats.success_rate));
        assert!(run.stats.mean_final_net_worth.is_finite());
        assert_eq!(run.percentile_runs.len(), 3);
    }
    // Different batch seeds draw different paths.
    assert_ne!(json(&few), json(&many));
    // But each is reproducible.
    assert_eq!(json(&many), json(&run_on_workers(&fixed_plan(7), 3).0));
}

#[test]
fn merge_batches_merges_a_one_round_plan_in_any_order() {
    let mc = fixed_plan(5);
    let params = plan();
    let prepared = prepare_run(&params, &mc).unwrap();
    let mut coordinator = MonteCarloCoordinator::new(&mc, base_seed(&mc).unwrap()).unwrap();
    let round = coordinator.next_round().unwrap();
    // The same round is offered until it is absorbed.
    assert_eq!(Some(round.clone()), coordinator.next_round());
    // 203 over 5 batches: the first 3 take one extra.
    let sizes: Vec<_> = round.iter().map(|s| s.iterations).collect();
    assert_eq!(sizes, [41, 41, 41, 40, 40]);
    assert_eq!(round[3].seed, 7 + 3);

    let mut outputs: Vec<BatchOutput> = round
        .iter()
        .map(|spec| run_batch(&prepared, spec, None).unwrap())
        .collect();
    outputs.swap(0, 4);
    outputs.swap(1, 3);
    let merged = merge_batches(&params, &mc, outputs.clone()).unwrap();
    assert_eq!(
        json(&merged),
        json(&monte_carlo_simulate_with_config(&params, &mc).unwrap())
    );

    // A batch missing from the round is refused, not summarized short.
    outputs.pop();
    assert!(matches!(
        merge_batches(&params, &mc, outputs),
        Err(SimulationError::Config(_))
    ));
}

#[test]
fn merge_batches_needs_a_coordinator_when_the_first_round_does_not_converge() {
    let mc = capped_plan();
    let params = plan();
    let prepared = prepare_run(&params, &mc).unwrap();
    let mut coordinator = MonteCarloCoordinator::new(&mc, 11).unwrap();
    let round = coordinator.next_round().unwrap();
    let outputs = round
        .iter()
        .map(|spec| run_batch(&prepared, spec, None).unwrap())
        .collect();
    assert!(matches!(
        merge_batches(&params, &mc, outputs),
        Err(SimulationError::Config(_))
    ));
}

#[test]
fn absorb_rejects_what_the_round_did_not_ask_for() {
    let mc = fixed_plan(3);
    let params = plan();
    let prepared = prepare_run(&params, &mc).unwrap();
    let mut coordinator = MonteCarloCoordinator::new(&mc, 7).unwrap();
    assert!(coordinator.absorb(Vec::new()).is_err(), "no round pending");

    let round = coordinator.next_round().unwrap();
    let mut outputs: Vec<_> = round
        .iter()
        .map(|spec| run_batch(&prepared, spec, None).unwrap())
        .collect();
    // A batch run twice stands in for one never run.
    outputs[2] = outputs[1].clone();
    assert!(coordinator.absorb(outputs).is_err());
    assert_eq!(coordinator.completed(), 0, "nothing merged");
}

#[test]
fn batch_output_round_trips_through_json_exactly() {
    let mc = fixed_plan(2);
    let prepared = prepare_run(&plan(), &mc).unwrap();
    let spec = BatchSpec {
        index: 1,
        seed: 99,
        iterations: 25,
    };
    let output = run_batch(&prepared, &spec, None).unwrap();
    assert!(output.mean_accumulators.is_some());
    assert!(output.real_accumulator.is_some());

    let text = serde_json::to_string(&output).unwrap();
    let back: BatchOutput = serde_json::from_str(&text).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), text);
    assert_eq!(back.results, output.results);
    assert_eq!(wire(&spec), spec);
}

#[test]
fn a_cancelled_batch_is_an_error_not_a_short_output() {
    let mc = fixed_plan(2);
    let prepared = prepare_run(&plan(), &mc).unwrap();
    let progress = MonteCarloProgress::new();
    progress.cancel();
    let spec = BatchSpec {
        index: 0,
        seed: 1,
        iterations: 10,
    };
    assert!(matches!(
        run_batch(&prepared, &spec, Some(&progress)),
        Err(SimulationError::Cancelled)
    ));
    assert_eq!(progress.completed(), 0);
}

#[test]
fn stats_only_batches_skip_phase_two() {
    let mc = converging_plan();
    let params = plan();
    let (expected, expected_seeds) =
        monte_carlo_stats_only(&params, &mc, &MonteCarloProgress::new()).unwrap();

    let prepared = crate::simulation::prepare_run_with(&params, &mc, false).unwrap();
    let mut coordinator = MonteCarloCoordinator::new(&mc, base_seed(&mc).unwrap()).unwrap();
    while let Some(round) = coordinator.next_round() {
        let outputs = round
            .iter()
            .map(|spec| run_batch(&prepared, spec, None).unwrap())
            .collect();
        coordinator.absorb(outputs).unwrap();
    }
    let (stats, seeds) = coordinator.finish_stats().unwrap();
    assert_eq!(seeds, expected_seeds);
    // `monte_carlo_stats_only` also drops the mean, which does not touch stats.
    assert_eq!(
        serde_json::to_value(&stats).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
}
