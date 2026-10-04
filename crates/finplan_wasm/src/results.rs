//! A stored run, read back.
//!
//! The browser keeps a finished run as its `RunResults` JSON (what
//! `coordinator_finish` returns, or what the server's offload route returns), and
//! these answer the screens' reads from it: what `GET /runs/{id}/results` and
//! `GET /runs/{id}/ledger` answer on the server.
//!
//! A default plan's `RunResults` is ~8 MB of JSON, and the Results screen reads
//! it several times (the median path, then p10, then p90, then a ledger page per
//! year). Parsing it for each read is the cost, so a caller that will read a run
//! more than once opens it ([`results_open`]): parse once, answer many, close.

use std::cell::RefCell;
use std::collections::HashMap;

use finplan_plan::PlanError;
use finplan_plan::results::RunResults;
use finplan_plan::results::view::LedgerQuery;

use crate::error::{EngineResult, parse, to_json};

/// `GET /runs/{id}/results`: a `Results` body from a stored `RunResults`.
/// `series` picks the path the per-account series and cash flows describe:
/// `"mean"`, or a percentile such as `"0.5"` (the nearest stored one); omitted,
/// the median. `run_id` and `scenario_id` are the caller's, copied into the
/// body.
pub fn results_view_json(
    run_results: &str,
    run_id: i64,
    scenario_id: i64,
    series: Option<&str>,
) -> EngineResult<String> {
    let run_results: RunResults = parse("run results", run_results)?;
    to_json(&run_results.results(run_id, scenario_id, series)?)
}

/// `GET /runs/{id}/ledger`: a `LedgerPage` body. `query` is a `LedgerQuery`
/// (`series`, `year`, `category`, `limit`, `offset`).
pub fn ledger_page_json(run_results: &str, run_id: i64, query: &str) -> EngineResult<String> {
    let run_results: RunResults = parse("run results", run_results)?;
    let query: LedgerQuery = parse("ledger query", query)?;
    to_json(&run_results.ledger_page(
        run_id,
        query.series.as_deref(),
        query.year,
        query.category.as_deref(),
        query.limit,
        query.offset,
    )?)
}

// ── handles ────────────────────────────────────────────────────────────────

struct Open {
    next: u32,
    runs: HashMap<u32, RunResults>,
}

thread_local! {
    static OPEN: RefCell<Open> = RefCell::new(Open { next: 1, runs: HashMap::new() });
}

/// Parse a stored `RunResults` once and keep it: returns a handle for
/// [`results_view_open`] and [`ledger_page_open`]. Close it with
/// [`results_close`].
pub fn results_open(run_results: &str) -> EngineResult<u32> {
    let run_results: RunResults = parse("run results", run_results)?;
    Ok(OPEN.with_borrow_mut(|open| {
        let handle = open.next;
        open.next += 1;
        open.runs.insert(handle, run_results);
        handle
    }))
}

fn with_open<T>(
    handle: u32,
    f: impl FnOnce(&RunResults) -> finplan_plan::PlanResult<T>,
) -> EngineResult<T> {
    OPEN.with_borrow(|open| {
        let run_results = open
            .runs
            .get(&handle)
            .ok_or(PlanError::NotFound("run results"))?;
        Ok(f(run_results)?)
    })
}

/// [`results_view_json`] over an opened run.
pub fn results_view_open(
    handle: u32,
    run_id: i64,
    scenario_id: i64,
    series: Option<&str>,
) -> EngineResult<String> {
    let results = with_open(handle, |run_results| {
        run_results.results(run_id, scenario_id, series)
    })?;
    to_json(&results)
}

/// [`ledger_page_json`] over an opened run.
pub fn ledger_page_open(handle: u32, run_id: i64, query: &str) -> EngineResult<String> {
    let query: LedgerQuery = parse("ledger query", query)?;
    let page = with_open(handle, |run_results| {
        run_results.ledger_page(
            run_id,
            query.series.as_deref(),
            query.year,
            query.category.as_deref(),
            query.limit,
            query.offset,
        )
    })?;
    to_json(&page)
}

/// Forget an opened run.
pub fn results_close(handle: u32) {
    OPEN.with_borrow_mut(|open| {
        open.runs.remove(&handle);
    });
}
