//! FinPlan in the browser: the plan model and the engine behind a thin
//! `wasm-bindgen` layer (spec 19).
//!
//! There is no logic here. Every export parses JSON, calls `finplan_plan` or
//! `finplan_core`, and writes JSON back, so the browser answers exactly what the
//! server answers (same types, same refusals, same hash). The JSON types are the
//! ones the server already generates TypeScript for, in
//! `web/lib/api/generated`; the few shapes this layer adds (`EngineError`,
//! `ReadQuery`, ...) are generated beside them.
//!
//! Each export is a plain function of `&str`s returning `Result<String, String>`
//! (`x_json` in the modules below, so the same code runs natively in the
//! workspace's tests) wrapped for JS in [`exports`].
//!
//! # Errors
//!
//! A failed call throws a string: the JSON of an [`error::EngineError`],
//! `{status, code, message, body?}`, with the status and code the server would
//! answer, so it maps straight onto `ApiError`. Parse failures name a position,
//! never the text (it is a plan).
//!
//! # Exports
//!
//! JS names are the Rust names. `Library`, `ScenarioGraph` and the rest are JSON
//! strings; `ScenarioGraph` is opaque to TypeScript (store it, hand it back).
//! Numbers are JS numbers (ids and seeds are below 2^53).
//!
//! | export | in | out |
//! |---|---|---|
//! | `model_version()` | | the engine's `MODEL_VERSION` |
//! | `library_seed()` | | `Library` |
//! | `new_plan(body, library, id, now)` | `CreateScenario`, `Library` | `ScenarioGraph` |
//! | `duplicate_plan(graph, new_id, name, now)` | | `ScenarioGraph` |
//! | `apply_edit(graph, library, op, now?)` | `EditOp` | `EditResult` |
//! | `apply_library(library, plans, op, now?)` | `LibraryOp`, `ScenarioGraph[]` | `LibraryEditResult` |
//! | `read(graph, library, query)` | `ReadQuery` | by query, see [`reads`] |
//! | `read_library(library, plans, query)` | `LibraryReadQuery` | by query |
//! | `snapshot(graph, library)` | | `SnapshotResult` `{json, hash}` |
//! | `export_archive(graphs)` | `ScenarioGraph[]` | `PlanArchive` |
//! | `import_archive(archive)` | `PlanArchive` | `ScenarioGraph[]` |
//! | `preview_archive(archive)` | `PlanArchive` | `ArchivePreview` |
//! | `run_cost(graph, settings)` | `CreateRun` | `RunCost` |
//! | `coordinator_new(snapshot, settings)` | snapshot JSON, `CreateRun` | handle |
//! | `coordinator_info(h)` | | `RunInfo` |
//! | `coordinator_next_round(h)` | | `BatchSpec[]` or `undefined` when over |
//! | `coordinator_absorb(h, outputs)` | `BatchOutput[]` | |
//! | `coordinator_completed(h)` | | iterations merged |
//! | `coordinator_finish(h)` | | `RunResults` |
//! | `coordinator_drop(h)` | | |
//! | `prepare(snapshot, settings)` | | handle |
//! | `run_batch(h, spec)` | `BatchSpec` | `BatchOutput` |
//! | `release(h)` | | |
//! | `results_view(run_results, run_id, scenario_id, series?)` | `RunResults` | `Results` |
//! | `ledger_page(run_results, run_id, query)` | `LedgerQuery` | `LedgerPage` |
//!
//! `BatchSpec`, `BatchOutput` and `RunResults` have no generated TypeScript:
//! treat the first two as opaque strings (see [`runs`] for why) and the last as
//! a stored blob that `results_view` and `ledger_page` read.

// An `EngineError` carries the server's whole error body; it is built once per
// refusal, off every hot path, so its size is not worth boxing it for.
#![allow(clippy::result_large_err)]

pub mod error;
pub mod plans;
pub mod reads;
pub mod results;
pub mod runs;

mod exports;

#[cfg(test)]
mod tests;
