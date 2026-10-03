//! Versioned immutable input graph, captured before a worker can see the run.
use crate::compile::rows::ScenarioGraph;
use sha2::{Digest, Sha256};

/// Bump when compilation, model semantics, or snapshot interpretation changes.
pub const MODEL_VERSION: &str = "finplan-0.8.0/snapshot-1";

pub fn snapshot(graph: &ScenarioGraph) -> Result<(String, String), serde_json::Error> {
    let mut graph = graph.clone();
    let mut profiles = std::collections::HashSet::new();
    profiles.extend(graph.assets.iter().filter_map(|a| a.return_profile_id));
    profiles.extend(graph.bank.values().map(|a| a.return_profile_id));
    profiles.extend(graph.investment.values().map(|a| a.cash_return_profile_id));
    graph.return_profiles.retain(|id, _| profiles.contains(id));
    let mut pending: Vec<_> = graph
        .return_profiles
        .values()
        .map(|p| p.distribution_id)
        .collect();
    pending.extend(graph.inflation_distribution_id);
    let mut distributions = std::collections::HashSet::new();
    while let Some(id) = pending.pop() {
        if distributions.insert(id)
            && let Some(row) = graph.distributions.get(&id)
        {
            pending.extend(row.bull_id);
            pending.extend(row.bear_id);
        }
    }
    graph
        .distributions
        .retain(|id, _| distributions.contains(id));
    let value = canonical_json(serde_json::to_value(&graph)?);
    let json = serde_json::to_string(&value)?;
    let mut fingerprint = value;
    // Wall-clock bookkeeping does not describe an input, and can vary without
    // an economic change. Hash actual values, including shared definitions.
    if let Some(scenario) = fingerprint
        .get_mut("scenario")
        .and_then(|v| v.as_object_mut())
    {
        scenario.remove("updated_at");
        scenario.remove("created_at");
    }
    let bytes = serde_json::to_vec(&fingerprint)?;
    let hash = format!("{:x}", Sha256::digest(bytes));
    Ok((json, hash))
}

/// Sort explicitly: workspace feature unification can enable serde_json's
/// preserve_order, so its Map implementation is not a canonicalization policy.
fn canonical_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let sorted: std::collections::BTreeMap<_, _> = object.into_iter().collect();
            serde_json::Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| (key, canonical_json(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical_json).collect())
        }
        scalar => scalar,
    }
}

#[cfg(test)]
mod tests {
    use super::snapshot;
    use crate::api::accounts::CatchUpSpec;
    use crate::compile::rows::{ParameterRow, ScenarioGraph, WithdrawalItemRow};
    use std::path::PathBuf;

    fn default_graph() -> ScenarioGraph {
        serde_json::from_str(include_str!("../suggest/testdata/default_snapshot.json")).unwrap()
    }

    /// The default graph plus the fields its anonymized file leaves empty: an
    /// investment account with catch-up tiers and a plan type, a named
    /// parameter, withdrawal source options and items, and a nested effect.
    /// Built in code so every value, timestamps included, is fixed.
    fn rich_graph() -> ScenarioGraph {
        let mut graph = default_graph();
        graph.scenario.created_at = "2026-01-02 03:04:05".to_string();
        graph.scenario.updated_at = "2026-02-03 04:05:06".to_string();

        let k401 = graph.investment.get_mut(&3).unwrap();
        k401.plan_type = Some("Traditional401k".to_string());
        k401.catch_up = sqlx::types::Json(vec![
            CatchUpSpec {
                from_age: 50,
                through_age: Some(59),
                amount: 7_500.0,
            },
            CatchUpSpec {
                from_age: 60,
                through_age: None,
                amount: 11_250.0,
            },
        ]);
        let roth = graph.investment.get_mut(&2).unwrap();
        roth.plan_type = Some("RothIra".to_string());
        roth.catch_up = sqlx::types::Json(vec![CatchUpSpec {
            from_age: 50,
            through_age: Some(120),
            amount: 1_000.0,
        }]);

        graph.parameters.push(ParameterRow {
            id: 1,
            name: "retirement".to_string(),
            kind: "Age".to_string(),
            number_value: None,
            date_value: None,
            age_years: Some(60),
            age_months: Some(6),
        });
        graph.parameters.push(ParameterRow {
            id: 2,
            name: "bonus".to_string(),
            kind: "Number".to_string(),
            number_value: Some(12_345.5),
            date_value: None,
            age_years: None,
            age_months: None,
        });

        let source = graph.withdrawal_sources.get_mut(&4).unwrap();
        source.bracket_ceiling = Some(47_150.0);
        graph.withdrawal_items.insert(
            4,
            vec![
                WithdrawalItemRow {
                    effect_id: 4,
                    role: "Order".to_string(),
                    position: 1,
                    account_id: 3,
                    asset_id: None,
                },
                WithdrawalItemRow {
                    effect_id: 4,
                    role: "Order".to_string(),
                    position: 0,
                    account_id: 1,
                    asset_id: None,
                },
            ],
        );

        let mut child = graph.effects.get(&4).unwrap().clone();
        child.id = 10;
        child.event_id = None;
        child.parent_id = Some(4);
        child.parent_slot = Some("on_true".to_string());
        child.probability = Some(0.25);
        child.sell_to_cover = Some(1);
        graph.effects.insert(10, child);
        graph.effect_children.insert((4, "on_true".to_string()), 10);
        graph
    }

    fn golden_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runner/testdata")
    }

    /// Compares a graph's snapshot JSON and hash to files recorded under
    /// `testdata/`. `UPDATE_GOLDEN=1` rewrites them; a mismatch otherwise means
    /// the serialized input changed, which stales every stored run.
    fn check_golden(name: &str, graph: &ScenarioGraph) {
        let (json, hash) = snapshot(graph).unwrap();
        let json_path = golden_dir().join(format!("{name}.snapshot.json"));
        let hash_path = golden_dir().join(format!("{name}.snapshot.sha256"));
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(golden_dir()).unwrap();
            std::fs::write(&json_path, &json).unwrap();
            std::fs::write(&hash_path, format!("{hash}\n")).unwrap();
        }
        let want_json = std::fs::read_to_string(&json_path).unwrap();
        let want_hash = std::fs::read_to_string(&hash_path).unwrap();
        assert!(
            json == want_json,
            "{name}: snapshot JSON drifted from {}",
            json_path.display()
        );
        assert_eq!(hash, want_hash.trim(), "{name}: snapshot hash drifted");
    }

    #[test]
    fn default_graph_snapshot_matches_golden() {
        check_golden("default", &default_graph());
    }

    #[test]
    fn rich_graph_snapshot_matches_golden() {
        check_golden("rich", &rich_graph());
    }

    #[test]
    fn rich_graph_exercises_the_fields_the_default_leaves_empty() {
        let (json, _) = snapshot(&rich_graph()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["investment"]["3"]["catch_up"][1]["amount"], 11_250.0);
        assert_eq!(value["withdrawal_items"]["4"].as_array().unwrap().len(), 2);
        assert_eq!(
            value["withdrawal_sources"]["4"]["bracket_ceiling"],
            47_150.0
        );
        assert_eq!(value["parameters"].as_array().unwrap().len(), 2);
        assert_eq!(value["effect_children"][0][1], 10);
        assert!(!value["tax_brackets"].as_array().unwrap().is_empty());
    }

    #[test]
    fn canonical_serialization_does_not_depend_on_json_map_features() {
        let a: serde_json::Value = serde_json::from_str(r#"{"z":{"b":2,"a":1},"a":0}"#).unwrap();
        let b: serde_json::Value = serde_json::from_str(r#"{"a":0,"z":{"a":1,"b":2}}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&super::canonical_json(a)).unwrap(),
            serde_json::to_string(&super::canonical_json(b)).unwrap()
        );
    }
}
