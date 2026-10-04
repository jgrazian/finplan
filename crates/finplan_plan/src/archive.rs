//! Plan archives: versioned, self-contained exports of plan inputs, and the
//! checks an import applies before it trusts one.
//!
//! An archive carries inputs only (the snapshot JSON of each graph), never
//! sessions, credentials or run results. Everything here is pure, so the
//! server's import route and the browser's file import validate identically.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::compile;
use crate::error::{PlanError, PlanResult};
use crate::graph::ScenarioGraph;

/// The `format` of every archive.
pub const FORMAT: &str = "finplan.inputs";

/// The version [`pack`] writes. Older servers must reject these inputs
/// instead of silently ignoring parameters and treating expression-backed
/// amounts as their zero stub.
pub const VERSION: u32 = 3;

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanArchive {
    pub format: String,
    pub version: u32,
    /// Plan graph only: never sessions, credentials, or billing identifiers.
    #[ts(type = "unknown[]")]
    pub plans: Vec<Value>,
}

/// What importing an archive would add.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ArchivePreview {
    pub names: Vec<String>,
    pub accounts: usize,
    pub events: usize,
    pub assumptions: usize,
}

/// Archive `graphs` as their input snapshots, with the owner cleared.
pub fn pack(graphs: Vec<ScenarioGraph>) -> PlanResult<PlanArchive> {
    let plans = graphs
        .into_iter()
        .map(|mut graph| {
            graph.scenario.user_id.clear();
            crate::snapshot::snapshot(&graph)
                .map_err(|e| PlanError::internal(e.to_string()))
                .and_then(|(json, _)| {
                    serde_json::from_str(&json).map_err(|e| PlanError::internal(e.to_string()))
                })
        })
        .collect::<PlanResult<_>>()?;
    Ok(PlanArchive {
        format: FORMAT.into(),
        version: VERSION,
        plans,
    })
}

/// The graphs of `archive`, each checked for size, identifiers, tree
/// ownership, nesting, expressions and compilability.
pub fn unpack(archive: &PlanArchive) -> PlanResult<Vec<ScenarioGraph>> {
    if archive.format != FORMAT || !matches!(archive.version, 2 | 3) {
        return Err(PlanError::invalid(
            "Unsupported archive. Use a version 2 or 3 FinPlan input export; legacy browser and CLI archives have different formats.",
        ));
    }
    if archive.plans.is_empty() || archive.plans.len() > 100 {
        return Err(PlanError::invalid("An import must contain 1–100 plans."));
    }
    archive
        .plans
        .iter()
        .map(|value| {
            let graph: ScenarioGraph = serde_json::from_value(value.clone())
                .map_err(|_| PlanError::invalid("Invalid plan graph in archive."))?;
            validate_graph(&graph)?;
            Ok(graph)
        })
        .collect()
}

/// What importing `archive` would add, after the same checks as the import.
pub fn preview(archive: &PlanArchive) -> PlanResult<ArchivePreview> {
    let graphs = unpack(archive)?;
    Ok(ArchivePreview {
        names: graphs.iter().map(|g| g.scenario.name.clone()).collect(),
        accounts: graphs.iter().map(|g| g.accounts.len()).sum(),
        events: graphs.iter().map(|g| g.events.len()).sum(),
        assumptions: graphs
            .iter()
            .map(|g| {
                g.return_profiles.len()
                    + usize::from(g.tax_config.is_some())
                    + usize::from(g.inflation_distribution_id.is_some())
            })
            .sum(),
    })
}

/// Whether `graph` is a plan an import can restore: within the size limits,
/// with consistent identifiers and tree ownership, no cycles, valid
/// expressions and parameters, and one the engine can compile.
pub fn validate_graph(graph: &ScenarioGraph) -> PlanResult<()> {
    let size = graph.accounts.len()
        + graph.parameters.len()
        + graph.assets.len()
        + graph.events.len()
        + graph.distributions.len()
        + graph.amounts.len()
        + graph.effects.len()
        + graph.triggers.len()
        + graph.return_profiles.len()
        + graph.tax_brackets.len()
        + graph.withdrawal_sources.len()
        + graph.withdrawal_items.values().map(Vec::len).sum::<usize>()
        + graph.trigger_children.values().map(Vec::len).sum::<usize>()
        + graph.event_effects.values().map(Vec::len).sum::<usize>()
        + graph.positions.values().map(Vec::len).sum::<usize>();
    if size > 10_000 || !(1..=120).contains(&graph.scenario.duration_years) {
        return Err(PlanError::invalid(
            "Archive exceeds the plan size or horizon limit.",
        ));
    }
    let invalid = || {
        PlanError::invalid("Archive contains inconsistent record identifiers or tree ownership.")
    };
    let unique = |ids: Vec<i64>| -> bool {
        ids.iter().all(|id| *id > 0) && ids.iter().collect::<HashSet<_>>().len() == ids.len()
    };
    if !unique(graph.accounts.iter().map(|r| r.id).collect())
        || !unique(graph.assets.iter().map(|r| r.id).collect())
        || !unique(graph.events.iter().map(|r| r.id).collect())
        || !unique(graph.parameters.iter().map(|r| r.id).collect())
    {
        return Err(invalid());
    }
    macro_rules! check_rows {
        ($rows:expr) => {
            if $rows.iter().any(|(id, row)| *id <= 0 || *id != row.id) {
                return Err(invalid());
            }
        };
    }
    check_rows!(graph.distributions);
    check_rows!(graph.return_profiles);
    check_rows!(graph.amounts);
    check_rows!(graph.effects);
    check_rows!(graph.triggers);
    // Compound trigger children point back at an already inserted parent. Parent
    // pointers are ownership, not additional semantic child edges.
    for (parent, children) in &graph.trigger_children {
        if !graph.triggers.contains_key(parent) || !unique(children.clone()) {
            return Err(invalid());
        }
        for child in children {
            if graph
                .triggers
                .get(child)
                .is_none_or(|r| r.parent_id != Some(*parent) || r.event_id.is_some())
            {
                return Err(invalid());
            }
        }
    }
    for row in graph.triggers.values() {
        if let Some(parent) = row.parent_id
            && !graph
                .trigger_children
                .get(&parent)
                .is_some_and(|c| c.contains(&row.id))
        {
            return Err(invalid());
        }
        for id in [row.start_trigger_id, row.end_trigger_id]
            .into_iter()
            .flatten()
        {
            if graph
                .triggers
                .get(&id)
                .is_none_or(|r| r.parent_id.is_some() || r.event_id.is_some())
            {
                return Err(invalid());
            }
        }
    }
    for (event, root) in &graph.event_trigger {
        if !graph.events.iter().any(|e| e.id == *event)
            || graph
                .triggers
                .get(root)
                .is_none_or(|r| r.event_id != Some(*event) || r.parent_id.is_some())
        {
            return Err(invalid());
        }
    }
    for (event, effects) in &graph.event_effects {
        if !graph.events.iter().any(|e| e.id == *event) || !unique(effects.clone()) {
            return Err(invalid());
        }
        for effect in effects {
            if graph
                .effects
                .get(effect)
                .is_none_or(|r| r.event_id != Some(*event) || r.parent_id.is_some())
            {
                return Err(invalid());
            }
        }
    }
    for ((parent, slot), child) in &graph.effect_children {
        if !graph.effects.contains_key(parent)
            || !["on_true", "on_false"].contains(&slot.as_str())
            || graph.effects.get(child).is_none_or(|r| {
                r.parent_id != Some(*parent)
                    || r.parent_slot.as_ref() != Some(slot)
                    || r.event_id.is_some()
            })
        {
            return Err(invalid());
        }
    }
    for row in graph.effects.values() {
        if let Some(parent) = row.parent_id
            && row.parent_slot.as_ref().is_none_or(|slot| {
                graph.effect_children.get(&(parent, slot.clone())) != Some(&row.id)
            })
        {
            return Err(invalid());
        }
    }
    // Guard even unused rows before the recursive copier sees them.
    check_dag(graph.distributions.iter().map(|(id, row)| {
        (
            *id,
            [row.bull_id, row.bear_id].into_iter().flatten().collect(),
        )
    }))?;
    check_dag(graph.amounts.iter().map(|(id, row)| {
        (
            *id,
            [row.left_id, row.right_id].into_iter().flatten().collect(),
        )
    }))?;
    check_dag(graph.triggers.iter().map(|(id, row)| {
        let mut children = graph.trigger_children.get(id).cloned().unwrap_or_default();
        children.extend(
            [row.start_trigger_id, row.end_trigger_id]
                .into_iter()
                .flatten(),
        );
        (*id, children)
    }))?;
    check_dag(graph.effects.keys().map(|id| {
        (
            *id,
            graph
                .effect_children
                .iter()
                .filter_map(|((parent, _), child)| (parent == id).then_some(*child))
                .collect(),
        )
    }))?;
    let (_, metadata, parameters) = compile::expression_context(graph)
        .map_err(|e| PlanError::invalid(format!("Archive parameter is invalid: {e}")))?;
    for row in graph.triggers.values() {
        if let Some(id) = row.parameter_id {
            let required = match row.kind.as_str() {
                "Date" => "Date",
                "Age" => "Age",
                _ => return Err(invalid()),
            };
            if graph
                .parameters
                .iter()
                .find(|p| p.id == id)
                .is_none_or(|p| p.kind != required)
            {
                return Err(invalid());
            }
        }
    }
    for row in graph.amounts.values() {
        if let Some(source) = &row.expression_source {
            finplan_core::expression::compile_amount(source, &metadata, &parameters)
                .map_err(|e| PlanError::invalid(format!("Archive expression is invalid: {e}")))?;
            if graph
                .amounts
                .values()
                .any(|parent| parent.left_id == Some(row.id) || parent.right_id == Some(row.id))
            {
                return Err(PlanError::invalid(
                    "Archive nests an Expression inside a legacy amount",
                ));
            }
        }
    }
    // The funding policy holds to what `PUT …/funding` accepts.
    let policy = graph.funding();
    let stray = policy.is_none()
        && (graph.scenario.funding_strategy.is_some()
            || graph.scenario.funding_bracket_ceiling.is_some()
            || !graph.funding_excludes.is_empty());
    let mut excludes = graph.funding_excludes.clone();
    excludes.sort_unstable();
    excludes.dedup();
    if stray
        || policy.is_some_and(|p| p.check().is_err())
        || excludes.len() != graph.funding_excludes.len()
        || excludes.iter().any(|id| !graph.investment.contains_key(id))
    {
        return Err(PlanError::invalid("Archive funding policy is invalid."));
    }
    compile::compile(graph)
        .map_err(|e| PlanError::invalid(format!("Archive plan cannot be compiled: {e}")))?;
    Ok(())
}

/// `rows` as `(id, children)` is a forest no deeper than 32 whose every child
/// is itself a row.
fn check_dag(rows: impl Iterator<Item = (i64, Vec<i64>)>) -> PlanResult<()> {
    let edges: HashMap<_, _> = rows.collect();
    fn visit(
        id: i64,
        edges: &HashMap<i64, Vec<i64>>,
        path: &mut HashSet<i64>,
        depth: usize,
        heights: &mut HashMap<i64, usize>,
    ) -> PlanResult<usize> {
        if let Some(height) = heights.get(&id) {
            if depth + height > 32 {
                return Err(PlanError::invalid("Archive nesting exceeds limit."));
            }
            return Ok(*height);
        }
        if depth > 32 || !path.insert(id) {
            return Err(PlanError::invalid(
                "Archive contains a cycle or excessive nesting.",
            ));
        }
        let children = edges
            .get(&id)
            .ok_or_else(|| PlanError::invalid("Archive contains a missing reference."))?;
        let mut height = 0;
        for child in children {
            height = height.max(1 + visit(*child, edges, path, depth + 1, heights)?);
        }
        path.remove(&id);
        heights.insert(id, height);
        Ok(height)
    }
    let mut heights = HashMap::new();
    for id in edges.keys() {
        visit(*id, &edges, &mut HashSet::new(), 0, &mut heights)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph() -> ScenarioGraph {
        serde_json::from_str(include_str!("../testdata/default_snapshot.json")).unwrap()
    }

    #[test]
    fn the_funding_policy_travels_and_is_checked() {
        let mut plan = graph();
        plan.scenario.funding_strategy = Some("BracketFilling".into());
        plan.scenario.funding_bracket_ceiling = Some(0.22);
        plan.funding_excludes = vec![2];
        let back = unpack(&pack(vec![plan.clone()]).unwrap()).unwrap();
        assert_eq!(back[0].funding(), plan.funding());

        // An archive from before the policy has none.
        assert!(
            unpack(&pack(vec![graph()]).unwrap()).unwrap()[0]
                .funding()
                .is_none()
        );

        for bad in [
            |g: &mut ScenarioGraph| g.funding_excludes = vec![6],
            |g: &mut ScenarioGraph| g.funding_excludes = vec![2, 2],
            |g: &mut ScenarioGraph| g.scenario.funding_strategy = Some("Nope".into()),
            |g: &mut ScenarioGraph| g.scenario.funding_bracket_ceiling = Some(1.5),
        ] {
            let mut g = plan.clone();
            bad(&mut g);
            assert!(validate_graph(&g).is_err());
        }
    }
}
