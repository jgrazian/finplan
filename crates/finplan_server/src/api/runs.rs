//! Simulation runs: enqueue, poll, cancel, and read results.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::session::CurrentUser;
use crate::compile::{self, rows::ScenarioGraph};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use ts_rs::TS;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/scenarios/{scenario_id}/runs",
            get(list_for_scenario).post(create),
        )
        .route("/runs/{id}", get(fetch).delete(destroy))
        .route("/runs/{id}/cancel", post(cancel))
        .route("/runs/{id}/results", get(results))
        .route("/runs/{id}/ledger", get(ledger))
}

#[derive(Debug, Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct Run {
    pub id: i64,
    pub scenario_id: i64,
    pub status: String,
    /// Fixed runs: the count. Converging runs: the minimum sample taken
    /// before the metric is first tested.
    pub iterations: i64,
    pub completed_iterations: i64,
    /// Set only on a converging run: the ceiling it may not pass, and the
    /// denominator progress should be read against.
    pub converge: bool,
    pub max_iterations: Option<i64>,
    pub seed: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

const RUN_COLUMNS: &str = "id, scenario_id, status, iterations, completed_iterations, converge,
     max_iterations, seed, error_message, created_at, started_at, finished_at";

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateRun {
    #[serde(default = "default_iterations")]
    pub iterations: i64,
    #[serde(default = "default_percentiles")]
    pub percentiles: Vec<f64>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default = "default_batch")]
    pub batch_size: i64,
    #[serde(default = "default_parallel")]
    pub parallel_batches: i64,
    #[serde(default = "yes")]
    pub compute_mean: bool,
    /// Keep sampling until the median settles instead of stopping at
    /// `iterations`, which then reads as the minimum sample to take first.
    #[serde(default)]
    pub converge: bool,
}

/// Ceiling on a converging run, before `--max-iterations` is applied.
///
/// A converging run is asked for by someone who does not want to pick a count,
/// so it needs an answer in the time a count would have taken. Ten thousand
/// iterations is roughly twice the largest fixed size the UI offers.
const CONVERGE_CEILING: i64 = 10_000;

fn default_iterations() -> i64 {
    1000
}

fn default_percentiles() -> Vec<f64> {
    vec![0.05, 0.50, 0.95]
}

fn default_batch() -> i64 {
    100
}

fn default_parallel() -> i64 {
    4
}

fn yes() -> bool {
    true
}

async fn owned_run(state: &AppState, id: i64, user_id: &str) -> ApiResult<Run> {
    let row: Option<Run> = sqlx::query_as(&format!(
        "SELECT {RUN_COLUMNS} FROM runs WHERE id = ?1 AND user_id = ?2"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    row.ok_or(ApiError::NotFound("run"))
}

async fn list_for_scenario(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
) -> ApiResult<Json<Vec<Run>>> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    let rows: Vec<Run> = sqlx::query_as(&format!(
        "SELECT {RUN_COLUMNS} FROM runs WHERE scenario_id = ?1 ORDER BY created_at DESC LIMIT 50"
    ))
    .bind(scenario_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// Queue a run. The scenario is compiled synchronously first so a misconfigured
/// plan fails immediately with a useful message instead of surfacing as a failed
/// job seconds later.
async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<CreateRun>,
) -> ApiResult<(StatusCode, Json<Run>)> {
    if body.iterations < 1 {
        return Err(ApiError::bad_request("iterations must be at least 1"));
    }
    if body.iterations as usize > state.config.max_iterations {
        return Err(ApiError::bad_request(format!(
            "iterations must not exceed {}",
            state.config.max_iterations
        )));
    }

    // A converging run's `iterations` is its minimum sample, so it is clamped
    // to the ceiling rather than refused: asking to look at the metric later
    // than the run is allowed to go just means looking at it once, at the end.
    let ceiling = body
        .converge
        .then(|| CONVERGE_CEILING.min(state.config.max_iterations as i64));
    let iterations = match ceiling {
        Some(cap) => body.iterations.min(cap),
        None => body.iterations,
    };

    let mut percentiles = body.percentiles.clone();
    percentiles.retain(|p| (0.0..=1.0).contains(p));
    percentiles.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    percentiles.dedup();
    if percentiles.is_empty() {
        return Err(ApiError::bad_request(
            "percentiles must contain at least one value in 0..1",
        ));
    }

    let graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    compile::compile(&graph)?;

    let mut tx = state.db.begin().await?;
    let run_id: i64 = sqlx::query_scalar(
        "INSERT INTO runs
            (scenario_id, user_id, iterations, seed, batch_size, parallel_batches, compute_mean,
             converge, max_iterations)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) RETURNING id",
    )
    .bind(scenario_id)
    .bind(&user.id)
    .bind(iterations)
    .bind(body.seed)
    .bind(body.batch_size.max(1))
    .bind(body.parallel_batches.max(1))
    .bind(i64::from(body.compute_mean))
    .bind(i64::from(body.converge))
    .bind(ceiling)
    .fetch_one(&mut *tx)
    .await?;

    for percentile in &percentiles {
        sqlx::query("INSERT INTO run_percentiles (run_id, percentile) VALUES (?1, ?2)")
            .bind(run_id)
            .bind(percentile)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    state.runs.enqueue(run_id);

    let row: Run = sqlx::query_as(&format!("SELECT {RUN_COLUMNS} FROM runs WHERE id = ?1"))
        .bind(run_id)
        .fetch_one(&state.db)
        .await?;

    Ok((StatusCode::ACCEPTED, Json(row)))
}

async fn fetch(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Run>> {
    Ok(Json(owned_run(&state, id, &user.id).await?))
}

async fn cancel(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Run>> {
    let run = owned_run(&state, id, &user.id).await?;

    match run.status.as_str() {
        "running" => {
            state.runs.cancel(id).await;
        }
        "queued" => {
            // Not yet claimed: mark it canceled so the worker skips it when the
            // id comes up.
            sqlx::query(
                "UPDATE runs SET status = 'canceled', finished_at = datetime('now')
                  WHERE id = ?1 AND status = 'queued'",
            )
            .bind(id)
            .execute(&state.db)
            .await?;
        }
        other => {
            return Err(ApiError::Conflict(format!(
                "run has already finished with status '{other}'"
            )));
        }
    }

    Ok(Json(owned_run(&state, id, &user.id).await?))
}

async fn destroy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let run = owned_run(&state, id, &user.id).await?;
    if run.status == "running" {
        return Err(ApiError::Conflict(
            "cancel the run before deleting it".into(),
        ));
    }

    sqlx::query("DELETE FROM runs WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(&user.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── results ─────────────────────────────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
struct StatsRow {
    num_iterations: i64,
    success_rate: f64,
    funding_success_rate: Option<f64>,
    mean_final_net_worth: f64,
    std_dev_final_net_worth: f64,
    min_final_net_worth: f64,
    max_final_net_worth: f64,
    lifetime_taxes: f64,
    converged: Option<i64>,
    convergence_metric: Option<String>,
    convergence_value: Option<f64>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Stats {
    pub num_iterations: i64,
    /// Fraction of paths with positive terminal net worth, not funding success.
    pub success_rate: f64,
    /// No settled cash shortfalls or event warnings. Null for historical runs
    /// that did not measure this; rerun instead of inferring it from snapshots.
    pub funding_success_rate: Option<f64>,
    pub mean_final_net_worth: f64,
    pub std_dev_final_net_worth: f64,
    pub min_final_net_worth: f64,
    pub max_final_net_worth: f64,
    pub lifetime_taxes: f64,
    pub converged: Option<bool>,
    pub convergence_metric: Option<String>,
    pub convergence_value: Option<f64>,
    pub percentile_values: Vec<PercentileValue>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct PercentileValue {
    pub percentile: f64,
    pub final_net_worth: f64,
}

/// A representative path ranked by terminal NOMINAL net worth, not a
/// pointwise quantile. Null percentile is the synthetic nominal mean, which
/// has no coherent ledger and must not be deflated using mean inflation.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Band {
    /// Run-local identity, also accepted by results/ledger `series` queries.
    pub path_id: String,
    pub percentile: Option<f64>,
    pub dates: Vec<String>,
    pub net_worth: Vec<f64>,
    /// Cumulative inflation at each of `dates`, on this path's own realised
    /// inflation: `real = net_worth[i] / inflation[i]`. All ones for a run
    /// stored before inflation was recorded.
    pub inflation: Vec<f64>,
}

/// Cumulative inflation for one plan year. Factor 1.0 is the plan's base
/// year; the engine uses annual factors without within-year interpolation.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct InflationPoint {
    pub year: i64,
    pub factor: f64,
}

/// What one year of the ledger holds, without the entries themselves — enough
/// for the cash-flow table to say how much is behind each row before anyone
/// expands it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct LedgerYear {
    pub year: i64,
    /// Entry counts per filter bucket, and in total.
    pub cash: i64,
    pub asset: i64,
    pub tax: i64,
    pub event: i64,
    pub total: i64,
    /// The name of the event that fired this year — retiring, a pension
    /// starting — or null for a year that only did the ordinary things.
    pub tag: Option<String>,
}

/// One flattened ledger entry.
#[derive(Debug, Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct LedgerEntry {
    pub position: i64,
    pub date: String,
    pub year: i64,
    /// `cash` | `asset` | `tax` | `event`.
    pub category: String,
    pub kind: String,
    /// Prose naming the accounts and events involved. Deliberately free of
    /// dollar figures, so the client can restate `amount` and `basis` in real
    /// dollars without rewriting it.
    pub detail: String,
    /// Signed against the plan: money in is positive, money out negative.
    pub amount: Option<f64>,
    pub basis: Option<f64>,
    pub basis_label: Option<String>,
    pub account_id: Option<i64>,
    pub event_id: Option<i64>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct AccountSeries {
    pub account_id: i64,
    pub label: String,
    pub values: Vec<f64>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CashFlow {
    pub year: i64,
    pub income: f64,
    pub expenses: f64,
    pub contributions: f64,
    pub withdrawals: f64,
    pub appreciation: f64,
    pub net_cash_flow: f64,
    pub taxes: f64,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Warning {
    pub kind: String,
    pub date: Option<String>,
    pub event_id: Option<i64>,
    pub message: String,
}

/// Real-dollar pointwise quantiles over ALL iterations, not selected paths.
#[derive(Debug, Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct RealQuantilePoint {
    pub date: String,
    pub p5: f64,
    pub p50: f64,
    pub p95: f64,
}

#[derive(Debug, Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct RealTerminalStats {
    pub base_date: String,
    pub num_iterations: i64,
    pub mean: f64,
    /// Population standard deviation.
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct RealNetWorthSummary {
    /// Each iteration is deflated before aggregation. Annual factors relative
    /// to base_date; no within-year interpolation. Includes warning paths;
    /// hard simulation errors or invalid numbers fail the entire run.
    pub terminal: RealTerminalStats,
    /// Plan start, Dec 31 checkpoints, terminal date; duplicate dates collapsed.
    /// Exact type-7 quantiles: linear interpolation at (N - 1) * p.
    /// This envelope has no path ID, account decomposition or ledger.
    pub points: Vec<RealQuantilePoint>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Results {
    pub run_id: i64,
    pub scenario_id: i64,
    pub stats: Stats,
    pub bands: Vec<Band>,
    /// Null for historical runs; never inferred from stored representative paths.
    pub real_net_worth: Option<RealNetWorthSummary>,
    /// Actual run-local path ID shared by accounts, cash flows and ledger.
    pub series_id: String,
    /// Per-account decomposition of the path named by `series_percentile`.
    pub account_series: Vec<AccountSeries>,
    pub series_percentile: Option<f64>,
    pub cash_flows: Vec<CashFlow>,
    pub warnings: Vec<Warning>,
    /// Cumulative inflation on the same path as `cash_flows`, one point per
    /// plan year. Empty for a run stored before inflation was recorded.
    pub inflation: Vec<InflationPoint>,
    /// Per-year ledger index, for the years the ledger covers.
    pub ledger_years: Vec<LedgerYear>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ResultsQuery {
    /// Which path the per-account series and cash flows describe. Defaults to
    /// the terminal nominal median-ranked path; `mean` is a synthetic nominal average.
    #[serde(default)]
    pub series: Option<String>,
}

async fn results(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<ResultsQuery>,
) -> ApiResult<Json<Results>> {
    let run = owned_run(&state, id, &user.id).await?;

    if run.status != "succeeded" {
        return Err(ApiError::Conflict(format!(
            "run is '{}'; results are only available once it has succeeded",
            run.status
        )));
    }

    let stats_row: StatsRow = sqlx::query_as(
        "SELECT num_iterations, success_rate, funding_success_rate, mean_final_net_worth, std_dev_final_net_worth,
                min_final_net_worth, max_final_net_worth, lifetime_taxes,
                converged, convergence_metric, convergence_value
           FROM run_stats WHERE run_id = ?1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound("run results"))?;

    let percentile_values: Vec<(f64, f64)> = sqlx::query_as(
        "SELECT percentile, final_net_worth FROM run_percentile_values
          WHERE run_id = ?1 ORDER BY percentile",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let stats = Stats {
        num_iterations: stats_row.num_iterations,
        success_rate: stats_row.success_rate,
        funding_success_rate: stats_row.funding_success_rate,
        mean_final_net_worth: stats_row.mean_final_net_worth,
        std_dev_final_net_worth: stats_row.std_dev_final_net_worth,
        min_final_net_worth: stats_row.min_final_net_worth,
        max_final_net_worth: stats_row.max_final_net_worth,
        lifetime_taxes: stats_row.lifetime_taxes,
        converged: stats_row.converged.map(|c| c != 0),
        convergence_metric: stats_row.convergence_metric,
        convergence_value: stats_row.convergence_value,
        percentile_values: percentile_values
            .into_iter()
            .map(|(percentile, final_net_worth)| PercentileValue {
                percentile,
                final_net_worth,
            })
            .collect(),
    };

    // Bands, one per stored path.
    let path_percentiles: Vec<Option<f64>> = sqlx::query_scalar(
        "SELECT DISTINCT percentile FROM run_net_worth_points WHERE run_id = ?1
          ORDER BY percentile",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let mut bands = Vec::with_capacity(path_percentiles.len());
    for percentile in &path_percentiles {
        let points: Vec<(String, f64)> = sqlx::query_as(
            "SELECT as_of_date, net_worth FROM run_net_worth_points
              WHERE run_id = ?1 AND percentile IS ?2 ORDER BY step",
        )
        .bind(id)
        .bind(percentile)
        .fetch_all(&state.db)
        .await?;

        // Deflating a representative path does NOT turn it into a pointwise
        // real quantile. Those are stored separately in run_real_quantiles.
        let factors = inflation_by_year(&state, id, *percentile).await?;
        let inflation = points
            .iter()
            .map(|(date, _)| factor_for(&factors, year_of(date)))
            .collect();

        let (dates, net_worth) = points.into_iter().unzip();
        bands.push(Band {
            path_id: path_id(*percentile),
            percentile: *percentile,
            dates,
            net_worth,
            inflation,
        });
    }

    // Which path the per-account series and cash flows describe.
    let series_percentile = resolve_series(query.series.as_deref(), &path_percentiles)?;

    let account_rows: Vec<(i64, String, f64)> = sqlx::query_as(
        "SELECT p.account_id, a.name, p.value
           FROM run_account_points p JOIN accounts a ON a.id = p.account_id
          WHERE p.run_id = ?1 AND p.percentile IS ?2
          ORDER BY a.sort_order, p.account_id, p.step",
    )
    .bind(id)
    .bind(series_percentile)
    .fetch_all(&state.db)
    .await?;

    let mut account_series: Vec<AccountSeries> = Vec::new();
    for (account_id, label, value) in account_rows {
        match account_series.last_mut() {
            Some(series) if series.account_id == account_id => series.values.push(value),
            _ => account_series.push(AccountSeries {
                account_id,
                label,
                values: vec![value],
            }),
        }
    }

    let flow_rows: Vec<(i64, f64, f64, f64, f64, f64, f64)> = sqlx::query_as(
        "SELECT year, income, expenses, contributions, withdrawals, appreciation, net_cash_flow
           FROM run_cash_flows WHERE run_id = ?1 AND percentile IS ?2 ORDER BY year",
    )
    .bind(id)
    .bind(series_percentile)
    .fetch_all(&state.db)
    .await?;

    let tax_rows: Vec<(i64, f64, f64)> = sqlx::query_as(
        "SELECT year, total_tax, early_withdrawal_penalties
           FROM run_taxes WHERE run_id = ?1 AND percentile IS ?2 ORDER BY year",
    )
    .bind(id)
    .bind(series_percentile)
    .fetch_all(&state.db)
    .await?;

    let cash_flows = flow_rows
        .into_iter()
        .map(
            |(year, income, expenses, contributions, withdrawals, appreciation, net_cash_flow)| {
                let taxes = tax_rows
                    .iter()
                    .find(|(y, _, _)| *y == year)
                    .map(|(_, total, penalties)| total + penalties)
                    .unwrap_or(0.0);
                CashFlow {
                    year,
                    income,
                    expenses,
                    contributions,
                    withdrawals,
                    appreciation,
                    net_cash_flow,
                    taxes,
                }
            },
        )
        .collect();

    let warning_rows: Vec<(String, Option<String>, Option<i64>, String)> = sqlx::query_as(
        "SELECT kind, as_of_date, event_id, message FROM run_warnings
          WHERE run_id = ?1 AND percentile IS ?2 ORDER BY position",
    )
    .bind(id)
    .bind(series_percentile)
    .fetch_all(&state.db)
    .await?;

    let inflation = inflation_by_year(&state, id, series_percentile)
        .await?
        .into_iter()
        .map(|(year, factor)| InflationPoint { year, factor })
        .collect();

    let real_terminal: Option<RealTerminalStats> = sqlx::query_as(
        "SELECT base_date, num_iterations, mean, std_dev, min, max FROM run_real_stats WHERE run_id = ?1",
    ).bind(id).fetch_optional(&state.db).await?;
    let real_net_worth = if let Some(terminal) = real_terminal {
        let points = sqlx::query_as(
            "SELECT as_of_date AS date, p5, p50, p95 FROM run_real_quantiles WHERE run_id = ?1 ORDER BY as_of_date",
        ).bind(id).fetch_all(&state.db).await?;
        Some(RealNetWorthSummary { terminal, points })
    } else {
        None
    };

    Ok(Json(Results {
        real_net_worth,
        series_id: path_id(series_percentile),
        run_id: run.id,
        scenario_id: run.scenario_id,
        stats,
        bands,
        account_series,
        series_percentile,
        cash_flows,
        warnings: warning_rows
            .into_iter()
            .map(|(kind, date, event_id, message)| Warning {
                kind,
                date,
                event_id,
                message,
            })
            .collect(),
        inflation,
        ledger_years: ledger_years(&state, id, series_percentile).await?,
    }))
}

// ── path selection ──────────────────────────────────────────────────────────

fn path_id(percentile: Option<f64>) -> String {
    percentile.map_or_else(|| "mean".into(), |p| p.to_string())
}

/// Which stored path a `series` query names: the mean, an explicit percentile,
/// or — by default — whichever stored percentile sits closest to the median.
///
/// An explicit percentile resolves to the nearest stored path rather than to
/// itself. A run keeps the percentiles it was started with, and a caller asking
/// for `0.05` is naming the run that stands for the bad case, not asserting
/// that a path was stored at exactly that mark — so a run stored at 0.1 answers
/// with the path it has instead of with an empty series.
fn resolve_series(series: Option<&str>, stored: &[Option<f64>]) -> ApiResult<Option<f64>> {
    Ok(match series {
        Some("mean") if stored.contains(&None) => None,
        Some("mean") => return Err(ApiError::NotFound("stored mean series")),
        Some(other) => {
            let target = other.parse::<f64>().map_err(|_| {
                ApiError::bad_request("series must be 'mean' or a percentile such as 0.5")
            })?;
            if !(0.0..=1.0).contains(&target) {
                return Err(ApiError::bad_request("series percentile must be in 0..1"));
            }
            Some(nearest_stored(stored, target).ok_or(ApiError::NotFound("representative path"))?)
        }
        None => Some(nearest_stored(stored, 0.5).ok_or(ApiError::NotFound("representative path"))?),
    })
}

/// The stored percentile closest to `target`, if the run stored any at all.
fn nearest_stored(stored: &[Option<f64>], target: f64) -> Option<f64> {
    stored.iter().flatten().copied().min_by(|a, b| {
        (a - target)
            .abs()
            .partial_cmp(&(b - target).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// `resolve_series` for a caller that has not already read the stored paths.
async fn series_path(
    state: &AppState,
    run_id: i64,
    series: Option<&str>,
) -> ApiResult<Option<f64>> {
    let stored: Vec<Option<f64>> = sqlx::query_scalar(
        "SELECT DISTINCT percentile FROM run_net_worth_points WHERE run_id = ?1
          ORDER BY percentile",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;
    resolve_series(series, &stored)
}

// ── inflation ───────────────────────────────────────────────────────────────

fn year_of(date: &str) -> i64 {
    date.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0)
}

/// The path's cumulative inflation, ascending by year.
async fn inflation_by_year(
    state: &AppState,
    run_id: i64,
    percentile: Option<f64>,
) -> ApiResult<Vec<(i64, f64)>> {
    Ok(sqlx::query_as(
        "SELECT year, factor FROM run_inflation
          WHERE run_id = ?1 AND percentile IS ?2 ORDER BY year",
    )
    .bind(run_id)
    .bind(percentile)
    .fetch_all(&state.db)
    .await?)
}

/// The factor for one year: the plan's first year before the table starts, the
/// last recorded factor beyond its end, and 1.0 for a run that stored none.
fn factor_for(factors: &[(i64, f64)], year: i64) -> f64 {
    if factors.is_empty() {
        return 1.0;
    }
    match factors.binary_search_by_key(&year, |(y, _)| *y) {
        Ok(i) => factors[i].1,
        Err(0) => 1.0,
        Err(i) => factors[i - 1].1,
    }
}

// ── ledger ──────────────────────────────────────────────────────────────────

async fn ledger_years(
    state: &AppState,
    run_id: i64,
    percentile: Option<f64>,
) -> ApiResult<Vec<LedgerYear>> {
    let rows: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT year, category, COUNT(*) FROM run_ledger
          WHERE run_id = ?1 AND percentile IS ?2
          GROUP BY year, category ORDER BY year",
    )
    .bind(run_id)
    .bind(percentile)
    .fetch_all(&state.db)
    .await?;

    let mut years: Vec<LedgerYear> = Vec::new();
    for (year, category, count) in rows {
        if years.last().map(|y| y.year) != Some(year) {
            years.push(LedgerYear {
                year,
                cash: 0,
                asset: 0,
                tax: 0,
                event: 0,
                total: 0,
                tag: None,
            });
        }
        let entry = years.last_mut().expect("just pushed");
        match category.as_str() {
            "cash" => entry.cash += count,
            "asset" => entry.asset += count,
            "tax" => entry.tax += count,
            "event" => entry.event += count,
            _ => {}
        }
        entry.total += count;
    }

    // What makes a year worth a second look is an event *starting* in it —
    // retiring, a pension beginning, RMDs coming due. Two things it is not:
    // ranking entry kinds would tag every year after retirement, since every
    // one of them withdraws and sells, and a monthly event fires in all of
    // them. So each event tags only the first year it triggered in.
    //
    // One row per event, at its earliest trigger: SQLite pairs a bare column
    // with an aggregated `min()`, so `year` and `detail` come from that row.
    // The name stands in for a deleted event, which has no id left to group by.
    let firsts: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT year, detail, MIN(position) FROM run_ledger
          WHERE run_id = ?1 AND percentile IS ?2 AND kind = 'Triggered'
          GROUP BY COALESCE(event_id, -1), detail
          ORDER BY MIN(position)",
    )
    .bind(run_id)
    .bind(percentile)
    .fetch_all(&state.db)
    .await?;

    // Ordered by position, so a year that starts two events reads as the first.
    for (year, detail, _) in firsts {
        if let Ok(i) = years.binary_search_by_key(&year, |y| y.year)
            && years[i].tag.is_none()
        {
            years[i].tag = Some(detail);
        }
    }
    Ok(years)
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct LedgerQuery {
    /// Which path to read, matching `ResultsQuery::series`.
    #[serde(default)]
    pub series: Option<String>,
    /// Restrict to one calendar year — how the cash-flow table reads the
    /// entries behind a row it has expanded.
    #[serde(default)]
    pub year: Option<i64>,
    /// One of `cash`, `asset`, `tax`, `event`; omit for all four.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct LedgerPage {
    pub run_id: i64,
    pub series_id: String,
    pub entries: Vec<LedgerEntry>,
    /// Entries matching the filter, of which `entries` is one page.
    pub total: i64,
}

/// The most entries one request will return. A year of a busy plan runs to a
/// few hundred; the cap is what stops `year` being omitted by accident from
/// serialising the whole run.
const LEDGER_PAGE_MAX: i64 = 500;

/// The itemised effects behind a year's cash-flow totals.
///
/// Separate from `results` rather than folded into it: the ledger is an order
/// of magnitude larger than everything else a run stores, and the screen only
/// ever reads one year of it at a time.
async fn ledger(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<LedgerQuery>,
) -> ApiResult<Json<LedgerPage>> {
    let run = owned_run(&state, id, &user.id).await?;
    if run.status != "succeeded" {
        return Err(ApiError::Conflict(format!(
            "run is '{}'; the ledger is only available once it has succeeded",
            run.status
        )));
    }

    let percentile = series_path(&state, id, query.series.as_deref()).await?;

    if let Some(category) = &query.category
        && !["cash", "asset", "tax", "event"].contains(&category.as_str())
    {
        return Err(ApiError::bad_request(
            "category must be one of cash, asset, tax, event",
        ));
    }

    let limit = query
        .limit
        .unwrap_or(LEDGER_PAGE_MAX)
        .clamp(1, LEDGER_PAGE_MAX);
    let offset = query.offset.unwrap_or(0).max(0);

    // `?3 IS NULL OR column = ?3` keeps one prepared statement for every
    // combination of filters, rather than concatenating SQL per request.
    const FILTER: &str = "run_id = ?1 AND percentile IS ?2
          AND (?3 IS NULL OR year = ?3) AND (?4 IS NULL OR category = ?4)";

    let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM run_ledger WHERE {FILTER}"))
        .bind(id)
        .bind(percentile)
        .bind(query.year)
        .bind(&query.category)
        .fetch_one(&state.db)
        .await?;

    let entries: Vec<LedgerEntry> = sqlx::query_as(&format!(
        "SELECT position, as_of_date AS date, year, category, kind, detail, amount, basis,
                basis_label, account_id, event_id
           FROM run_ledger WHERE {FILTER}
          ORDER BY position LIMIT ?5 OFFSET ?6"
    ))
    .bind(id)
    .bind(percentile)
    .bind(query.year)
    .bind(&query.category)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(LedgerPage {
        run_id: id,
        series_id: path_id(percentile),
        total,
        entries,
    }))
}
