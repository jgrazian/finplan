//! The `#[wasm_bindgen]` wrappers: argument conversion and error throwing, and
//! nothing else. What each does is documented on the function it calls.

use wasm_bindgen::prelude::*;

use crate::error::{EngineError, EngineResult};
use crate::{analysis, plans, reads, results, runs};

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

/// See [`plans::setup_plan_json`].
#[wasm_bindgen]
pub fn setup_plan(body: &str, library: &str, id: f64, now: &str) -> Result<String, String> {
    js(plans::setup_plan_json(body, library, whole(id, "id")?, now))
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

/// See [`plans::restore_plan_json`].
#[wasm_bindgen]
pub fn restore_plan(
    library: &str,
    graph: &str,
    new_id: f64,
    name: &str,
    now: &str,
    suffix: &str,
) -> Result<String, String> {
    js(plans::restore_plan_json(
        library,
        graph,
        whole(new_id, "id")?,
        name,
        now,
        suffix,
    ))
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

/// See [`runs::run_batch_json`]. With `progress`, it is called as
/// `progress(done, total)` while the batch runs ([`runs::run_batch_reporting_json`]);
/// return true from it to stop.
#[wasm_bindgen]
pub fn run_batch(
    handle: u32,
    spec: &str,
    progress: Option<js_sys::Function>,
) -> Result<String, String> {
    match progress {
        Some(progress) => {
            let mut report = progress_fn(&progress);
            js(runs::run_batch_reporting_json(handle, spec, &mut report))
        }
        None => js(runs::run_batch_json(handle, spec)),
    }
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

/// See [`results::results_open`].
#[wasm_bindgen]
pub fn results_open(run_results: &str) -> Result<u32, String> {
    js(results::results_open(run_results))
}

/// See [`results::results_view_open`].
#[wasm_bindgen]
pub fn results_view_open(
    handle: u32,
    run_id: f64,
    scenario_id: f64,
    series: Option<String>,
) -> Result<String, String> {
    js(results::results_view_open(
        handle,
        whole(run_id, "run_id")?,
        whole(scenario_id, "scenario_id")?,
        series.as_deref(),
    ))
}

/// See [`results::ledger_page_open`].
#[wasm_bindgen]
pub fn ledger_page_open(handle: u32, run_id: f64, query: &str) -> Result<String, String> {
    js(results::ledger_page_open(
        handle,
        whole(run_id, "run_id")?,
        query,
    ))
}

/// See [`results::results_close`].
#[wasm_bindgen]
pub fn results_close(handle: u32) {
    results::results_close(handle);
}

/// See [`analysis::analysis_plan_json`].
#[wasm_bindgen]
pub fn analysis_plan(graph: &str, library: &str, body: &str) -> Result<String, String> {
    js(analysis::analysis_plan_json(graph, library, body))
}

/// Wrap a JS `progress(done, total)` callback. A truthy answer asks the
/// analysis to stop; a callback that throws is read as "stop" too, and the
/// exception is dropped (it cannot unwind through the engine).
fn progress_fn(callback: &js_sys::Function) -> impl FnMut(usize, usize) -> bool + '_ {
    move |done, total| {
        callback
            .call2(
                &JsValue::NULL,
                &JsValue::from_f64(done as f64),
                &JsValue::from_f64(total as f64),
            )
            .map(|answer| answer.is_truthy())
            .unwrap_or(true)
    }
}

/// See [`analysis::analysis_run_json`]. `progress(done, total)` is called as
/// simulations finish; return true from it to stop.
#[wasm_bindgen]
pub fn analysis_run(
    graph: &str,
    library: &str,
    body: &str,
    progress: &js_sys::Function,
) -> Result<String, String> {
    let mut report = progress_fn(progress);
    js(analysis::analysis_run_json(
        graph,
        library,
        body,
        &mut report,
    ))
}

/// See [`analysis::analysis_shard_json`]. `progress(done, total)` reports this
/// shard's simulations; return true from it to stop.
#[wasm_bindgen]
pub fn analysis_shard(
    graph: &str,
    library: &str,
    body: &str,
    shard: u32,
    shards: u32,
    progress: &js_sys::Function,
) -> Result<String, String> {
    let mut report = progress_fn(progress);
    js(analysis::analysis_shard_json(
        graph,
        library,
        body,
        shard as usize,
        shards as usize,
        &mut report,
    ))
}

/// See [`analysis::analysis_finish_json`]. `answers` is a `string[]` JSON of
/// the shards' results.
#[wasm_bindgen]
pub fn analysis_finish(
    graph: &str,
    library: &str,
    body: &str,
    answers: &str,
    progress: &js_sys::Function,
) -> Result<String, String> {
    let mut report = progress_fn(progress);
    js(analysis::analysis_finish_json(
        graph,
        library,
        body,
        answers,
        &mut report,
    ))
}

/// See [`analysis::quick_what_if_json`].
#[wasm_bindgen]
pub fn quick_what_if(
    graph: &str,
    library: &str,
    body: &str,
    progress: &js_sys::Function,
) -> Result<String, String> {
    let mut report = progress_fn(progress);
    js(analysis::quick_what_if_json(
        graph,
        library,
        body,
        &mut report,
    ))
}

/// See [`analysis::apply_what_if_json`].
#[wasm_bindgen]
pub fn apply_what_if(
    graph: &str,
    library: &str,
    body: &str,
    new_id: f64,
    now: &str,
) -> Result<String, String> {
    js(analysis::apply_what_if_json(
        graph,
        library,
        body,
        whole(new_id, "id")?,
        now,
    ))
}

/// See [`analysis::check_what_if_stack_json`].
#[wasm_bindgen]
pub fn check_what_if_stack(body: &str) -> Result<String, String> {
    js(analysis::check_what_if_stack_json(body))
}

/// See [`analysis::apply_note_json`]. `copy_id` is NaN, or the new plan's id
/// when the steps go to a copy named `copy_name`.
#[wasm_bindgen]
pub fn apply_note(
    graph: &str,
    library: &str,
    path_key: &str,
    steps: &str,
    copy_id: f64,
    copy_name: &str,
    now: &str,
) -> Result<String, String> {
    let copy = if copy_id.is_nan() {
        None
    } else {
        Some((whole(copy_id, "id")?, copy_name))
    };
    js(analysis::apply_note_json(
        graph, library, path_key, steps, copy, now,
    ))
}

/// See [`analysis::local_review_json`].
#[wasm_bindgen]
pub fn local_review(
    graph: &str,
    library: &str,
    run_results: &str,
    run_id: f64,
    reviewed_at: &str,
    silenced: &str,
) -> Result<String, String> {
    js(analysis::local_review_json(
        graph,
        library,
        run_results,
        whole(run_id, "run_id")?,
        reviewed_at,
        silenced,
    ))
}
