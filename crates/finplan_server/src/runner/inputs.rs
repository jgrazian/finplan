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
