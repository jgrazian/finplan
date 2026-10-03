//! The library, the edit entry point and new plans, with no database to
//! compare against. The server's `domain::edit_route_tests` hold each to its
//! route.

use serde_json::{Value, json};

use super::*;
use crate::create::{LOCAL_USER_ID, duplicate, new_plan};
use crate::edit::{EditOp, apply};
use crate::specs::scenarios::CreateScenario;

fn default_graph() -> ScenarioGraph {
    serde_json::from_str(include_str!("../testdata/default_snapshot.json")).unwrap()
}

fn op(value: Value) -> LibraryOp {
    serde_json::from_value(value).unwrap()
}

fn edit(value: Value) -> EditOp {
    serde_json::from_value(value).unwrap()
}

fn scenario(value: Value) -> CreateScenario {
    serde_json::from_value(value).unwrap()
}

#[test]
fn the_starter_library_is_what_the_server_has_always_seeded() {
    let library = seed();
    assert_eq!(library.return_profiles.len(), 8);
    assert_eq!(library.inflation_profiles.len(), 2);
    assert_eq!(library.tax_configs.len(), 1);
    assert_eq!(library.distributions.len(), 10);
    assert_eq!(library.tax_configs[0].federal_brackets.len(), 7);
    assert_eq!(
        library.return_profile_order().first().map(|id| library
            .return_profiles
            .iter()
            .find(|p| p.id == *id)
            .unwrap()
            .name
            .as_str()),
        Some("Cash / T-Bills"),
        "all at sort_order 0, so alphabetical"
    );
    let mut pruned = library.clone();
    pruned.prune_distributions();
    assert_eq!(pruned, library, "nothing orphaned");
    let again: Library = serde_json::from_str(&serde_json::to_string(&library).unwrap()).unwrap();
    assert_eq!(again, library);
}

#[test]
fn a_new_plan_is_an_empty_scenario_on_the_library() {
    let library = seed();
    let body = scenario(json!({
        "name": "  First  ", "start_date": "2026-01-01", "birth_date": "1990-05-06",
        "inflation_profile_id": 2, "tax_config_id": 1,
    }));
    let graph = new_plan(&body, &library, 7, "2026-10-03 14:05:09").unwrap();
    assert_eq!(graph.scenario.id, 7);
    assert_eq!(graph.scenario.user_id, LOCAL_USER_ID);
    assert_eq!(graph.scenario.name, "First");
    assert_eq!(graph.scenario.duration_years, 30);
    assert_eq!(graph.scenario.collect_ledger, 1);
    assert_eq!(graph.scenario.updated_at, "2026-10-03 14:05:09");
    assert!(graph.accounts.is_empty() && graph.assets.is_empty() && graph.events.is_empty());
    assert_eq!(graph.tax_brackets.len(), 7);
    assert_eq!(
        graph.inflation_profile_name.as_deref(),
        Some("US Historical (stochastic)")
    );
    assert_eq!(Library::from_graph(&graph), library);

    // With no assumptions named it has none; there is no default.
    let bare = new_plan(
        &scenario(json!({"name": "Bare", "start_date": "2026-01-01"})),
        &library,
        1,
        "now",
    )
    .unwrap();
    assert!(bare.tax_config.is_none() && bare.inflation_distribution_id.is_none());

    for body in [
        json!({"name": "A", "start_date": "2026-01-01", "tax_config_id": 2}),
        json!({"name": "A", "start_date": "2026-01-01", "inflation_profile_id": 3}),
    ] {
        let err = new_plan(&scenario(body), &library, 1, "now").unwrap_err();
        assert_eq!(err.to_string(), "assumption must belong to your account");
    }
}

#[test]
fn a_duplicate_is_the_plan_under_a_new_name() {
    let graph = default_graph();
    let copy = duplicate(&graph, 99, "  Second ", "2026-10-03 14:05:09").unwrap();
    assert_eq!(copy.scenario.id, 99);
    assert_eq!(copy.scenario.name, "Second");
    assert_eq!(copy.scenario.created_at, "2026-10-03 14:05:09");
    assert_eq!(copy.events.len(), graph.events.len());
    assert_eq!(
        crate::snapshot::snapshot(&copy).unwrap().1,
        crate::snapshot::snapshot(&{
            let mut same = graph.clone();
            same.scenario.id = 99;
            same.scenario.name = "Second".into();
            same.scenario.created_at = copy.scenario.created_at.clone();
            same.scenario.updated_at = copy.scenario.updated_at.clone();
            same
        })
        .unwrap()
        .1
    );
    assert!(duplicate(&graph, 99, "   ", "now").is_err());
}

#[test]
fn attach_refreshes_the_scenarios_copies_and_clears_dangling_references() {
    let library = seed();
    let mut graph = new_plan(
        &scenario(json!({"name": "P", "start_date": "2026-01-01",
                         "inflation_profile_id": 1, "tax_config_id": 1})),
        &library,
        1,
        "now",
    )
    .unwrap();

    // The tax table changes under the plan: attaching brings the copy along.
    let mut edited = library.clone();
    apply_library(
        &mut edited,
        std::slice::from_ref(&graph),
        &op(json!({"op": "update_tax_config", "id": 1,
                   "body": {"state_rate": 0.07, "federal_brackets": [{"threshold": 0.0, "rate": 0.2}]}})),
    )
    .unwrap();
    assert_eq!(graph.tax_config.as_ref().unwrap().state_rate, 0.05);
    edited.attach(&mut graph);
    assert_eq!(graph.tax_config.as_ref().unwrap().state_rate, 0.07);
    assert_eq!(graph.tax_brackets.len(), 1);

    // Deleting what it names leaves it with none.
    apply_library(
        &mut edited,
        std::slice::from_ref(&graph),
        &op(json!({"op": "delete_inflation_profile", "id": 1})),
    )
    .unwrap();
    edited.attach(&mut graph);
    assert_eq!(graph.scenario.inflation_profile_id, None);
    assert_eq!(graph.inflation_profile_name, None);
    assert_eq!(graph.inflation_distribution_id, None);
    assert_eq!(graph.scenario.tax_config_id, Some(1));
}

#[test]
fn library_edits_are_atomic_and_name_what_is_in_use() {
    let mut graph = default_graph();
    let mut library = Library::from_graph(&graph);
    assert!(!library.return_profiles.is_empty());
    let used = graph
        .assets
        .iter()
        .find_map(|a| a.return_profile_id)
        .expect("an asset with a profile");

    let before = library.clone();
    let err = apply_library(
        &mut library,
        std::slice::from_ref(&graph),
        &op(json!({"op": "delete_return_profile", "id": used})),
    )
    .unwrap_err();
    assert!(
        matches!(&err, PlanError::Conflict(m) if m.starts_with("return profile is still used by: ")),
        "{err}"
    );
    assert_eq!(library, before);

    // Another plan, not passed, would hold it: the caller lists every plan.
    graph.assets.clear();
    graph.bank.clear();
    graph.investment.clear();
    graph.accounts.clear();
    let out = apply_library(
        &mut library,
        std::slice::from_ref(&graph),
        &op(json!({"op": "delete_return_profile", "id": used})),
    )
    .unwrap();
    assert_eq!(out.id, None);
    assert!(library.return_profiles.iter().all(|p| p.id != used));
    assert!(
        library.distributions.len() < before.distributions.len(),
        "the profile's distribution is swept"
    );

    // A failing create changes nothing, and a good one numbers above the rest.
    let snapshot = library.clone();
    for body in [
        json!({"name": "", "distribution": {"kind": "None"}}),
        json!({"name": "X", "distribution": {"kind": "Normal", "mean": 0.0, "std_dev": -1.0}}),
    ] {
        apply_library(
            &mut library,
            &[],
            &op(json!({"op": "create_return_profile", "body": body})),
        )
        .unwrap_err();
        assert_eq!(library, snapshot);
    }
    let max = library.return_profiles.iter().map(|p| p.id).max().unwrap();
    let out = apply_library(
        &mut library,
        &[],
        &op(json!({"op": "create_return_profile",
                   "body": {"name": "Fresh", "distribution": {"kind": "Fixed", "rate": 0.01}}})),
    )
    .unwrap();
    assert_eq!(out.id, Some(max + 1));
}

#[test]
fn edit_ops_apply_by_name_and_report_created_ids() {
    let mut graph = default_graph();
    let asset = apply(
        &mut graph,
        &edit(json!({"op": "create_asset", "body": {"name": "NEW", "initial_price": 10.0}})),
    )
    .unwrap();
    let id = asset.id.expect("a created row has an id");
    assert!(graph.assets.iter().any(|a| a.id == id && a.name == "NEW"));

    let updated = apply(
        &mut graph,
        &edit(json!({"op": "update_asset", "id": id, "body": {"name": "NEWER"}})),
    )
    .unwrap();
    assert_eq!(updated.id, None);

    // A reorder through the entry point puts the named asset first.
    apply(
        &mut graph,
        &edit(json!({"op": "reorder_assets", "ids": [id]})),
    )
    .unwrap();
    assert_eq!(graph.assets[0].id, id);
    assert_eq!(graph.assets[0].sort_order, 0);

    // A refusal leaves the graph as it was.
    let before = serde_json::to_value(&graph).unwrap();
    apply(
        &mut graph,
        &edit(json!({"op": "delete_asset", "id": 987654})),
    )
    .unwrap_err();
    apply(
        &mut graph,
        &edit(json!({"op": "create_asset", "body": {"name": "NEWER", "initial_price": 1.0}})),
    )
    .unwrap_err();
    assert_eq!(serde_json::to_value(&graph).unwrap(), before);

    assert!(serde_json::from_value::<EditOp>(json!({"op": "drop_table"})).is_err());
}

#[test]
fn reorders_renumber_only_when_something_moves() {
    let mut graph = default_graph();
    let ids: Vec<i64> = graph.accounts.iter().map(|a| a.id).collect();
    let orders: Vec<i64> = graph.accounts.iter().map(|a| a.sort_order).collect();

    crate::edit::reorder_accounts(&mut graph, &ids);
    assert_eq!(
        graph
            .accounts
            .iter()
            .map(|a| a.sort_order)
            .collect::<Vec<_>>(),
        orders,
        "already in that order"
    );

    let last = *ids.last().unwrap();
    crate::edit::reorder_accounts(&mut graph, &[last, 424242, last]);
    let now: Vec<i64> = graph.accounts.iter().map(|a| a.id).collect();
    assert_eq!(now[0], last);
    assert_eq!(&now[1..], &ids[..ids.len() - 1]);
    assert_eq!(
        graph
            .accounts
            .iter()
            .map(|a| a.sort_order)
            .collect::<Vec<_>>(),
        (0..ids.len() as i64).collect::<Vec<_>>()
    );
}
