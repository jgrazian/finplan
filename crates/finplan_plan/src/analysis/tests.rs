//! Each analysis kind, run against a deterministic sequential runner on the
//! anonymized default plan (never a personal fixture), with the plan given
//! the two named parameters the analyses vary.

use finplan_core::analysis::{McRunner, StatsRun};
use finplan_core::config::SimulationConfig;
use finplan_core::error::SimulationError;
use finplan_core::model::{MonteCarloConfig, MonteCarloProgress, MonteCarloSummary};
use finplan_core::simulation::{monte_carlo_simulate_with_progress, monte_carlo_stats_only};
use serde_json::json;

use super::*;
use crate::edit::{self, EditOp};
use crate::graph::ScenarioGraph;
use crate::what_if::{self, WhatIfLayer};

/// Runs every simulation one after another on the calling thread, counting
/// what it was asked for and optionally cancelling after a number of runs.
#[derive(Default)]
struct Sequential {
    stats_runs: usize,
    summary_runs: usize,
    begun: Vec<usize>,
    cancel_after: Option<usize>,
}

impl McRunner for Sequential {
    fn stats(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<StatsRun, SimulationError> {
        self.stats_runs += 1;
        monte_carlo_stats_only(config, mc, &MonteCarloProgress::default())
    }

    fn summary(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<MonteCarloSummary, SimulationError> {
        self.summary_runs += 1;
        monte_carlo_simulate_with_progress(config, mc, &MonteCarloProgress::default())
    }

    fn cancelled(&self) -> bool {
        self.cancel_after
            .is_some_and(|n| self.stats_runs + self.summary_runs >= n)
    }

    fn begin(&mut self, total_iterations: usize) {
        self.begun.push(total_iterations);
    }
}

const LIMITS: Limits = Limits {
    iteration_cap: 1_000,
    parallel_batches: 1,
};

/// The default plan, shortened so a grid of runs stays quick, with a spending
/// amount and a retirement age both named and used by an event.
fn plan() -> ScenarioGraph {
    let mut graph: ScenarioGraph =
        serde_json::from_str(include_str!("../../testdata/default_snapshot.json")).unwrap();
    graph.scenario.duration_years = 15;
    let mut make = |op: serde_json::Value| {
        let op: EditOp = serde_json::from_value(op).unwrap();
        edit::apply(&mut graph, &op).unwrap().id.unwrap_or_default()
    };
    make(json!({"op": "create_parameter", "body": {
        "name": "Spending", "value": {"kind": "Money", "value": 2000.0}}}));
    let retire = make(json!({"op": "create_parameter", "body": {
        "name": "Retirement age", "value": {"kind": "Age", "years": 45, "months": 0}}}));
    make(json!({"op": "create_event", "body": {
        "name": "Retire", "fires_once": true, "enabled": true,
        "trigger": {"kind": "AgeParameter", "parameter_id": retire},
        "effects": [{"kind": "Expense", "from_account_id": 6,
            "amount": {"kind": "Expression", "source": "$Spending"}}]}}));
    graph
}

fn request(value: serde_json::Value) -> CreateAnalysis {
    serde_json::from_value(value).unwrap()
}

fn parameter_id(graph: &ScenarioGraph, name: &str) -> String {
    discover(graph)
        .unwrap()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
        .id
}

fn as_json(outcome: &AnalysisOutcome) -> serde_json::Value {
    serde_json::to_value(outcome).unwrap()
}

#[test]
fn discovery_lists_each_named_parameter_with_a_default_range() {
    let params = discover(&plan()).unwrap();
    assert_eq!(params.len(), 2);
    let spending = params.iter().find(|p| p.name == "Spending").unwrap();
    assert_eq!(spending.kind, "amount");
    assert_eq!(
        (spending.current, spending.min, spending.max),
        (2000.0, 1000.0, 3000.0)
    );
    let age = params.iter().find(|p| p.name == "Retirement age").unwrap();
    assert_eq!(age.kind, "age");
    assert_eq!((age.current, age.min, age.max), (45.0, 35.0, 55.0));
    assert!(
        params
            .windows(2)
            .all(|w| w[0].parameter_id < w[1].parameter_id)
    );
    assert!(spending.id.starts_with("parameter:"));
}

#[test]
fn a_plan_without_parameters_cannot_be_swept_but_can_be_what_ifed() {
    let mut graph: ScenarioGraph =
        serde_json::from_str(include_str!("../../testdata/default_snapshot.json")).unwrap();
    graph.scenario.duration_years = 10;
    assert!(discover(&graph).unwrap().is_empty());
    let refused = prepare(&graph, request(json!({"kind": "sensitivity"})), &LIMITS)
        .err()
        .unwrap();
    assert!(
        matches!(refused, crate::PlanError::Unprocessable(_)),
        "{refused}"
    );
    let ok = prepare(
        &graph,
        request(json!({"kind": "what-if", "layers": []})),
        &LIMITS,
    );
    assert!(ok.is_ok());
}

#[test]
fn sweep_grid_covers_every_combination_in_row_major_order() {
    let graph = plan();
    let spend = parameter_id(&graph, "Spending");
    let age = parameter_id(&graph, "Retirement age");
    let body = || {
        request(json!({"kind": "sweep", "iterations": 25, "axes": [
            {"parameter_id": spend, "min": 1000.0, "max": 3000.0, "steps": 3},
            {"parameter_id": age, "min": 44.0, "max": 46.0, "steps": 3}]}))
    };
    let mut runner = Sequential::default();
    let outcome = analyze(&graph, body(), &LIMITS, &mut runner).unwrap();
    let AnalysisOutcome::Sweep(sweep) = &outcome else {
        panic!("expected a sweep");
    };
    assert_eq!(sweep.axes.len(), 2);
    assert_eq!(sweep.axes[0].values, vec![1000.0, 2000.0, 3000.0]);
    assert_eq!(sweep.axes[1].values, vec![44.0, 45.0, 46.0]);
    assert_eq!(sweep.axes[0].kind, "amount");
    assert_eq!(sweep.axes[1].kind, "age");
    assert_eq!(sweep.cells.len(), 9);
    let order: Vec<_> = sweep.cells.iter().map(|c| c.indices.clone()).collect();
    assert_eq!(order[0], vec![0, 0]);
    assert_eq!(order[1], vec![0, 1], "the last axis varies fastest");
    assert_eq!(order[8], vec![2, 2]);
    assert_eq!(sweep.plan_indices, Some(vec![1, 1]));
    assert_eq!(sweep.iterations, 25);
    assert_eq!(sweep.default_metric.as_deref(), Some("funding"));
    // Nine cells and the plan, each a stats-only run, and nothing else.
    assert_eq!((runner.stats_runs, runner.summary_runs), (10, 0));
    assert_eq!(runner.begun, vec![9 * 25]);

    // The plan's own cell is the plan measured separately, same seed.
    let middle = &sweep.cells[4];
    assert_eq!(middle.indices, vec![1, 1]);
    assert_eq!(middle.point.success_rate, sweep.plan.success_rate);
    assert_eq!(middle.point.p50, sweep.plan.p50);

    // Same question, same seed, same answer.
    let again = analyze(&graph, body(), &LIMITS, &mut Sequential::default()).unwrap();
    assert_eq!(as_json(&outcome), as_json(&again));
}

#[test]
fn sweep_is_bit_identical_to_the_engines_own_grid_runner() {
    use finplan_core::analysis::sweep_simulate_lazy;
    let graph = plan();
    let spend = parameter_id(&graph, "Spending");
    let Prepared { base, spec, .. } = prepare(
        &graph,
        request(json!({"kind": "sweep", "iterations": 25, "axes": [
            {"parameter_id": spend, "min": 1000.0, "max": 3000.0, "steps": 3}]})),
        &LIMITS,
    )
    .unwrap();
    let AnalysisSpec::Sweep { config, .. } = &spec else {
        panic!("expected a sweep spec");
    };
    let grid = sweep_simulate_lazy(&base, config, None).unwrap();
    let AnalysisOutcome::Sweep(sweep) = run(&base, &spec, &mut Sequential::default()).unwrap()
    else {
        panic!("expected a sweep");
    };
    for (i, cell) in sweep.cells.iter().enumerate() {
        let stats = grid.get_stats(&[i]).unwrap();
        assert_eq!(cell.point.success_rate, stats.success_rate);
        assert_eq!(cell.point.funding_success_rate, stats.funding_success_rate);
    }
}

#[test]
fn sensitivity_ranks_the_biggest_mover_first() {
    let graph = plan();
    let body = || request(json!({"kind": "sensitivity", "iterations": 25, "fraction": 0.5}));
    let mut runner = Sequential::default();
    let outcome = analyze(&graph, body(), &LIMITS, &mut runner).unwrap();
    let AnalysisOutcome::Sensitivity(result) = &outcome else {
        panic!("expected a sensitivity");
    };
    assert_eq!(result.fraction, 0.5);
    assert_eq!(result.iterations, 25);
    assert!(!result.rows.is_empty());
    assert!(result.rows.windows(2).all(|w| w[0].span >= w[1].span));
    // Two simulations a ranked parameter, and the plan.
    assert_eq!(runner.stats_runs, result.rows.len() * 2 + 1);
    for row in &result.rows {
        assert!(row.low_value < row.high_value);
        assert_eq!(
            row.span,
            (row.high.success_rate - row.low.success_rate).abs() * 100.0
        );
    }
    let age = result
        .rows
        .iter()
        .find(|r| r.label == "Retirement age")
        .unwrap();
    assert_eq!(
        (age.low_value, age.high_value),
        (40.0, 50.0),
        "ages move by five years"
    );
    let again = analyze(&graph, body(), &LIMITS, &mut Sequential::default()).unwrap();
    assert_eq!(as_json(&outcome), as_json(&again));
}

#[test]
fn solving_one_amount_bisects_and_reports_its_probes() {
    let graph = plan();
    let spend = parameter_id(&graph, "Spending");
    let body = |min_value: f64| {
        request(
            json!({"kind": "solve", "iterations": 25, "objective": "max-parameter",
            "constraint": "success-rate", "min_value": min_value,
            "vary": [{"parameter_id": spend, "min": 0.0, "max": 100_000_000.0}]}),
        )
    };
    // A threshold nothing fails: the top of the range is the answer.
    let AnalysisOutcome::Solve(easy) =
        analyze(&graph, body(0.0), &LIMITS, &mut Sequential::default()).unwrap()
    else {
        panic!("expected a solve");
    };
    assert_eq!(easy.method, "bisection");
    assert_eq!(easy.constraint, "success-rate");
    assert_eq!(easy.steps.len(), 1);
    assert_eq!(easy.best.as_ref().unwrap().values, vec![100_000_000.0]);
    assert_eq!(easy.parameters.len(), 1);

    // A threshold only a mild spend meets: the bracket halves toward it.
    let mut runner = Sequential::default();
    let AnalysisOutcome::Solve(hard) = analyze(&graph, body(1.0), &LIMITS, &mut runner).unwrap()
    else {
        panic!("expected a solve");
    };
    assert!(hard.steps.len() > 2, "{} probes", hard.steps.len());
    assert_eq!(
        runner.stats_runs,
        hard.steps.len() + 1,
        "the probes and the plan"
    );
    assert!(hard.steps.iter().all(|s| s.bracket_low.is_some()));
    if let Some(best) = &hard.best {
        assert!(best.feasible);
        assert!(best.values[0] < 100_000_000.0);
    }
    assert!(hard.iterations == 25);
}

#[test]
fn solving_an_age_searches_a_grid_and_answers_in_years() {
    let graph = plan();
    let age = parameter_id(&graph, "Retirement age");
    let AnalysisOutcome::Solve(solved) = analyze(
        &graph,
        request(
            json!({"kind": "solve", "iterations": 25, "objective": "min-parameter",
            "constraint": "success-rate", "min_value": 0.0,
            "vary": [{"parameter_id": age, "min": 40.0, "max": 50.0, "steps": 3}]}),
        ),
        &LIMITS,
        &mut Sequential::default(),
    )
    .unwrap() else {
        panic!("expected a solve");
    };
    assert_eq!(solved.method, "grid-search");
    assert_eq!(solved.steps.len(), 3);
    let values: Vec<f64> = solved.steps.iter().map(|s| s.values[0]).collect();
    assert_eq!(values, vec![40.0, 45.0, 50.0]);
    assert_eq!(solved.best.as_ref().unwrap().values, vec![40.0]);
    assert!(solved.steps.iter().all(|s| s.bracket_low.is_none()));
}

#[test]
fn what_if_steps_are_cumulative_and_share_one_seed() {
    let graph = plan();
    let layers = json!([
        {"kind": "one-off", "age": 38, "amount": -60_000.0, "account_id": null},
        {"kind": "market-shock", "age": 40, "drop": 0.85}
    ]);
    let mut runner = Sequential::default();
    let outcome = analyze(
        &graph,
        request(json!({"kind": "what-if", "layers": layers, "iterations": 75})),
        &LIMITS,
        &mut runner,
    )
    .unwrap();
    let AnalysisOutcome::WhatIf(result) = &outcome else {
        panic!("expected a what-if");
    };
    assert_eq!(result.steps.len(), 3);
    assert_eq!((runner.stats_runs, runner.summary_runs), (0, 3));
    let success = |i: usize| result.steps[i].point.success_rate;
    assert!(success(1) <= success(0));
    assert!(success(2) <= success(1));
    assert!(
        result.steps[2].median_end_real < result.steps[0].median_end_real,
        "a crash and a cost lower the median"
    );
    assert_eq!(result.years.len(), result.plan_fan.p50.len());
    assert_eq!(result.years.len(), result.what_if_fan.p50.len());
    let ages = result.ages.as_ref().unwrap();
    assert!((ages[0] - (result.years[0] - 1996.0)).abs() < 1e-9);
    assert_eq!(result.plan_retirement_age, Some(45.0));
    assert_eq!(result.what_if_retirement_age, Some(45.0));

    // No layers: one step, which is the first step of the stack, as both run
    // the same seed and the same per-step iterations.
    let none = analyze(
        &graph,
        request(json!({"kind": "what-if", "layers": [], "iterations": 25})),
        &LIMITS,
        &mut Sequential::default(),
    )
    .unwrap();
    let AnalysisOutcome::WhatIf(none) = none else {
        panic!("expected a what-if");
    };
    assert_eq!(none.steps.len(), 1);
    assert_eq!(
        none.steps[0].point.success_rate,
        result.steps[0].point.success_rate
    );
}

#[test]
fn a_parameter_layer_moves_the_reported_retirement_age() {
    let graph = plan();
    let retire = discover(&graph)
        .unwrap()
        .into_iter()
        .find(|p| p.name == "Retirement age")
        .unwrap();
    let AnalysisOutcome::WhatIf(result) = analyze(
        &graph,
        request(json!({"kind": "what-if", "iterations": 25, "layers": [
            {"kind": "parameter", "parameter_id": retire.parameter_id, "value": 50.5}]})),
        &LIMITS,
        &mut Sequential::default(),
    )
    .unwrap() else {
        panic!("expected a what-if");
    };
    assert_eq!(result.plan_retirement_age, Some(45.0));
    assert_eq!(result.what_if_retirement_age, Some(50.5));
}

#[test]
fn quick_what_if_is_the_same_analysis_with_a_smaller_budget() {
    let graph = plan();
    let quick = what_if::QuickWhatIf {
        layers: vec![WhatIfLayer::MarketShock { age: 40, drop: 0.3 }],
        iterations: Some(900),
    };
    let iterations = what_if::quick_iterations(quick.iterations, LIMITS.iteration_cap);
    assert_eq!(iterations, what_if::MAX_QUICK_ITERATIONS);
    let outcome = analyze(
        &graph,
        CreateAnalysis::WhatIf {
            layers: quick.layers,
            iterations: Some(25),
        },
        &LIMITS,
        &mut Sequential::default(),
    )
    .unwrap();
    assert!(matches!(outcome, AnalysisOutcome::WhatIf(_)));
}

#[test]
fn cancellation_stops_every_kind_between_simulations() {
    let graph = plan();
    let spend = parameter_id(&graph, "Spending");
    let bodies = [
        json!({"kind": "sweep", "iterations": 25, "axes": [
            {"parameter_id": spend, "min": 1000.0, "max": 3000.0, "steps": 3}]}),
        json!({"kind": "sensitivity", "iterations": 25}),
        json!({"kind": "solve", "iterations": 25, "objective": "max-parameter",
            "min_value": 0.5, "vary": [{"parameter_id": spend}]}),
        json!({"kind": "what-if", "iterations": 75,
            "layers": [{"kind": "market-shock", "age": 40, "drop": 0.3}]}),
    ];
    for body in bodies {
        let mut runner = Sequential {
            cancel_after: Some(1),
            ..Sequential::default()
        };
        let error = analyze(&graph, request(body.clone()), &LIMITS, &mut runner)
            .err()
            .unwrap_or_else(|| panic!("{body} ran to completion"));
        assert!(error.is_cancelled(), "{body}: {error}");
        assert_eq!(runner.stats_runs + runner.summary_runs, 1, "{body}");
    }
}

#[test]
fn requests_are_checked_before_anything_runs() {
    let graph = plan();
    let spend = parameter_id(&graph, "Spending");
    let refused = |body: serde_json::Value| {
        let mut runner = Sequential::default();
        let error = analyze(&graph, request(body), &LIMITS, &mut runner)
            .err()
            .unwrap();
        assert_eq!(runner.stats_runs + runner.summary_runs, 0);
        error
    };
    let plan_error = |error: AnalysisError| match error {
        AnalysisError::Plan(e) => e,
        other => panic!("not a plan error: {other}"),
    };
    // Too few or too many iterations; an unknown, repeated or empty axis.
    for iterations in [24, 1_001] {
        let error = refused(json!({"kind": "sensitivity", "iterations": iterations}));
        assert!(matches!(plan_error(error), crate::PlanError::Invalid(_)));
    }
    let unknown = plan_error(refused(json!({"kind": "sweep", "axes": [
        {"parameter_id": "parameter:999"}]})));
    assert!(matches!(unknown, crate::PlanError::NotFound("parameter")));
    let twice = plan_error(refused(json!({"kind": "sweep", "axes": [
        {"parameter_id": spend}, {"parameter_id": spend}]})));
    assert!(twice.to_string().contains("named twice"));
    assert!(matches!(
        plan_error(refused(json!({"kind": "sweep", "axes": []}))),
        crate::PlanError::Invalid(_)
    ));
    let range = plan_error(refused(json!({"kind": "sweep", "axes": [
        {"parameter_id": spend, "min": 5.0, "max": 1.0}]})));
    assert!(range.to_string().contains("max above min"));
    let fraction = plan_error(refused(json!({"kind": "sensitivity", "fraction": 2.0})));
    assert!(fraction.to_string().contains("fraction"));
    let threshold = plan_error(refused(
        json!({"kind": "solve", "objective": "max-parameter",
        "min_value": 1.5, "vary": [{"parameter_id": spend}]}),
    ));
    assert!(threshold.to_string().contains("between 0 and 1"));
    // What-if layers: nine, a total wipe-out, an unknown parameter.
    let nine: Vec<_> = (0..9)
        .map(|_| json!({"kind": "market-shock", "age": 45, "drop": 0.1}))
        .collect();
    for layers in [
        json!(nine),
        json!([{"kind": "market-shock", "age": 45, "drop": 1.0}]),
        json!([{"kind": "parameter", "parameter_id": 999_999, "value": 1.0}]),
    ] {
        let error = refused(json!({"kind": "what-if", "layers": layers}));
        assert!(matches!(plan_error(error), crate::PlanError::Invalid(_)));
    }
}

#[test]
fn the_budget_is_the_progress_denominator() {
    let graph = plan();
    let spend = parameter_id(&graph, "Spending");
    let budget = |body: serde_json::Value| {
        prepare(&graph, request(body), &LIMITS)
            .unwrap()
            .spec
            .budget()
    };
    assert_eq!(
        budget(json!({"kind": "sweep", "iterations": 30, "axes": [
            {"parameter_id": spend, "steps": 4}]})),
        (4 + 1) * 30
    );
    assert_eq!(
        budget(json!({"kind": "sensitivity", "iterations": 40})),
        (2 * 2 + 1) * 40
    );
    // 500 over the plan and two layers, rounded up per step.
    assert_eq!(
        budget(json!({"kind": "what-if", "layers": [
            {"kind": "market-shock", "age": 40, "drop": 0.2},
            {"kind": "market-shock", "age": 41, "drop": 0.2}]})),
        3 * 167
    );
}

#[test]
fn applying_a_stack_writes_values_and_events_into_the_graph() {
    let mut graph = plan();
    let retire = discover(&graph)
        .unwrap()
        .into_iter()
        .find(|p| p.name == "Retirement age")
        .unwrap();
    let events = graph.events.len();
    let layers = vec![
        WhatIfLayer::Parameter {
            parameter_id: retire.parameter_id,
            value: 50.5,
        },
        WhatIfLayer::OneOff {
            age: 38,
            amount: -40_000.0,
            account_id: None,
        },
        WhatIfLayer::OneOff {
            age: 38,
            amount: -40_000.0,
            account_id: None,
        },
        WhatIfLayer::MarketShock { age: 40, drop: 0.3 },
    ];
    what_if::apply(&mut graph, &layers).unwrap();
    assert_eq!(graph.events.len(), events + 3);
    let names: Vec<_> = graph.events.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"One-off cost $40k at 38"));
    assert!(names.contains(&"One-off cost $40k at 38 (2)"));
    assert!(names.contains(&"Market shock −30% at 40"));
    let row = graph
        .parameters
        .iter()
        .find(|p| p.id == retire.parameter_id)
        .unwrap();
    assert_eq!((row.age_years, row.age_months), (Some(50), Some(6)));
    // It still compiles, and what-if says the plan now retires at 50.5.
    let now = discover(&graph).unwrap();
    assert_eq!(
        now.iter()
            .find(|p| p.name == "Retirement age")
            .unwrap()
            .current,
        50.5
    );

    // Atomic: a bad layer leaves the graph as it was.
    let before = serde_json::to_string(&graph).unwrap();
    let bad = [
        WhatIfLayer::MarketShock { age: 41, drop: 0.2 },
        WhatIfLayer::Parameter {
            parameter_id: 999,
            value: 1.0,
        },
    ];
    assert!(what_if::apply(&mut graph, &bad).is_err());
    assert_eq!(serde_json::to_string(&graph).unwrap(), before);
}

#[test]
fn stacks_validate_their_shape() {
    let stack = |entries: serde_json::Value| -> what_if::WhatIfStack {
        serde_json::from_value(json!({"entries": entries})).unwrap()
    };
    let entry = |id: &str, layer: serde_json::Value| json!({"id": id, "enabled": id != "off", "layer": layer});
    let shock = json!({"kind": "market-shock", "age": 50, "drop": 0.2});
    assert!(stack(json!([entry("a", shock.clone())])).validate().is_ok());
    assert!(stack(json!([entry("", shock.clone())])).validate().is_err());
    assert!(
        stack(json!([entry(
            "a",
            json!({"kind": "one-off", "age": 50, "amount": 0.0, "account_id": null})
        )]))
        .validate()
        .is_err()
    );
    let many: Vec<_> = (0..17)
        .map(|i| entry(&format!("k{i}"), shock.clone()))
        .collect();
    assert!(stack(json!(many)).validate().is_err());
    let mixed = stack(json!([entry("on", shock.clone()), entry("off", shock)]));
    assert_eq!(mixed.enabled_layers().len(), 1);
}
