# finplan_plan: a database-free plan crate

Status: proposed (2026-10-03). Second of three:
[17](17_guest_access.md) guest access, this refactor, then
[19](19_local_first_wasm.md) local-first in the browser. This is a pure
refactor: the server's behaviour, API and generated bindings do not change.
It pays off on its own by giving the plan model one home that tests can reach
without SQLite, and it is what 19 compiles to WebAssembly.

## Why

What a plan *is* lives inside `finplan_server`, mixed with HTTP and SQL:

- `compile/rows.rs`: the row structs and `ScenarioGraph`, with
  `sqlx::FromRow` derives and the bulk loader in the same file.
- `compile/mod.rs` and `idmap.rs`: graph → `SimulationConfig`. Already pure
  apart from `ApiError` and the row types.
- `domain/edit.rs`: every write route re-implemented over an in-memory
  `ScenarioGraph`, "the same validation, the same rows … the same cascades",
  so preview can simulate an edit without writing. It imports its request
  types and helpers from `api::{accounts, assets, events, expression_refs,
  expressions, parameters, profiles, scenarios, taxes}`.
- `api/row_batch.rs`: lowered rows with two sinks, `insert` (SQL) and
  `merge_into` (in-memory graph).
- `runner/inputs.rs`: the canonical snapshot, its hash and `MODEL_VERSION`.
- `api/funding.rs` `funding_view` and the result shaping in `runner/store.rs`
  and `api/runs.rs`.

The browser needs all of this and none of axum, sqlx or tokio. Moving it out
also turns the "in-memory edit mirrors the route" promise from a convention
into a structure: the route becomes "load graph, call the plan crate, write
rows", so the two can't drift.

## Target layout

```
crates/
  finplan_core/    engine (unchanged API; wasm-clean, see "Core changes")
  finplan_plan/    NEW: plan model, specs, validation, edits, compile, snapshot, results
  finplan_server/  HTTP, SQL, auth, billing, jobs, AI; depends on finplan_plan
  finplan/         TUI (unchanged)
```

`finplan_plan` depends on `finplan_core`, `serde`, `serde_json`, `jiff`,
`sha2` and `ts-rs`. It must not depend on `sqlx`, `tokio`, `axum` or anything
with I/O. A CI job builds it for `wasm32-unknown-unknown` to keep that true.

### What moves

| From `finplan_server` | To `finplan_plan` |
|---|---|
| `compile/rows.rs` structs, `ScenarioGraph` and its serde helpers | `graph` |
| `compile/mod.rs`, `compile/idmap.rs` | `compile` |
| Request specs: `CreateAccount`, `FlavorSpec`, `CreatePosition`, `CreateAsset`, `EventBody`, `ParameterBody`, `CreateProfile`, `DistributionSpec`, `UpdateScenario`, `CreateTaxConfig`, `api::specs`, … | `specs` |
| `api/events.rs` `lower_tree`, `api/expressions.rs` `validate_tree`, `api/expression_refs.rs`, `api/parameters.rs` `delete_refusal` | `specs` / `validate` |
| `api/row_batch.rs` `RowBatch`, `merge_into` | `batch` |
| `domain/edit.rs` and `edit_tests.rs` (the pure parts) | `edit` |
| `runner/inputs.rs` `snapshot`, `canonical_json`, `MODEL_VERSION` | `snapshot` |
| `api/funding.rs` `funding_view` and the result view types | `results` |
| `runner/ledger.rs` (flattens `LedgerEntry` into display rows; no SQL) | `results::ledger` |
| `suggest/templates` (no SQL today) | `templates` |

### What stays

- `ScenarioGraph::load` / `load_connection` become `db::graph::load(conn,
  scenario_id, user_id)` in the server. An inherent `impl` can't live outside
  the crate that defines the type, and the loader is SQL anyway.
- `RowBatch::insert` becomes `db::batch::insert(conn, &batch)`.
- `domain::clone_scenario` / `clone_into` (SQL with id remaps).
- Everything under `auth`, `billing`, `runner` (queue, workers, SQL
  persistence), `analysis` jobs, `suggest` (for now, see phase 5),
  `documents`, `observability`.

## Mechanics

### Errors

`compile` and the edits return `ApiError`, which is HTTP-shaped. Add a small
error type to the plan crate:

```rust
pub enum PlanError {
    Invalid(String),        // -> ApiError::bad_request
    Unprocessable(String),  // -> ApiError::unprocessable
    NotFound(&'static str), // -> ApiError::NotFound
    Conflict(String),       // -> ApiError::Conflict
}
```

The server implements `From<PlanError> for ApiError`, so call sites keep their
`?`. Messages are moved verbatim, because the web and the tests match on some
of them.

### Row derives without sqlx

The row structs derive `sqlx::FromRow`, and one field is a
`sqlx::types::Json<T>`: `InvestmentRow::catch_up`. Give `finplan_plan` an
optional `sqlx` feature:

```rust
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct InvestmentRow {
    #[cfg_attr(feature = "sqlx", sqlx(json))]
    pub catch_up: Vec<CatchUpSpec>,
    ...
}
```

The server enables the feature; the WASM build doesn't. sqlx 0.8 supports
`#[sqlx(json)]` on `FromRow` fields, which replaces the `Json<T>` wrapper, so
plan code reads plain `T`. Serde output must be unchanged; the snapshot hash
test below checks that.

### Ids

The graph keys rows by database `i64` ids, and `merge_into` already assigns
in-memory ids as `max + 1` per table. Keep `i64` ids. Add
`ScenarioGraph::next_id(table)` so the local store in 19 and `merge_into`
share one allocator. Nothing about ids changes for the server.

### ts-rs

The specs derive `ts_rs::TS` with `#[ts(export)]`. They keep the same names,
so `.cargo/config.toml`'s `TS_RS_EXPORT_DIR` (workspace-relative) still
writes them into `web/lib/api/generated/`. Exit check: run
`./scripts/gen-bindings.sh` and `git diff --exit-code web/lib/api/generated`
shows no change.

### Re-exports

Each `api::*` module re-exports what moved (`pub use finplan_plan::specs::
CreateAccount;`), so handlers, `suggest` and tests compile unchanged during
the move. Remove the re-exports in the last phase.

### Results without the database

Today a run's results are written to the `run_*` tables (`runner/store.rs`)
and read back and shaped by SQL in `api/runs.rs` (`results`, the real-dollar
quantiles, the ledger pages). The browser has no tables, so 19 needs the same
`Results` built straight from a `MonteCarloSummary`.

Add `finplan_plan::results::project(&CompiledScenario, &MonteCarloSummary,
&RunSettings) -> RunResults`, where `RunResults` holds everything the results
endpoints return, and change `store.rs` to persist from it instead of from the
summary. The read path stays SQL. A golden test runs fixture plans through
both paths and asserts the `GET /runs/{id}/results` body equals the projection
(rounding included). This is the riskiest step; it is its own phase.

## Core changes (wasm-clean)

`finplan_core` must also build for `wasm32-unknown-unknown`:

- `rayon`: add a default-on `parallel` feature. Without it, the batch loop in
  `monte_carlo_simulate_with_config` uses `into_iter` instead of
  `into_par_iter` (same batches, same seeds, same merge). `optimization/
  grid_search.rs` does the same.
- `rand::rng()` (unseeded runs, `simulation.rs:957`): on wasm32, `getrandom`
  needs the `wasm_js` backend (getrandom 0.3: a target-specific dependency
  with that feature plus the `getrandom_backend="wasm_js"` cfg). The
  alternative is to require a seed on wasm and draw it in JavaScript with
  `crypto.getRandomValues`. Prefer the latter; it keeps core free of
  JavaScript glue.
- `jiff`: pure Rust. Plans use civil dates only, so check that nothing on the
  plan path asks for the system time zone or `Zoned::now()`; on wasm32 those
  need jiff's `js` feature.

## Phases

Each phase is one or more commits with `cargo test`, `cargo clippy` and the
bindings check green, and no behaviour change.

1. **Crate and graph.** Create `finplan_plan`; move the row structs and
   `ScenarioGraph` behind the `sqlx` feature; move the loader to
   `db::graph`; add `PlanError`. Compile and edits still live in the server.
2. **Compile and snapshot.** Move `compile`, `idmap`, `snapshot`,
   `MODEL_VERSION`. Test that the snapshot JSON and hash for every fixture
   are byte-identical before and after (record them in phase 1).
3. **Specs, validation, batch, edits.** Move the request specs, `lower_tree`,
   `validate_tree`, `expression_refs`, `delete_refusal`, `RowBatch`
   (`merge_into` only) and `domain::edit`. Write routes become thin: load,
   call the shared validate/lower, `db::batch::insert`. `edit_tests.rs` moves
   with it; tests that need SQL to compare against stay in the server as
   "route equals edit" tests.
4. **Results projection.** `results::project`, `funding_view`, and the
   golden equivalence test against the SQL read path.
5. **Core wasm-clean, plus CI.** The `parallel` feature, seeding on wasm, and
   a CI step: `cargo build -p finplan_plan --target wasm32-unknown-unknown
   --no-default-features`. Optionally move `suggest/rules` (the rule-based
   review notes, no model and no SQL today) here too, along with the parts
   of `suggest` it needs (`Change`, diffs); 19 wants them for local plans.

## Exit criteria

- `cargo tree -p finplan_plan --no-default-features` contains no `sqlx`,
  `tokio`, `axum`, `hyper` or `reqwest`.
- `finplan_plan` and `finplan_core` build for `wasm32-unknown-unknown`.
- `web/lib/api/generated/` is unchanged.
- Snapshot hashes for fixtures are unchanged, so existing runs stay "fresh"
  (`input_hash` is compared to decide whether a run is stale).
- `MODEL_VERSION` is not bumped: nothing about compilation changed.
- `cargo test` passes with the same test count, adjusted for tests that
  moved crates.

## Risks

- **Import cycles.** `domain::edit` pulls in helpers from route modules that
  may themselves use SQL. Phase 3 finds them; anything that needs SQL to
  validate (for example a cross-scenario reference check) gets split into a
  pure check in the plan crate plus an SQL check in the route.
- **Serde drift.** Moving `Json<T>` to `#[sqlx(json)]` must not change the
  serialized snapshot. The phase 2 byte-identity test catches it.
- **Size of the move.** Roughly 7k lines move (`domain` 3.8k with tests,
  `compile` 2.1k, row batch, specs, funding). Do it in the phases above, not
  one commit, so review stays possible.
