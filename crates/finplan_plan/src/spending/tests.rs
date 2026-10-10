//! Compare spending rules against the anonymized default plan.

use finplan_core::analysis::{McRunner, StatsRun};
use finplan_core::config::SimulationConfig;
use finplan_core::error::SimulationError;
use finplan_core::model::{MonteCarloConfig, MonteCarloProgress, MonteCarloSummary};
use finplan_core::simulation::{monte_carlo_simulate_with_progress, monte_carlo_stats_only};
use serde_json::json;

use super::*;
use crate::edit::{self, EditOp};
use crate::specs::scenarios::SetFunding;

/// Every Monte Carlo it is asked for, with the seed and plan it ran.
#[derive(Default)]
struct Recording {
    seeds: Vec<Option<u64>>,
}

impl McRunner for Recording {
    fn stats(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<StatsRun, SimulationError> {
        self.seeds.push(mc.seed);
        monte_carlo_stats_only(config, mc, &MonteCarloProgress::default())
    }
    fn summary(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<MonteCarloSummary, SimulationError> {
        monte_carlo_simulate_with_progress(config, mc, &MonteCarloProgress::default())
    }
    fn cancelled(&self) -> bool {
        false
    }
}

fn make(graph: &mut ScenarioGraph, op: serde_json::Value) -> i64 {
    let op: EditOp = serde_json::from_value(op).unwrap();
    edit::apply(graph, &op).unwrap().id.unwrap_or_default()
}

/// The default plan, funded by its investments, retiring into `effect`
/// paid monthly from USAA (account 6) from 2036.
fn retiring_into(effect: serde_json::Value) -> (ScenarioGraph, i64) {
    let mut graph: ScenarioGraph =
        serde_json::from_str(include_str!("../../testdata/default_snapshot.json")).unwrap();
    graph.scenario.duration_years = 30;
    let body: SetFunding =
        serde_json::from_value(json!({"funding": {"strategy": "TaxEfficientEarly"}})).unwrap();
    edit::set_funding(&mut graph, &body).unwrap();
    let id = make(
        &mut graph,
        json!({"op": "create_event", "body": {
            "name": "Retirement spending", "enabled": true,
            "trigger": {"kind": "Repeating", "interval": "Monthly",
                        "start_condition": {"kind": "Date", "on_date": "2036-01-01"}},
            "effects": [effect]}}),
    );
    (graph, id)
}

fn guardrails_plan() -> (ScenarioGraph, i64) {
    retiring_into(json!({"kind": "Spending", "from_account_id": 6,
        "rule": {"rule": "Guardrails", "initial": {"kind": "Amount", "amount": 90000.0}}}))
}

fn body(iterations: usize) -> SpendingCompareRequest {
    SpendingCompareRequest {
        iterations: Some(iterations),
        ..Default::default()
    }
}

#[test]
fn every_choice_runs_on_the_same_markets() {
    let (graph, event) = guardrails_plan();
    let mut runner = Recording::default();
    let comparison = compare(&graph, &body(30), &mut runner).unwrap();
    assert_eq!(comparison.event_id, event);
    assert_eq!(comparison.event_name, "Retirement spending");
    let labels: Vec<&str> = comparison.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "As planned",
            "Fixed",
            "Percent of portfolio",
            "Guardrails",
            "Capped change",
            "VPW"
        ]
    );
    // As planned once, then each other choice, all on the analysis seed.
    assert_eq!(runner.seeds.len(), 6);
    assert!(runner.seeds.iter().all(|s| *s == Some(ANALYSIS_SEED)));
    // The plan's first draw is its rule's, and the rate rules start at it.
    assert!(
        (comparison.first_draw - 90_000.0).abs() < 1.0,
        "{}",
        comparison.first_draw
    );
    assert!(comparison.initial_rate > 0.0 && comparison.initial_rate <= 0.2);
    assert!(comparison.rows[0].effect.is_none());
    assert!(matches!(
        comparison.rows[1].effect,
        Some(EffectSpec::Expense { .. })
    ));
    assert!(matches!(
        comparison.rows[3].effect,
        Some(EffectSpec::Spending { .. })
    ));
    for row in &comparison.rows {
        assert!(row.funding_success_rate.is_some(), "{}", row.label);
        assert!(row.median_lifetime_spending > 0.0, "{}", row.label);
        assert!(row.median_final_net_worth_real.is_some(), "{}", row.label);
    }
    // A fixed draw never cuts.
    assert_eq!(comparison.rows[1].cut_rate, 0.0);
    // Same seed, same answer.
    let again = compare(&graph, &body(30), &mut Recording::default()).unwrap();
    assert_eq!(
        serde_json::to_string(&comparison).unwrap(),
        serde_json::to_string(&again).unwrap()
    );
}

#[test]
fn fixed_reproduces_the_plain_expense_plan() {
    let (graph, _) = guardrails_plan();
    let comparison = compare(&graph, &body(30), &mut Recording::default()).unwrap();
    let fixed = &comparison.rows[1];
    let Some(EffectSpec::Expense { amount, .. }) = &fixed.effect else {
        panic!("Fixed writes an Expense");
    };

    // The same plan written with that Expense, run the same way.
    let (expense_plan, _) = retiring_into(json!({"kind": "Expense", "from_account_id": 6,
        "amount": amount}));
    let compiled = compile::compile(&expense_plan).unwrap();
    let mut config = compiled.config.clone();
    config.collect_ledger = false;
    let mc = MonteCarloConfig {
        iterations: 30,
        percentiles: vec![0.5],
        compute_mean: false,
        parallel_batches: 4,
        seed: Some(ANALYSIS_SEED),
        ..Default::default()
    };
    let (stats, _) = Recording::default().stats(&config, &mc).unwrap();
    assert_eq!(fixed.funding_success_rate, stats.funding_success_rate);

    // And comparing that plan, its own spending is the Fixed row.
    let own = compare(&expense_plan, &body(30), &mut Recording::default()).unwrap();
    assert_eq!(own.rows[0].funding_success_rate, stats.funding_success_rate);
    assert!((own.first_draw - comparison.first_draw).abs() / comparison.first_draw < 0.01);
}

#[test]
fn chosen_rules_are_checked_and_counted() {
    let (graph, _) = guardrails_plan();
    let request = SpendingCompareRequest {
        choices: Some(vec![SpendingChoice::Rule {
            rule: SpendingRuleSpec::PercentOfPortfolio {
                rate: 0.5,
                floor: None,
                ceiling: None,
            },
        }]),
        iterations: Some(25),
        event_id: None,
    };
    let refused = compare(&graph, &request, &mut Recording::default()).unwrap_err();
    assert!(
        format!("{refused:?}").contains("withdrawal rate"),
        "{refused:?}"
    );

    let none = SpendingCompareRequest {
        choices: Some(Vec::new()),
        ..body(25)
    };
    let refused = compare(&graph, &none, &mut Recording::default()).unwrap_err();
    assert!(
        format!("{refused:?}").contains("between 1 and"),
        "{refused:?}"
    );

    // The default plan's expenses are not named for retirement, and it has
    // no rule: nothing to compare without an event named.
    let plain: ScenarioGraph =
        serde_json::from_str(include_str!("../../testdata/default_snapshot.json")).unwrap();
    let refused = compare(&plain, &body(25), &mut Recording::default()).unwrap_err();
    assert!(
        format!("{refused:?}").contains("no spending rule"),
        "{refused:?}"
    );
}
