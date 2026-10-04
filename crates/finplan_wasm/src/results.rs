//! A stored run, read back.
//!
//! The browser keeps a finished run as its `RunResults` JSON (what
//! `coordinator_finish` returns, or what the server's offload route returns), and
//! these answer the screens' reads from it: what `GET /runs/{id}/results` and
//! `GET /runs/{id}/ledger` answer on the server.

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
