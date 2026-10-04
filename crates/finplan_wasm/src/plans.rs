//! Making, editing, snapshotting and archiving plans: the store worker's side.
//!
//! Each function is the in-memory twin of what the server does after loading a
//! plan from SQLite. A plan travels as the JSON of a `ScenarioGraph` (opaque to
//! TypeScript, `unknown` in the bindings: it is stored and handed back, never
//! read), and the user's library as the JSON of a `Library`. A serialized graph
//! carries its return profiles and distributions but not its tax configs or
//! inflation profiles, so every function that reads or edits one is given the
//! library and attaches it first, as a load would.

use finplan_plan::archive::{self, ArchivePreview, PlanArchive};
use finplan_plan::create;
use finplan_plan::edit::{self, EditOp, EditOutcome};
use finplan_plan::graph::ScenarioGraph;
use finplan_plan::library::{self, Library, LibraryOp};
use finplan_plan::specs::scenarios::CreateScenario;
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use crate::error::{EngineError, EngineResult, parse, to_json};

/// The engine's `MODEL_VERSION`, which a stored run records so a later session
/// knows which engine produced it. Equal to the server's for the same build.
pub fn model_version() -> &'static str {
    finplan_plan::snapshot::MODEL_VERSION
}

/// The starter library every new account is given: `Library` JSON.
pub fn library_seed_json() -> EngineResult<String> {
    to_json(&library::seed())
}

/// `POST /scenarios`, locally: `body` is a `CreateScenario`. `id` is the new
/// plan's id (the store hands out local ids) and `now` its creation time in
/// SQLite's `datetime('now')` form, `2026-10-03 14:05:09`. Returns the new
/// plan's `ScenarioGraph` JSON. Refusals are the route's.
pub fn new_plan_json(body: &str, library: &str, id: i64, now: &str) -> EngineResult<String> {
    let body: CreateScenario = parse("scenario body", body)?;
    let library: Library = parse("library", library)?;
    to_json(&create::new_plan(&body, &library, id, now)?)
}

/// `POST /scenarios/{id}/duplicate`, locally: the plan as a plan of its own,
/// named `name`. Returns the copy's `ScenarioGraph` JSON.
pub fn duplicate_plan_json(
    graph: &str,
    new_id: i64,
    name: &str,
    now: &str,
) -> EngineResult<String> {
    let graph: ScenarioGraph = parse("plan", graph)?;
    to_json(&create::duplicate(&graph, new_id, name, now)?)
}

/// What [`apply_edit_json`] returns.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct EditResult {
    /// The edited plan: a `ScenarioGraph`, to be stored in place of the old one.
    #[ts(type = "unknown")]
    pub graph: Value,
    /// The id of the row a `create_*` op made; null for every other op.
    pub outcome: EditOutcome,
}

/// Every plan-scoped write route, locally. `op` is an `EditOp`
/// (`{"op": "update_asset", "id": 7, "body": {...}}`). The library is attached
/// to the plan first (so an edit may switch the plan onto another tax config).
///
/// Atomic: an error means nothing changed, and nothing was returned to store.
/// With `now`, the plan's `updated_at` is set to it (the edits do not model
/// bookkeeping, and the server's routes bump it).
pub fn apply_edit_json(
    graph: &str,
    library: &str,
    op: &str,
    now: Option<&str>,
) -> EngineResult<String> {
    let mut graph: ScenarioGraph = parse("plan", graph)?;
    let library: Library = parse("library", library)?;
    let op: EditOp = parse("edit", op)?;
    library.attach(&mut graph);
    let outcome = edit::apply(&mut graph, &op)?;
    if let Some(now) = now {
        graph.scenario.updated_at = now.to_string();
    }
    to_json(&EditResult {
        graph: serde_json::to_value(&graph).map_err(|e| EngineError::internal(e.to_string()))?,
        outcome,
    })
}

/// What [`apply_library_json`] returns.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct LibraryEditResult {
    /// The edited library: `Library`, to be stored in place of the old one.
    pub library: Library,
    /// The id of the row a `create_*` op made; null for every other op.
    pub outcome: EditOutcome,
    /// The plans the edit reached (their copies of a changed profile or tax
    /// config, or their reference to a deleted one, follow the library), each a
    /// `ScenarioGraph` to be stored in place of the old one. Plans the edit did
    /// not touch are not here.
    #[ts(type = "unknown[]")]
    pub changed_plans: Vec<Value>,
}

/// Every library write route (return profiles, inflation profiles, tax
/// configs), locally. `op` is a `LibraryOp`; `plans` is every plan of the
/// device, as `ScenarioGraph` JSON: what a database sees through its foreign
/// keys, and where a return profile that is still in use is found.
///
/// Atomic, and `now` bumps `updated_at` on the plans that changed, as
/// [`apply_edit_json`] does.
pub fn apply_library_json(
    library: &str,
    plans: &str,
    op: &str,
    now: Option<&str>,
) -> EngineResult<String> {
    let mut library: Library = parse("library", library)?;
    let mut plans: Vec<ScenarioGraph> = parse("plans", plans)?;
    let op: LibraryOp = parse("library edit", op)?;
    let (outcome, changed) = library::apply_library_to_plans(&mut library, &mut plans, &op, now)?;
    let changed_plans = plans
        .iter()
        .filter(|plan| changed.contains(&plan.scenario.id))
        .map(|plan| serde_json::to_value(plan).map_err(|e| EngineError::internal(e.to_string())))
        .collect::<EngineResult<Vec<_>>>()?;
    to_json(&LibraryEditResult {
        library,
        outcome,
        changed_plans,
    })
}

/// What [`snapshot_json`] returns.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SnapshotResult {
    /// The canonical input snapshot: what a run is made from, and what
    /// `coordinator_new`/`prepare` take.
    pub json: String,
    /// Its SHA-256, equal to the server's `input_hash` for the same plan.
    pub hash: String,
}

/// The plan's canonical input snapshot and its hash (the library attached
/// first, as the server's load does).
pub fn snapshot_json(graph: &str, library: &str) -> EngineResult<String> {
    let mut graph: ScenarioGraph = parse("plan", graph)?;
    let library: Library = parse("library", library)?;
    library.attach(&mut graph);
    let (json, hash) = finplan_plan::snapshot::snapshot(&graph)
        .map_err(|error| EngineError::bad_request(error.to_string()))?;
    to_json(&SnapshotResult { json, hash })
}

/// The `PlanArchive` of `graphs` (a `ScenarioGraph[]`): the file for "Export",
/// and what "Move to cloud" posts. The owner is cleared.
pub fn export_archive_json(graphs: &str) -> EngineResult<String> {
    let graphs: Vec<ScenarioGraph> = parse("plans", graphs)?;
    to_json(&archive::pack(graphs)?)
}

/// The plans in a `PlanArchive`, each checked as an import checks it (size,
/// identifiers, nesting, expressions, compilability). Returns a
/// `ScenarioGraph[]`; the caller gives each its local id and merges its
/// library tables.
pub fn import_archive_json(archive: &str) -> EngineResult<String> {
    let archive: PlanArchive = parse("archive", archive)?;
    to_json(&archive::unpack(&archive)?)
}

/// What importing a `PlanArchive` would add, after the same checks as the
/// import: `ArchivePreview` JSON.
pub fn preview_archive_json(archive: &str) -> EngineResult<String> {
    let archive: PlanArchive = parse("archive", archive)?;
    let preview: ArchivePreview = archive::preview(&archive)?;
    to_json(&preview)
}
