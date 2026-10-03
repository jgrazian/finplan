//! A target's current body, built from a `ScenarioGraph` in exactly the shape
//! its GET route returns — the shape a `Change` path points into.

use serde_json::Value;

use crate::graph::ScenarioGraph;
use crate::specs::parameters::ParameterValueSpec;

/// `GET /scenarios/{id}/events/{event}`, or `None` when the graph has no such
/// event (or cannot read it back).
pub fn event(graph: &ScenarioGraph, id: i64) -> Option<Value> {
    serde_json::to_value(crate::read::event(graph, id).ok()?).ok()
}

/// `GET /scenarios/{id}/assets/{asset}`.
pub fn asset(graph: &ScenarioGraph, id: i64) -> Option<Value> {
    serde_json::to_value(crate::read::asset(graph, id).ok()?).ok()
}

/// `GET /scenarios/{id}/parameters`, one entry: the parameter as the body its
/// write route takes, plus its `id`.
pub fn parameter(graph: &ScenarioGraph, id: i64) -> Option<Value> {
    let row = graph.parameters.iter().find(|p| p.id == id)?;
    let value = ParameterValueSpec::try_from(row).ok()?;
    Some(serde_json::json!({ "id": row.id, "name": row.name, "value": value }))
}

/// The scenario's settings, as `GET /scenarios/{id}` returns them (less the
/// bookkeeping columns): what a change to the `scenario` target points into.
pub fn scenario(graph: &ScenarioGraph) -> Value {
    let s = &graph.scenario;
    serde_json::json!({
        "id": s.id,
        "name": s.name,
        "description": s.description,
        "start_date": s.start_date,
        "birth_date": s.birth_date,
        "duration_years": s.duration_years,
        "inflation_profile_id": s.inflation_profile_id,
        "tax_config_id": s.tax_config_id,
    })
}

/// `GET /scenarios/{id}/accounts/{account}`: the account, its flavor fields
/// flattened in beside it, and its lots.
pub fn account(graph: &ScenarioGraph, id: i64) -> Option<Value> {
    serde_json::to_value(crate::read::account(graph, id).ok()?).ok()
}
