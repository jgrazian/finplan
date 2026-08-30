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

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/scenarios/{scenario_id}/runs",
            get(list_for_scenario).post(create),
        )
        .route("/runs/{id}", get(fetch).delete(destroy))
        .route("/runs/{id}/cancel", post(cancel))
        .route("/runs/{id}/results", get(results))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Run {
    pub id: i64,
    pub scenario_id: i64,
    pub status: String,
    pub iterations: i64,
    pub completed_iterations: i64,
    pub seed: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

const RUN_COLUMNS: &str = "id, scenario_id, status, iterations, completed_iterations, seed,
     error_message, created_at, started_at, finished_at";

#[derive(Debug, Deserialize)]
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
}

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
            (scenario_id, user_id, iterations, seed, batch_size, parallel_batches, compute_mean)
         VALUES (?1,?2,?3,?4,?5,?6,?7) RETURNING id",
    )
    .bind(scenario_id)
    .bind(&user.id)
    .bind(body.iterations)
    .bind(body.seed)
    .bind(body.batch_size.max(1))
    .bind(body.parallel_batches.max(1))
    .bind(i64::from(body.compute_mean))
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
    mean_final_net_worth: f64,
    std_dev_final_net_worth: f64,
    min_final_net_worth: f64,
    max_final_net_worth: f64,
    lifetime_taxes: f64,
    converged: Option<i64>,
    convergence_metric: Option<String>,
    convergence_value: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Stats {
    pub num_iterations: i64,
    pub success_rate: f64,
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

#[derive(Debug, Serialize)]
pub struct PercentileValue {
    pub percentile: f64,
    pub final_net_worth: f64,
}

/// Net-worth path for one percentile (or the mean, when `percentile` is null).
#[derive(Debug, Serialize)]
pub struct Band {
    pub percentile: Option<f64>,
    pub dates: Vec<String>,
    pub net_worth: Vec<f64>,
}

#[derive(Debug, Serialize)]
pub struct AccountSeries {
    pub account_id: i64,
    pub label: String,
    pub values: Vec<f64>,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct Warning {
    pub kind: String,
    pub date: Option<String>,
    pub event_id: Option<i64>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct Results {
    pub run_id: i64,
    pub scenario_id: i64,
    pub stats: Stats,
    pub bands: Vec<Band>,
    /// Per-account decomposition of the path named by `series_percentile`.
    pub account_series: Vec<AccountSeries>,
    pub series_percentile: Option<f64>,
    pub cash_flows: Vec<CashFlow>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Deserialize)]
pub struct ResultsQuery {
    /// Which path the per-account series and cash flows describe. Defaults to
    /// the median; pass `mean` for the averaged path.
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
        "SELECT num_iterations, success_rate, mean_final_net_worth, std_dev_final_net_worth,
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

        let (dates, net_worth) = points.into_iter().unzip();
        bands.push(Band {
            percentile: *percentile,
            dates,
            net_worth,
        });
    }

    // Which path the per-account series and cash flows describe.
    let series_percentile: Option<f64> = match query.series.as_deref() {
        Some("mean") => None,
        Some(other) => Some(other.parse::<f64>().map_err(|_| {
            ApiError::bad_request("series must be 'mean' or a percentile such as 0.5")
        })?),
        None => path_percentiles.iter().flatten().copied().min_by(|a, b| {
            (a - 0.5)
                .abs()
                .partial_cmp(&(b - 0.5).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    };

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

    Ok(Json(Results {
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
    }))
}
