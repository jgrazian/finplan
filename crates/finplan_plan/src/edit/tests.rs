//! Edits on a plan held in memory, with no database to compare against.
//!
//! The server keeps the tests that check an edit leaves the graph exactly as
//! the SQL route leaves the database (`domain::edit_route_tests`); these are the
//! ones that need only a graph.

use serde_json::json;

use super::*;
use crate::compile;
use crate::graph::ScenarioGraph;
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
