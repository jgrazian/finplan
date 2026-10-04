//! The `#[wasm_bindgen]` wrappers: argument conversion and error throwing, and
//! nothing else. What each does is documented on the function it calls.

use wasm_bindgen::prelude::*;

use crate::error::{EngineError, EngineResult};
use crate::{plans, reads, results, runs};

/// A failed call throws the JSON of its `EngineError`.
fn js<T>(result: EngineResult<T>) -> Result<T, String> {
    result.map_err(|error| error.to_json())
}

/// An id or seed from JS: a whole number a double holds exactly.
fn whole(value: f64, what: &str) -> Result<i64, String> {
    if value.is_finite() && value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0 {
        Ok(value as i64)
    } else {
        Err(EngineError::bad_request(format!("{what} must be a whole number")).to_json())
    }
}

/// A panic is a trap, and the instance is unusable after it. Say so in the
/// shape every other failure has, rather than as a bare `unreachable`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
fn start() {
    std::panic::set_hook(Box::new(|info| {
        wasm_bindgen::throw_str(&EngineError::new(500, "panic", info.to_string()).to_json())
    }));
}

/// The engine's `MODEL_VERSION`.
#[wasm_bindgen]
pub fn model_version() -> String {
    plans::model_version().to_string()
}

/// The starter library: `Library` JSON.
#[wasm_bindgen]
pub fn library_seed() -> Result<String, String> {
    js(plans::library_seed_json())
}

/// See [`plans::new_plan_json`].
#[wasm_bindgen]
pub fn new_plan(body: &str, library: &str, id: f64, now: &str) -> Result<String, String> {
    js(plans::new_plan_json(body, library, whole(id, "id")?, now))
}

/// See [`plans::duplicate_plan_json`].
#[wasm_bindgen]
pub fn duplicate_plan(graph: &str, new_id: f64, name: &str, now: &str) -> Result<String, String> {
    js(plans::duplicate_plan_json(
        graph,
        whole(new_id, "id")?,
        name,
        now,
    ))
}

/// See [`plans::apply_edit_json`].
#[wasm_bindgen]
pub fn apply_edit(
    graph: &str,
    library: &str,
    op: &str,
    now: Option<String>,
) -> Result<String, String> {
    js(plans::apply_edit_json(graph, library, op, now.as_deref()))
}

/// See [`plans::apply_library_json`].
#[wasm_bindgen]
pub fn apply_library(
    library: &str,
    plans: &str,
    op: &str,
    now: Option<String>,
) -> Result<String, String> {
    js(plans::apply_library_json(
        library,
        plans,
        op,
        now.as_deref(),
    ))
}

/// See [`reads::read_json`].
#[wasm_bindgen]
pub fn read(graph: &str, library: &str, query: &str) -> Result<String, String> {
    js(reads::read_json(graph, library, query))
}

/// See [`reads::read_library_json`].
#[wasm_bindgen]
pub fn read_library(library: &str, plans: &str, query: &str) -> Result<String, String> {
    js(reads::read_library_json(library, plans, query))
}

/// See [`plans::snapshot_json`].
#[wasm_bindgen]
pub fn snapshot(graph: &str, library: &str) -> Result<String, String> {
    js(plans::snapshot_json(graph, library))
}

/// See [`plans::export_archive_json`].
#[wasm_bindgen]
pub fn export_archive(graphs: &str) -> Result<String, String> {
    js(plans::export_archive_json(graphs))
}

/// See [`plans::import_archive_json`].
#[wasm_bindgen]
pub fn import_archive(archive: &str) -> Result<String, String> {
    js(plans::import_archive_json(archive))
}

/// See [`plans::preview_archive_json`].
#[wasm_bindgen]
pub fn preview_archive(archive: &str) -> Result<String, String> {
    js(plans::preview_archive_json(archive))
}

/// See [`runs::run_cost_json`].
#[wasm_bindgen]
pub fn run_cost(graph: &str, settings: &str) -> Result<String, String> {
    js(runs::run_cost_json(graph, settings))
}

/// See [`runs::coordinator_new`].
#[wasm_bindgen]
pub fn coordinator_new(snapshot: &str, settings: &str) -> Result<u32, String> {
    js(runs::coordinator_new(snapshot, settings))
}

/// See [`runs::coordinator_info_json`].
#[wasm_bindgen]
pub fn coordinator_info(handle: u32) -> Result<String, String> {
    js(runs::coordinator_info_json(handle))
}

/// See [`runs::coordinator_next_round`]. `undefined` when the run is over.
#[wasm_bindgen]
pub fn coordinator_next_round(handle: u32) -> Result<Option<String>, String> {
    js(runs::coordinator_next_round(handle))
}

/// See [`runs::coordinator_absorb`].
#[wasm_bindgen]
pub fn coordinator_absorb(handle: u32, outputs: &str) -> Result<(), String> {
    js(runs::coordinator_absorb(handle, outputs))
}

/// See [`runs::coordinator_completed`].
#[wasm_bindgen]
pub fn coordinator_completed(handle: u32) -> Result<u32, String> {
    js(runs::coordinator_completed(handle))
}

/// See [`runs::coordinator_finish`].
#[wasm_bindgen]
pub fn coordinator_finish(handle: u32) -> Result<String, String> {
    js(runs::coordinator_finish(handle))
}

/// See [`runs::coordinator_drop`].
#[wasm_bindgen]
pub fn coordinator_drop(handle: u32) {
    runs::coordinator_drop(handle);
}

/// See [`runs::prepare`].
#[wasm_bindgen]
pub fn prepare(snapshot: &str, settings: &str) -> Result<u32, String> {
    js(runs::prepare(snapshot, settings))
}

/// See [`runs::run_batch_json`].
#[wasm_bindgen]
pub fn run_batch(handle: u32, spec: &str) -> Result<String, String> {
    js(runs::run_batch_json(handle, spec))
}

/// See [`runs::release`].
#[wasm_bindgen]
pub fn release(handle: u32) {
    runs::release(handle);
}

/// See [`results::results_view_json`].
#[wasm_bindgen]
pub fn results_view(
    run_results: &str,
    run_id: f64,
    scenario_id: f64,
    series: Option<String>,
) -> Result<String, String> {
    js(results::results_view_json(
        run_results,
        whole(run_id, "run_id")?,
        whole(scenario_id, "scenario_id")?,
        series.as_deref(),
    ))
}

/// See [`results::ledger_page_json`].
#[wasm_bindgen]
pub fn ledger_page(run_results: &str, run_id: f64, query: &str) -> Result<String, String> {
    js(results::ledger_page_json(
        run_results,
        whole(run_id, "run_id")?,
        query,
    ))
}
