//! Preview: a plan edit simulated against the run it was written for.
//!
//! `POST /scenarios/{id}/preview` takes a batch of [`Change`]s, applies them to
//! the base run's input snapshot in memory — nothing is written — and runs the
//! edited plan with the base run's own Monte Carlo settings and seed. Each
//! iteration draws its market from a seed that does not depend on the plan, so
//! as long as the edit leaves the market inputs alone (the same return
//! profiles, inflation, horizon and tracking error) the base and the edited
//! plan see the same markets, iteration by iteration, and the difference
//! between them is the edit rather than sampling noise. The response says
//! whether that held (`paired`).
//!
//! The base run's own results are reused when they can be: the same iteration
//! count, a fixed seed, and a complete set of stored summary figures.
//! Otherwise the base is simulated alongside the edit, with the same settings.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use finplan_core::config::SimulationConfig;
use finplan_core::model::{MonteCarloConfig, MonteCarloProgress, MonteCarloSummary};
use finplan_core::simulation::monte_carlo_simulate_with_progress;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::api::funding::{FundingDiagnostics, funding_view};
use crate::auth::session::CurrentUser;
use crate::compile::rows::{
    DistributionRow, InflationEntry, ReturnProfileRow, ScenarioGraph, TaxBracketRow,
    TaxConfigEntry, TaxConfigRow,
};
use crate::compile::{self, CompiledScenario};
use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::observability::{JobKind as MetricKind, Origin};
use crate::runner::inputs::MODEL_VERSION;
use crate::runner::telemetry::Submission;
use crate::state::AppState;
use crate::suggest::{
    self, Change, ChangeProblem, ChangeTarget, Created, DiffLine, Names, Resolved,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/scenarios/{scenario_id}/preview", post(preview))
}

/// Most simulations one side of a preview may spend. The request is held open
/// while it runs, and a preview may simulate the base as well as the edit.
pub const MAX_PREVIEW_ITERATIONS: usize = 5_000;

/// The seed for a base run that was not seeded, so its preview can still pair
/// the base and the edit. Mixed with the run id so previews of different runs
/// do not all sample the same markets.
const UNSEEDED_BASE: u64 = 0x9E37_79B9_7F4A_7C15;

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct PreviewRequest {
    /// The run the changes were written against. Defaults to the scenario's
    /// latest succeeded run.
    #[serde(default)]
    pub base_run_id: Option<i64>,
    pub changes: Vec<Change>,
    /// Simulations for each side. Defaults to the base run's count; capped at
    /// `MAX_PREVIEW_ITERATIONS`.
    #[serde(default)]
    pub iterations: Option<usize>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct Preview {
    /// The run the preview was paired against; null for a draft, which has
    /// none. Such a preview only checks the batch and renders its diff: it
    /// simulates nothing, so `base` and `edited` are null.
    pub base_run_id: Option<i64>,
    /// Iterations behind both `base` and `edited`.
    pub iterations: usize,
    /// The base and the edited plan saw the same simulated markets, so their
    /// difference is the edit's. False when the edit changes the market
    /// inputs (a newly used or no longer used return profile, the inflation
    /// profile, the horizon, tracking error).
    pub paired: bool,
    /// What the batch changes, rendered by the server.
    pub diff: Vec<DiffLine>,
    /// Why the batch cannot be applied. Non-empty means nothing was simulated.
    pub problems: Vec<ChangeProblem>,
    pub base: Option<PreviewStats>,
    pub edited: Option<PreviewStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PreviewStats {
    pub success_rate: f64,
    pub funding_success_rate: Option<f64>,
    /// Final net worth in today's dollars, over all iterations.
    pub real_final: Option<RealFinal>,
    pub funding: Option<FundingDiagnostics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct RealFinal {
    pub p5: f64,
    pub p10: f64,
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
    pub p90: f64,
    pub p95: f64,
}

async fn preview(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(scenario_id): Path<i64>,
    Json(body): Json<PreviewRequest>,
) -> ApiResult<Json<Preview>> {
    let mut decision = Submission::new(&state.telemetry, MetricKind::Preview);
    let result = run_preview(
        &state,
        &user,
        scenario_id,
        body.base_run_id,
        &body.changes,
        body.iterations,
        Some(&mut decision),
    )
    .await;
    decision.result(&result);
    result.map(Json)
}

/// The columns of a run a preview is based on.
#[derive(sqlx::FromRow)]
struct BaseRun {
    id: i64,
    status: String,
    iterations: i64,
    seed: Option<i64>,
    batch_size: i64,
    parallel_batches: i64,
    converge: i64,
    snapshot_json: Option<String>,
    model_version: Option<String>,
}

/// Preview `changes` against a run of `scenario_id` — the route's body, for
/// callers that preview in-process (suggestions). `decision` records the
/// request's admission when the caller is the preview route itself.
pub(crate) async fn run_preview(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    base_run_id: Option<i64>,
    changes: &[Change],
    iterations: Option<usize>,
    decision: Option<&mut Submission>,
) -> ApiResult<Preview> {
    super::owned_scenario(&state.db, scenario_id, &user.id).await?;
    if iterations == Some(0) {
        return Err(ApiError::bad_request("iterations must be at least 1"));
    }

    if base_run_id.is_none() && super::is_draft(&state.db, scenario_id).await? {
        return draft_preview(state, user, scenario_id, changes).await;
    }

    let run = base_run(&state.db, scenario_id, base_run_id).await?;
    if run.model_version.as_deref() != Some(MODEL_VERSION) {
        return Err(ApiError::Conflict(format!(
            "run {} was made by an earlier version of the model; run the plan again to preview against it",
            run.id
        )));
    }
    let mut graph: ScenarioGraph = run
        .snapshot_json
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .ok_or_else(|| ApiError::Conflict(format!("run {} has no readable inputs", run.id)))?;
    let base_compiled = compile::compile(&graph).map_err(|e| {
        ApiError::Conflict(format!("run {}'s inputs no longer compile: {e}", run.id))
    })?;

    let iterations = iterations
        .unwrap_or(run.iterations.max(1) as usize)
        .min(MAX_PREVIEW_ITERATIONS);
    let mut outcome = Preview {
        base_run_id: Some(run.id),
        iterations,
        paired: false,
        diff: Vec::new(),
        problems: Vec::new(),
        base: None,
        edited: None,
    };

    let resolved = match suggest::resolve(&graph, changes) {
        Ok(resolved) => resolved,
        Err(problems) => {
            outcome.problems = problems;
            return Ok(outcome);
        }
    };
    // A snapshot keeps only the profiles the run used; bring in any the
    // changes newly point at before the edits check they exist.
    load_profiles(
        &state.db,
        &user.id,
        &mut graph,
        resolved.referenced_profiles(),
    )
    .await?;
    let (tax, inflation) = suggest::assumptions_named(changes);
    load_assumptions(&state.db, &user.id, &mut graph, tax, inflation).await?;
    outcome.diff = resolved.diff(&Names::from_graph(&graph));

    let edited_compiled = match edited(&graph, &resolved, changes)? {
        Ok((_, compiled)) => compiled,
        Err(problem) => {
            outcome.problems.push(problem);
            return Ok(outcome);
        }
    };
    outcome.paired = market_inputs(&base_compiled.config) == market_inputs(&edited_compiled.config);

    let seed = run
        .seed
        .map_or(UNSEEDED_BASE ^ run.id as u64, |seed| seed as u64);
    let mc_config = MonteCarloConfig {
        iterations,
        // Stats, real quantiles and funding diagnostics come from every
        // iteration; representative paths and the mean path are not shown.
        percentiles: Vec::new(),
        compute_mean: false,
        convergence: None,
        batch_size: run.batch_size as usize,
        parallel_batches: run.parallel_batches as usize,
        seed: Some(seed),
    };
    let stored = if run.converge == 0 && run.seed.is_some() && iterations == run.iterations as usize
    {
        stored_stats(&state.db, run.id).await?
    } else {
        None
    };

    let permit =
        crate::billing::admit_compute_observed(&user.id, &state.telemetry, Origin::Request)?;
    if let Some(decision) = decision {
        decision.accepted();
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let guard = CancelOnDrop(cancel.clone());
    let base_config = stored.is_none().then(|| base_compiled.config.clone());
    let edited_config = edited_compiled.config.clone();
    let simulated = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let progress = || {
            MonteCarloProgress::from_atomics_accumulating(
                Arc::new(AtomicUsize::new(0)),
                cancel.clone(),
            )
        };
        let base = base_config
            .map(|config| monte_carlo_simulate_with_progress(&config, &mc_config, &progress()))
            .transpose()
            .map_err(|e| ApiError::Internal(format!("base simulation failed: {e}")))?;
        let edited = monte_carlo_simulate_with_progress(&edited_config, &mc_config, &progress())
            .map_err(|e| {
                ApiError::unprocessable(format!("the edited plan failed to simulate: {e}"))
            })?;
        Ok::<_, ApiError>((base, edited))
    })
    .await;
    drop(guard);
    let (base, edited) =
        simulated.map_err(|_| ApiError::Internal("preview simulation panicked".into()))??;

    outcome.base = match (stored, base) {
        (Some(stored), _) => Some(stored),
        (None, Some(summary)) => Some(stats(&base_compiled, &summary)),
        (None, None) => None,
    };
    outcome.edited = Some(stats(&edited_compiled, &edited));
    Ok(outcome)
}

/// A preview of a draft: there is no run to pair against (whole-plan draft
/// simulation is a later step), so this resolves the batch, renders its diff
/// and checks that the edited plan compiles, and reports problems the same way.
async fn draft_preview(
    state: &AppState,
    user: &CurrentUser,
    scenario_id: i64,
    changes: &[Change],
) -> ApiResult<Preview> {
    let mut graph = ScenarioGraph::load(&state.db, scenario_id, &user.id).await?;
    let mut outcome = Preview {
        base_run_id: None,
        iterations: 0,
        paired: false,
        diff: Vec::new(),
        problems: Vec::new(),
        base: None,
        edited: None,
    };
    let resolved = match suggest::resolve(&graph, changes) {
        Ok(resolved) => resolved,
        Err(problems) => {
            outcome.problems = problems;
            return Ok(outcome);
        }
    };
    load_profiles(
        &state.db,
        &user.id,
        &mut graph,
        resolved.referenced_profiles(),
    )
    .await?;
    let (tax, inflation) = suggest::assumptions_named(changes);
    load_assumptions(&state.db, &user.id, &mut graph, tax, inflation).await?;
    outcome.diff = resolved.diff(&Names::from_graph(&graph));
    if let Err(problem) = edited(&graph, &resolved, changes)? {
        outcome.problems.push(problem);
    }
    Ok(outcome)
}

/// The succeeded run `run_id` names (or the scenario's latest success) and
/// its input snapshot: what suggestions are written and checked against. A
/// draft, asked for no particular run, has none: its notes are written against
/// the plan as it stands (run id `None`).
/// The caller has already checked the scenario is the user's.
pub(crate) async fn base_snapshot(
    db: &Db,
    scenario_id: i64,
    user_id: &str,
    run_id: Option<i64>,
) -> ApiResult<(Option<i64>, ScenarioGraph)> {
    if run_id.is_none() && super::is_draft(db, scenario_id).await? {
        return Ok((None, ScenarioGraph::load(db, scenario_id, user_id).await?));
    }
    let run = base_run(db, scenario_id, run_id).await?;
    let graph = run
        .snapshot_json
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .ok_or_else(|| ApiError::Conflict(format!("run {} has no readable inputs", run.id)))?;
    Ok((Some(run.id), graph))
}

/// The run to preview against: the one named, or the scenario's latest success.
async fn base_run(db: &Db, scenario_id: i64, run_id: Option<i64>) -> ApiResult<BaseRun> {
    const COLUMNS: &str = "id, status, iterations, seed, batch_size, parallel_batches, converge, \
                           snapshot_json, model_version";
    let run: Option<BaseRun> = match run_id {
        Some(id) => {
            sqlx::query_as(&format!(
                "SELECT {COLUMNS} FROM runs WHERE id = ?1 AND scenario_id = ?2"
            ))
            .bind(id)
            .bind(scenario_id)
            .fetch_optional(db)
            .await?
        }
        None => {
            sqlx::query_as(&format!(
                "SELECT {COLUMNS} FROM runs WHERE scenario_id = ?1 AND status = 'succeeded'
                  ORDER BY id DESC LIMIT 1"
            ))
            .bind(scenario_id)
            .fetch_optional(db)
            .await?
        }
    };
    match run {
        Some(run) if run.status == "succeeded" => Ok(run),
        Some(run) => Err(ApiError::Conflict(format!(
            "run {} has not succeeded ({}); preview against a finished run",
            run.id, run.status
        ))),
        None if run_id.is_some() => Err(ApiError::NotFound("run")),
        None => Err(ApiError::Conflict(
            "this plan has no finished run to preview against; run it first".into(),
        )),
    }
}

/// Add the caller's return profiles in `wanted` that `graph` lacks, with their
/// distributions (and a regime-switching profile's bull and bear children).
/// A profile that is not the caller's is left out; the edit that names it
/// then fails the way the route would.
pub(crate) async fn load_profiles(
    db: &Db,
    user_id: &str,
    graph: &mut ScenarioGraph,
    wanted: BTreeSet<i64>,
) -> ApiResult<()> {
    let mut pending = Vec::new();
    for id in wanted {
        if graph.return_profiles.contains_key(&id) {
            continue;
        }
        let row: Option<ReturnProfileRow> = sqlx::query_as(
            "SELECT id, name, description, distribution_id, asset_class
               FROM return_profiles WHERE id = ?1 AND user_id = ?2",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(db)
        .await?;
        if let Some(row) = row {
            pending.push(row.distribution_id);
            graph.return_profiles.insert(row.id, row);
        }
    }
    load_distributions(db, user_id, graph, pending).await
}

/// Add the distributions `pending` names, and their regime children, that
/// `graph` lacks.
async fn load_distributions(
    db: &Db,
    user_id: &str,
    graph: &mut ScenarioGraph,
    mut pending: Vec<i64>,
) -> ApiResult<()> {
    while let Some(id) = pending.pop() {
        if graph.distributions.contains_key(&id) {
            continue;
        }
        let row: Option<DistributionRow> = sqlx::query_as(
            "SELECT id, kind, rate, mean, std_dev, scale, df, bull_id, bear_id,
                    bull_to_bear_prob, bear_to_bull_prob, history_preset, block_size
               FROM distributions WHERE id = ?1 AND user_id = ?2",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(db)
        .await?;
        if let Some(row) = row {
            pending.extend(row.bull_id);
            pending.extend(row.bear_id);
            graph.distributions.insert(row.id, row);
        }
    }
    Ok(())
}

/// Add the caller's tax configs and inflation profiles in `tax` and
/// `inflation` that `graph` lacks from its library (a run snapshot keeps only
/// the scenario's own), so a change that switches the plan onto one finds it.
/// One that is not the caller's is left out; the edit that names it then fails
/// the way the route would.
pub(crate) async fn load_assumptions(
    db: &Db,
    user_id: &str,
    graph: &mut ScenarioGraph,
    tax: impl IntoIterator<Item = i64>,
    inflation: impl IntoIterator<Item = i64>,
) -> ApiResult<()> {
    for id in tax {
        if graph.tax_configs.contains_key(&id) {
            continue;
        }
        let config: Option<TaxConfigRow> = sqlx::query_as(
            "SELECT id, name, state_rate, capital_gains_rate, early_withdrawal_penalty_rate
               FROM tax_configs WHERE id = ?1 AND user_id = ?2",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(db)
        .await?;
        if let Some(config) = config {
            let brackets: Vec<TaxBracketRow> = sqlx::query_as(
                "SELECT threshold, rate FROM tax_brackets
                  WHERE tax_config_id = ?1 ORDER BY threshold ASC",
            )
            .bind(id)
            .fetch_all(db)
            .await?;
            graph
                .tax_configs
                .insert(id, TaxConfigEntry { config, brackets });
        }
    }
    let mut distributions = Vec::new();
    for id in inflation {
        if graph.inflation_profiles.contains_key(&id) {
            continue;
        }
        let row: Option<(String, i64)> = sqlx::query_as(
            "SELECT name, distribution_id FROM inflation_profiles WHERE id = ?1 AND user_id = ?2",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(db)
        .await?;
        if let Some((name, distribution_id)) = row {
            distributions.push(distribution_id);
            graph.inflation_profiles.insert(
                id,
                InflationEntry {
                    name,
                    distribution_id,
                },
            );
        }
    }
    load_distributions(db, user_id, graph, distributions).await
}

/// Apply `resolved` to a copy of `graph` and compile it, returning both. The
/// outer error is the server's; the inner one is the batch's, reported as a
/// problem.
pub(crate) fn edited(
    graph: &ScenarioGraph,
    resolved: &Resolved,
    changes: &[Change],
) -> ApiResult<Result<(ScenarioGraph, CompiledScenario), ChangeProblem>> {
    let mut edited = graph.clone();
    if let Err(problem) = suggest::apply_to_graph(&mut edited, resolved, &mut Created::new())? {
        return Ok(Err(problem));
    }
    match compile::compile(&edited) {
        Ok(compiled) => Ok(Ok((edited, compiled))),
        Err(err) => {
            let change = changes.len().saturating_sub(1);
            let target = changes
                .last()
                .map_or(ChangeTarget::NewEvent(String::new()), |c| c.target.clone());
            suggest::plan_problem(err, change, target).map(Err)
        }
    }
}

/// Everything that decides an iteration's simulated markets, given its seed:
/// the return and inflation processes sampled (in compiled-id order), the
/// number of years sampled, and the per-asset tracking-error noise. Two
/// configs that agree on these draw the same markets from the same seed.
fn market_inputs(config: &SimulationConfig) -> Vec<String> {
    fn sorted<K: Debug, V: Debug>(map: &HashMap<K, V>) -> String {
        let mut entries: Vec<String> = map.iter().map(|kv| format!("{kv:?}")).collect();
        entries.sort();
        entries.join(", ")
    }
    vec![
        sorted(&config.return_profiles),
        format!("{:?}", config.inflation_profile),
        format!("{:?} {:?}", config.start_date, config.duration_years),
        sorted(&config.asset_tracking_errors),
    ]
}

/// The figures a stored run already has, when all of them survive. Superseded
/// runs lose their real quantiles; runs from before funding diagnostics lack
/// those. Either way the base is then simulated alongside the edit.
async fn stored_stats(db: &Db, run_id: i64) -> ApiResult<Option<PreviewStats>> {
    let row: Option<(f64, Option<f64>, Option<String>)> = sqlx::query_as(
        "SELECT success_rate, funding_success_rate, funding_diagnostics
           FROM run_stats WHERE run_id = ?1",
    )
    .bind(run_id)
    .fetch_optional(db)
    .await?;
    let Some((success_rate, funding_success_rate, Some(funding))) = row else {
        return Ok(None);
    };
    let Ok(funding) = serde_json::from_str::<FundingDiagnostics>(&funding) else {
        return Ok(None);
    };
    let real_final: Option<RealFinal> = sqlx::query_as(
        "SELECT p5, p10, p25, p50, p75, p90, p95 FROM run_real_quantiles
          WHERE run_id = ?1 ORDER BY as_of_date DESC LIMIT 1",
    )
    .bind(run_id)
    .fetch_optional(db)
    .await?;
    if real_final.is_none() || funding_success_rate.is_none() {
        return Ok(None);
    }
    Ok(Some(PreviewStats {
        success_rate,
        funding_success_rate,
        real_final,
        funding: Some(funding),
    }))
}

fn stats(compiled: &CompiledScenario, summary: &MonteCarloSummary) -> PreviewStats {
    PreviewStats {
        success_rate: summary.stats.success_rate,
        funding_success_rate: summary.stats.funding_success_rate,
        real_final: summary
            .real_net_worth
            .as_ref()
            .and_then(|real| real.points.last())
            .map(|p| RealFinal {
                p5: p.p5,
                p10: p.p10,
                p25: p.p25,
                p50: p.p50,
                p75: p.p75,
                p90: p.p90,
                p95: p.p95,
            }),
        funding: summary
            .funding
            .as_ref()
            .map(|funding| funding_view(&compiled.id_map, funding)),
    }
}

/// Stops the simulation when the request goes away: a client that has moved
/// on aborts its fetch, axum drops the handler's future, and this flags the
/// engine rather than finishing an answer nobody will read.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}
