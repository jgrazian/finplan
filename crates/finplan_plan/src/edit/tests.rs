//! Edits on a plan held in memory, with no database to compare against.
//!
//! The server keeps the tests that check an edit leaves the graph exactly as
//! the SQL route leaves the database (`domain::edit_route_tests`); these are the
//! ones that need only a graph.

use serde_json::json;

use super::*;
use crate::compile;
use crate::error::{PlanError, PlanResult};
use crate::graph::ScenarioGraph;
use crate::specs::parameters::ParameterBody;
use crate::specs::profiles::CreateProfile;
use crate::specs::{AmountSpec, TriggerParent, TriggerSpec};

/// The anonymized default plan the other tests in this crate start from.
fn default_graph() -> ScenarioGraph {
    serde_json::from_str(include_str!("../../testdata/default_snapshot.json")).unwrap()
}

/// A compiled config as text that does not depend on hash-map order. (It does
/// not serialize to JSON: a `ReturnProfile::Fixed` is a tagged newtype.)
fn canonical(config: &finplan_core::config::SimulationConfig) -> Vec<String> {
    fn sorted<K: std::fmt::Debug, V: std::fmt::Debug>(
        map: &std::collections::HashMap<K, V>,
    ) -> String {
        let mut entries: Vec<String> = map.iter().map(|kv| format!("{kv:?}")).collect();
        entries.sort();
        entries.join(", ")
    }
    vec![
        sorted(&config.return_profiles),
        format!("{:?}", config.inflation_profile),
        sorted(&config.asset_returns),
        sorted(&config.asset_prices),
        format!("{:?}", config.tax_config),
        format!("{:?} {:?}", config.start_date, config.birth_date),
        format!("{:?}", config.accounts),
        format!("{:?}", config.duration_years),
        format!("{:?}", config.events),
        sorted(&config.asset_tracking_errors),
        sorted(&config.parameters),
        format!("{:?}", config.collect_ledger),
    ]
}

#[test]
fn orphan_rows_do_not_change_what_compiles() {
    let mut graph = default_graph();
    let before = canonical(&compile::compile(&graph).unwrap().config);

    // Rows no event reaches: an amount tree, and a detached condition with a
    // child.
    let mut batch = RowBatch::for_graph(&graph);
    AmountSpec::Max {
        left: Box::new(AmountSpec::Fixed { value: 1.0 }),
        right: Box::new(AmountSpec::SourceBalance),
    }
    .lower(&mut batch, 0)
    .unwrap();
    serde_json::from_value::<TriggerSpec>(json!({"kind": "Or", "children": [
        {"kind": "Age", "years": 50}]}))
    .unwrap()
    .lower(&mut batch, TriggerParent::Detached, 0)
    .unwrap();
    batch.merge_into(&mut graph).unwrap();

    let after = canonical(&compile::compile(&graph).unwrap().config);
    assert_eq!(before, after);
}

fn body(value: serde_json::Value) -> EventBody {
    serde_json::from_value(value).expect("event body")
}

/// An edit that is refused leaves the graph exactly as it was.
fn refused<T: std::fmt::Debug>(
    graph: &mut ScenarioGraph,
    edit: impl FnOnce(&mut ScenarioGraph) -> PlanResult<T>,
) -> PlanError {
    let before = serde_json::to_value(&*graph).unwrap();
    let err = edit(graph).unwrap_err();
    assert_eq!(serde_json::to_value(&*graph).unwrap(), before, "{err}");
    err
}

#[test]
fn a_taken_name_is_refused_with_the_shared_message() {
    let mut graph = default_graph();
    let taken = body(json!({"name": " Salary ", "trigger": {"kind": "Manual"}}));
    assert_eq!(
        refused(&mut graph, |g| create_event(g, &taken)),
        PlanError::Conflict(events::NAME_TAKEN.into())
    );

    let asset: CreateAsset =
        serde_json::from_value(json!({"name": "VFIAX", "initial_price": 1.0})).unwrap();
    assert_eq!(
        refused(&mut graph, |g| create_asset(g, &asset)),
        PlanError::Conflict(assets::NAME_TAKEN.into())
    );

    let account: CreateAccount = serde_json::from_value(json!({
        "name": "Vanguard", "flavor": "Bank", "return_profile_id": 6
    }))
    .unwrap();
    assert_eq!(
        refused(&mut graph, |g| create_account(g, &account)),
        PlanError::Conflict(accounts::NAME_TAKEN.into())
    );
}

#[test]
fn an_asset_must_cost_something() {
    let mut graph = default_graph();
    let free: CreateAsset =
        serde_json::from_value(json!({"name": "Free", "initial_price": 0.0})).unwrap();
    assert_eq!(
        refused(&mut graph, |g| create_asset(g, &free)),
        PlanError::invalid("initial_price must be positive")
    );
    let update: UpdateAsset = serde_json::from_value(json!({"initial_price": -1.0})).unwrap();
    assert_eq!(
        refused(&mut graph, |g| update_asset(g, 1, &update)),
        PlanError::invalid("initial_price must be positive")
    );
}

#[test]
fn an_account_cannot_change_flavor() {
    let mut graph = default_graph();
    let to_bank: UpdateAccount = serde_json::from_value(json!({
        "flavor": "Bank", "cash_value": 1.0, "return_profile_id": 6
    }))
    .unwrap();
    assert_eq!(
        refused(&mut graph, |g| update_account(g, 1, &to_bank)),
        PlanError::Conflict(
            "cannot change account flavor from Investment to Bank; create a new account instead"
                .into()
        )
    );
    let blank: UpdateAccount = serde_json::from_value(json!({"name": "  "})).unwrap();
    assert_eq!(
        refused(&mut graph, |g| update_account(g, 1, &blank)),
        PlanError::invalid("an account needs a name")
    );
}

#[test]
fn lots_belong_in_investment_accounts_and_are_not_negative() {
    let mut graph = default_graph();
    let lot =
        |value: serde_json::Value| -> CreatePosition { serde_json::from_value(value).unwrap() };
    let bank = graph.bank.keys().copied().next().unwrap();

    assert_eq!(
        refused(&mut graph, |g| create_position(
            g,
            bank,
            &lot(json!({"asset_id": 1, "units": 1.0, "cost_basis": 1.0}))
        )),
        PlanError::Conflict("positions can only be held in Investment accounts, not Bank".into())
    );
    assert_eq!(
        refused(&mut graph, |g| create_position(
            g,
            9999,
            &lot(json!({"asset_id": 1, "units": 1.0, "cost_basis": 1.0}))
        )),
        PlanError::NotFound("account")
    );
    assert_eq!(
        refused(&mut graph, |g| create_position(
            g,
            1,
            &lot(json!({"asset_id": 1, "units": -1.0, "cost_basis": 1.0}))
        )),
        PlanError::invalid("units and cost_basis must be non-negative")
    );
    let date = refused(&mut graph, |g| {
        create_position(
            g,
            1,
            &lot(
                json!({"asset_id": 1, "purchase_date": "someday", "units": 1.0, "cost_basis": 1.0}),
            ),
        )
    });
    assert!(
        matches!(&date, PlanError::Invalid(m) if m.starts_with("invalid purchase_date 'someday'"))
    );

    let before = graph.positions[&1].len();
    let id = create_position(
        &mut graph,
        1,
        &lot(json!({"asset_id": 1, "units": 2.0, "cost_basis": 3.0})),
    )
    .unwrap();
    assert_eq!(graph.positions[&1].len(), before + 1);
    assert_eq!(
        graph.positions[&1].last().unwrap().purchase_date,
        graph.scenario.start_date
    );
    delete_position(&mut graph, 1, id).unwrap();
    assert_eq!(graph.positions[&1].len(), before);
}

#[test]
fn scenario_settings_are_checked_name_then_dates_then_horizon() {
    let mut graph = default_graph();
    let update =
        |value: serde_json::Value| -> UpdateScenario { serde_json::from_value(value).unwrap() };

    let all_bad = update(json!({"name": " ", "start_date": "x", "duration_years": 0}));
    assert_eq!(
        refused(&mut graph, |g| update_scenario(g, &all_bad)),
        PlanError::invalid("scenario name cannot be empty")
    );
    let dates_and_horizon = update(json!({"birth_date": "x", "duration_years": 0}));
    let err = refused(&mut graph, |g| update_scenario(g, &dates_and_horizon));
    assert!(matches!(&err, PlanError::Invalid(m) if m.starts_with("invalid birth_date 'x'")));
    let horizon = update(json!({"duration_years": 121}));
    assert_eq!(
        refused(&mut graph, |g| update_scenario(g, &horizon)),
        PlanError::invalid("duration_years must be between 1 and 120")
    );
    assert_eq!(
        refused(&mut graph, |g| update_scenario(
            g,
            &update(json!({"tax_config_id": 9999}))
        )),
        PlanError::NotFound("tax config")
    );
}

#[test]
fn parameter_names_are_unique_and_a_missing_one_cannot_be_deleted() {
    let mut graph = default_graph();
    let parameter =
        |value: serde_json::Value| -> ParameterBody { serde_json::from_value(value).unwrap() };
    let id = create_parameter(
        &mut graph,
        &parameter(json!({"name": "Floor", "value": {"kind": "Money", "value": 5.0}})),
    )
    .unwrap();
    assert_eq!(
        refused(&mut graph, |g| create_parameter(
            g,
            &parameter(json!({"name": " Floor", "value": {"kind": "Rate", "value": 0.1}}))
        )),
        PlanError::Conflict(parameters::NAME_TAKEN.into())
    );
    // Nothing reads it yet, so it may change type; then it is deleted.
    update_parameter(
        &mut graph,
        id,
        &parameter(json!({"name": "Floor", "value": {"kind": "Rate", "value": 0.1}})),
    )
    .unwrap();
    delete_parameter(&mut graph, id).unwrap();
    assert_eq!(
        refused(&mut graph, |g| delete_parameter(g, id)),
        PlanError::NotFound("parameter")
    );
}

#[test]
fn a_regime_switching_profile_places_its_regimes_first() {
    let mut graph = default_graph();
    let before = graph.distributions.len();
    let profile: CreateProfile = serde_json::from_value(json!({
        "name": "Regimes",
        "distribution": {"kind": "RegimeSwitching", "bull_to_bear_prob": 0.1,
            "bear_to_bull_prob": 0.3,
            "bull": {"kind": "Normal", "mean": 0.1, "std_dev": 0.12},
            "bear": {"kind": "StudentT", "mean": -0.05, "scale": 0.2, "df": 4.0}}
    }))
    .unwrap();
    let id = create_return_profile(&mut graph, &profile).unwrap();
    assert_eq!(graph.distributions.len(), before + 3);

    let root = &graph.distributions[&graph.return_profiles[&id].distribution_id];
    assert_eq!(root.kind, "RegimeSwitching");
    let (bull, bear) = (root.bull_id.unwrap(), root.bear_id.unwrap());
    assert!(bull < bear && bear < root.id, "children are placed first");
    assert_eq!(graph.distributions[&bull].kind, "Normal");
    assert_eq!(graph.distributions[&bear].df, Some(4.0));
    assert_eq!(root.bull_to_bear_prob, Some(0.1));
}

#[test]
fn deleting_what_events_use_is_refused() {
    let mut graph = default_graph();
    // The default plan's events pay into and spend from its accounts.
    let err = refused(&mut graph, |g| delete_account(g, 6));
    assert!(
        matches!(&err, PlanError::Conflict(m) if m.contains("event ")),
        "{err}"
    );
    assert_eq!(
        refused(&mut graph, |g| delete_account(g, 9999)),
        PlanError::NotFound("account")
    );
}

fn funding(strategy: &str, ceiling: Option<f64>, excludes: &[i64]) -> SetFunding {
    serde_json::from_value(json!({"funding": {
        "strategy": strategy, "bracket_ceiling": ceiling, "exclude_accounts": excludes}}))
    .unwrap()
}

#[test]
fn funding_policy_is_set_compiled_and_cleared() {
    let mut graph = default_graph();
    assert!(compile::compile(&graph).unwrap().config.funding.is_none());

    set_funding(
        &mut graph,
        &funding("BracketFilling", Some(0.22), &[3, 1, 3]),
    )
    .unwrap();
    assert_eq!(
        graph.funding_excludes,
        vec![1, 3],
        "sorted and deduplicated"
    );
    let compiled = compile::compile(&graph).unwrap();
    let policy = compiled.config.funding.expect("policy compiles");
    assert!(matches!(
        policy.order,
        finplan_core::model::WithdrawalOrder::BracketFilling { ceiling_rate } if ceiling_rate == 0.22
    ));
    assert_eq!(policy.exclude_accounts.len(), 2);
    assert!(policy.from.is_none());
    assert_eq!(graph.funding().unwrap().exclude_accounts, vec![1, 3]);

    // No ceiling means the engine's default.
    set_funding(&mut graph, &funding("BracketFilling", None, &[])).unwrap();
    let order = compile::compile(&graph)
        .unwrap()
        .config
        .funding
        .unwrap()
        .order;
    assert!(matches!(
        order,
        finplan_core::model::WithdrawalOrder::BracketFilling { ceiling_rate }
            if ceiling_rate == finplan_core::model::WithdrawalOrder::DEFAULT_BRACKET_CEILING
    ));

    set_funding(
        &mut graph,
        &SetFunding {
            funding: None,
            align_sweeps: None,
        },
    )
    .unwrap();
    assert!(graph.scenario.funding_strategy.is_none() && graph.funding_excludes.is_empty());
    assert!(graph.funding().is_none());
}

#[test]
fn funding_policy_can_align_strategy_sweeps() {
    let mut graph = default_graph();
    let strategy_rows = graph
        .withdrawal_sources
        .values()
        .filter(|r| r.mode == "Strategy")
        .count();
    assert!(strategy_rows > 0, "fixture has strategy sweeps");

    // Without the flag the sweeps keep their own strategy.
    set_funding(&mut graph, &funding("TaxFreeFirst", None, &[])).unwrap();
    assert!(
        graph
            .withdrawal_sources
            .values()
            .all(|r| r.strategy.as_deref() == Some("PenaltyAware"))
    );

    let mut body = funding("BracketFilling", Some(0.22), &[]);
    body.align_sweeps = Some(true);
    set_funding(&mut graph, &body).unwrap();
    for row in graph
        .withdrawal_sources
        .values()
        .filter(|r| r.mode == "Strategy")
    {
        assert_eq!(row.strategy.as_deref(), Some("BracketFilling"));
        assert_eq!(row.bracket_ceiling, Some(0.22));
    }
    compile::compile(&graph).unwrap();
}

#[test]
fn funding_policy_refuses_what_it_cannot_sell_or_fill() {
    let mut graph = default_graph();
    for (body, wanted) in [
        (funding("TaxFreeFirst", None, &[6]), "investment"),
        (funding("TaxFreeFirst", None, &[9999]), "does not exist"),
        (funding("ProRata", Some(0.12), &[]), "BracketFilling"),
        (funding("BracketFilling", Some(1.0), &[]), "not including"),
        (funding("BracketFilling", Some(-0.1), &[]), "not including"),
    ] {
        let err = refused(&mut graph, |g| set_funding(g, &body));
        assert!(
            matches!(&err, PlanError::Invalid(m) if m.contains(wanted)),
            "{err}"
        );
    }
    assert!(graph.scenario.funding_strategy.is_none(), "atomic");
}

#[test]
fn deleting_an_account_drops_it_from_the_funding_excludes() {
    let mut graph = default_graph();
    let mut spare = graph.accounts[0].clone();
    spare.id = 99;
    graph.accounts.push(spare);
    graph.investment.insert(99, graph.investment[&1].clone());
    graph.investment.get_mut(&99).unwrap().account_id = 99;
    graph.positions.remove(&99);
    set_funding(&mut graph, &funding("ProRata", None, &[1, 99])).unwrap();
    delete_account(&mut graph, 99).unwrap();
    assert_eq!(graph.funding_excludes, vec![1]);
    assert!(graph.scenario.funding_strategy.is_some());
}

#[test]
fn set_funding_runs_through_the_edit_entry_point() {
    let mut graph = default_graph();
    let op: EditOp = serde_json::from_value(json!({"op": "set_funding", "body": {
        "funding": {"strategy": "ProRata", "exclude_accounts": [2]}}}))
    .unwrap();
    apply(&mut graph, &op).unwrap();
    assert_eq!(graph.funding_excludes, vec![2]);
}
