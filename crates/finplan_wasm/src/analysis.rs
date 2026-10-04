//! Analyses, what-ifs and the rule-based review, on a plan held on the device.
//!
//! The loops that decide which configurations to simulate (the sweep grid, the
//! goal seek's bisection, the stack of what-if layers) are
//! `finplan_plan::analysis`; this module only gives them somewhere to simulate:
//! [`Local`], an `McRunner` that runs each Monte Carlo call to completion
//! on the calling thread, and the shards that split one analysis across workers. A browser runs it in a compute worker, so a long
//! analysis never blocks the page; the answer is the same as the server's for
//! the same plan and request (the batch plan, [`LOCAL_PARALLEL_BATCHES`], is
//! fixed rather than the machine's core count).
//!
//! **Progress and cancelling.** The call is synchronous, so the worker cannot
//! hear a "cancel" message while it runs; cancelling an analysis there is
//! terminating the worker. A host that can poll (the in-process runner the
//! tests use) returns `true` from the progress callback to stop: the next
//! simulation is not started and the call fails with `409 conflict`.

use std::collections::{HashMap, HashSet};

use finplan_core::analysis::{McRunner, StatsRun};
use finplan_core::config::SimulationConfig;
use finplan_core::error::SimulationError;
use finplan_core::model::{MonteCarloConfig, MonteCarloStats, MonteCarloSummary};
use finplan_core::simulation::{
    MonteCarloCoordinator, base_seed, prepare_run_with, run_batch_observed,
};
use finplan_plan::analysis::{
    AnalysisError, AnalysisOutcome, CreateAnalysis, Limits, MAX_ANALYSIS_ITERATIONS, analyze,
    prepare,
};
use finplan_plan::create;
use finplan_plan::drawdown::{self, CompareRequest, DrawdownRequest};
use finplan_plan::graph::ScenarioGraph;
use finplan_plan::library::Library;
use finplan_plan::results::RunResults;
use finplan_plan::review::{self, LocalReview};
use finplan_plan::suggest::{Change, ChangeProblem, Created, resolve_steps};
use finplan_plan::what_if::{self, ApplyWhatIf, QuickWhatIf, WhatIfStack, quick_iterations};
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use ts_rs::TS;

use crate::error::{EngineError, EngineResult, parse, to_json};

/// Batches each analysis simulation is split into: part of the batch plan, so
/// part of the answer. It is `CreateRun`'s default, which is also what the
/// server's analysis uses on a four-worker deployment; fixing it here makes a
/// local analysis the same on every device.
pub const LOCAL_PARALLEL_BATCHES: usize = 4;

/// An analysis may ask for as many iterations as the plan crate allows: the
/// cap is the device's, and the plan crate's own ceiling still applies.
pub const LOCAL_ITERATION_CAP: usize = MAX_ANALYSIS_ITERATIONS;

fn limits() -> Limits {
    Limits {
        iteration_cap: LOCAL_ITERATION_CAP,
        parallel_batches: LOCAL_PARALLEL_BATCHES,
    }
}

impl From<AnalysisError> for EngineError {
    fn from(error: AnalysisError) -> Self {
        match error {
            AnalysisError::Plan(error) => error.into(),
            AnalysisError::Cancelled => EngineError::new(409, "conflict", "analysis canceled"),
            AnalysisError::Failed(message) => EngineError::new(500, "simulation_failed", message),
        }
    }
}

/// Reports `(completed, total)` simulations, and answers whether to stop.
pub type Progress<'a> = &'a mut dyn FnMut(usize, usize) -> bool;

/// How many progress reports one Monte Carlo call makes at most.
const REPORTS_PER_CALL: usize = 50;

/// Runs every Monte Carlo call on the calling thread, batch by batch, as the
/// engine's own `monte_carlo_core` does (the same preparation, coordinator,
/// batches and merge order, so the same answer), reporting as simulations
/// finish rather than as calls do: a sweep point of 250 iterations is not
/// one tick of the counter.
struct Local<'a> {
    progress: Progress<'a>,
    done: usize,
    total: usize,
    stop: bool,
}

impl<'a> Local<'a> {
    fn new(progress: Progress<'a>) -> Self {
        Local {
            progress,
            done: 0,
            total: 0,
            stop: false,
        }
    }

    /// Report `extra` simulations beyond those of finished calls.
    fn report(&mut self, extra: usize) -> bool {
        let done = self.done + extra;
        // A phase may run past the count it announced (a sweep's baseline run
        // follows its grid), so the total grows to meet it.
        self.total = self.total.max(done);
        self.stop = (self.progress)(done, self.total) || self.stop;
        self.stop
    }

    /// One Monte Carlo call to the end of its rounds, ready to finish.
    fn run(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
        collect_real: bool,
    ) -> Result<MonteCarloCoordinator, SimulationError> {
        if self.stop {
            return Err(SimulationError::Cancelled);
        }
        let prepared = prepare_run_with(config, mc, collect_real)?;
        let mut coordinator = MonteCarloCoordinator::new(mc, base_seed(mc)?)?;
        let step = (mc.iterations / REPORTS_PER_CALL).max(1);
        let mut within = 0;
        while let Some(round) = coordinator.next_round() {
            let mut outputs = Vec::with_capacity(round.len());
            for spec in &round {
                let before = within;
                outputs.push(run_batch_observed(&prepared, spec, None, |done| {
                    (done % step == 0 || done == spec.iterations) && self.report(before + done)
                })?);
                within += spec.iterations;
            }
            coordinator.absorb(outputs)?;
        }
        self.done += within;
        Ok(coordinator)
    }
}

impl McRunner for Local<'_> {
    /// `monte_carlo_stats_only`, observed.
    fn stats(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<StatsRun, SimulationError> {
        let mut mc = mc.clone();
        mc.compute_mean = false;
        self.run(config, &mc, false)?.finish_stats()
    }

    /// `monte_carlo_simulate_with_progress`, observed.
    fn summary(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<MonteCarloSummary, SimulationError> {
        self.run(config, mc, true)?.finish(config, true)
    }

    fn cancelled(&self) -> bool {
        self.stop
    }

    fn begin(&mut self, total_iterations: usize) {
        self.done = 0;
        self.total = total_iterations;
        self.stop = (self.progress)(0, total_iterations) || self.stop;
    }
}

// ── sharding: one analysis on several workers ──────────────────────────────
//
// A sweep's points and a sensitivity's rows are independent Monte Carlo
// calls: which calls an analysis makes does not depend on what any answers.
// So each of `shards` workers runs the analysis's own loop and simulates
// every `shards`-th call (`analysis_shard_json`), answering the rest with a
// placeholder it throws away; then one pass (`analysis_finish_json`) runs the
// loop again, answered from what the shards found. A call it finds no answer
// for, or one whose plan or settings differ from the answer's, it simulates
// itself, so the outcome is always the one-worker outcome. A solve chooses
// each probe from the last answer, and a what-if makes a handful of full
// runs, so neither is split: shard 0 runs it whole.

/// One call's answer, as a shard found it.
#[derive(Debug, Serialize, Deserialize)]
struct CallAnswer {
    /// The call's place in the analysis's sequence of calls.
    index: usize,
    /// [`call_key`] of the call it answers.
    key: String,
    answer: Answer,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Answer {
    Stats {
        stats: MonteCarloStats,
        seeds: Vec<(f64, u64)>,
    },
    Summary(Box<MonteCarloSummary>),
}

/// What identifies a call: its kind, its plan and its Monte Carlo settings.
/// The plan is hashed as canonical JSON (every object's keys sorted), so the
/// same plan gives the same key in every worker: its maps are `HashMap`s,
/// whose order differs from one instance to the next, and serde_json keeps
/// that order when a build turns on `preserve_order`.
fn call_key(kind: &str, config: &SimulationConfig, mc: &MonteCarloConfig) -> String {
    use std::hash::{Hash, Hasher};
    let mut plan = String::new();
    if let Ok(value) = serde_json::to_value(config) {
        canonical(&value, &mut plan);
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (kind, plan, format!("{mc:?}")).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// `value` as JSON text with every object's keys in sorted order.
fn canonical(value: &serde_json::Value, out: &mut String) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (key, value)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                canonical(value, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Whether the analysis's calls can be split across workers.
fn shardable(body: &CreateAnalysis) -> bool {
    matches!(
        body,
        CreateAnalysis::Sweep { .. } | CreateAnalysis::Sensitivity { .. }
    )
}

/// The calls of one shard: its own simulated, the others answered with a
/// placeholder so the loop goes on.
struct Shard<'a> {
    local: Local<'a>,
    shard: usize,
    shards: usize,
    index: usize,
    answers: Vec<CallAnswer>,
}

impl Shard<'_> {
    /// The next call's index, and whether this shard simulates it.
    fn next(&mut self) -> (usize, bool) {
        let index = self.index;
        self.index += 1;
        (index, index % self.shards == self.shard)
    }
}

impl McRunner for Shard<'_> {
    fn stats(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<StatsRun, SimulationError> {
        if self.local.stop {
            return Err(SimulationError::Cancelled);
        }
        let (index, mine) = self.next();
        if !mine {
            return Ok(placeholder(mc));
        }
        let (stats, seeds) = self.local.stats(config, mc)?;
        self.answers.push(CallAnswer {
            index,
            key: call_key("stats", config, mc),
            answer: Answer::Stats {
                stats: stats.clone(),
                seeds: seeds.clone(),
            },
        });
        Ok((stats, seeds))
    }

    /// Not made by a shardable analysis; simulated, and kept, if one does.
    fn summary(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<MonteCarloSummary, SimulationError> {
        let (index, _) = self.next();
        let summary = self.local.summary(config, mc)?;
        self.answers.push(CallAnswer {
            index,
            key: call_key("summary", config, mc),
            answer: Answer::Summary(Box::new(summary.clone())),
        });
        Ok(summary)
    }

    fn cancelled(&self) -> bool {
        self.local.stop
    }

    fn begin(&mut self, total_iterations: usize) {
        self.local.begin(total_iterations);
    }
}

/// A well-formed answer that is not one: what a shard tells the loop about a
/// call another shard simulates. Nothing it feeds is kept.
fn placeholder(mc: &MonteCarloConfig) -> StatsRun {
    let stats = MonteCarloStats {
        num_iterations: mc.iterations,
        success_rate: 0.0,
        funding_success_rate: None,
        mean_final_net_worth: 0.0,
        std_dev_final_net_worth: 0.0,
        min_final_net_worth: 0.0,
        max_final_net_worth: 0.0,
        percentile_values: mc.percentiles.iter().map(|&p| (p, 0.0)).collect(),
        after_tax_percentile_values: mc.percentiles.iter().map(|&p| (p, 0.0)).collect(),
        converged: None,
        convergence_metric: None,
        convergence_value: None,
    };
    (stats, mc.percentiles.iter().map(|&p| (p, 0)).collect())
}

/// The loop again, answered from the shards; anything unanswered simulated.
struct Replay<'a> {
    local: Local<'a>,
    index: usize,
    answers: HashMap<usize, CallAnswer>,
}

impl Replay<'_> {
    fn take(
        &mut self,
        kind: &str,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Option<Answer> {
        let index = self.index;
        self.index += 1;
        let found = self.answers.remove(&index)?;
        (found.key == call_key(kind, config, mc)).then_some(found.answer)
    }
}

impl McRunner for Replay<'_> {
    fn stats(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<StatsRun, SimulationError> {
        match self.take("stats", config, mc) {
            Some(Answer::Stats { stats, seeds }) => Ok((stats, seeds)),
            _ => self.local.stats(config, mc),
        }
    }

    fn summary(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<MonteCarloSummary, SimulationError> {
        match self.take("summary", config, mc) {
            Some(Answer::Summary(summary)) => Ok(*summary),
            _ => self.local.summary(config, mc),
        }
    }

    fn cancelled(&self) -> bool {
        self.local.stop
    }

    // The shards reported the work; a replay that has to simulate reports
    // only that, on top of what is already shown.
    fn begin(&mut self, total_iterations: usize) {
        self.local.done = 0;
        self.local.total = total_iterations;
    }
}

fn attached(graph: &str, library: &str) -> EngineResult<ScenarioGraph> {
    let mut graph: ScenarioGraph = parse("plan", graph)?;
    let library: Library = parse("library", library)?;
    library.attach(&mut graph);
    Ok(graph)
}

/// What [`analysis_plan_json`] returns.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct AnalysisPlan {
    /// `"sweep"`, `"sensitivity"`, `"solve"` or `"what-if"`.
    pub kind: String,
    /// Simulations the analysis will run at most: the progress denominator.
    pub total: i64,
}

fn kind_of(body: &CreateAnalysis) -> &'static str {
    match body {
        CreateAnalysis::Sweep { .. } => "sweep",
        CreateAnalysis::Sensitivity { .. } => "sensitivity",
        CreateAnalysis::Solve { .. } => "solve",
        CreateAnalysis::WhatIf { .. } => "what-if",
    }
}

/// Check an analysis request against the plan and cost it, without simulating:
/// `body` is a `CreateAnalysis`. The refusals are the server's, so the screen
/// hears them when the request is made.
pub fn analysis_plan_json(graph: &str, library: &str, body: &str) -> EngineResult<String> {
    let graph = attached(graph, library)?;
    let body: CreateAnalysis = parse("analysis", body)?;
    let kind = kind_of(&body);
    let prepared = prepare(&graph, body, &limits())?;
    to_json(&AnalysisPlan {
        kind: kind.to_string(),
        total: prepared.spec.budget() as i64,
    })
}

/// Run an analysis to completion on this thread: an `AnalysisOutcome` JSON.
/// `progress` is called with `(simulations done, simulations expected)`.
pub fn analysis_run_json(
    graph: &str,
    library: &str,
    body: &str,
    progress: Progress<'_>,
) -> EngineResult<String> {
    let graph = attached(graph, library)?;
    let body: CreateAnalysis = parse("analysis", body)?;
    let mut runner = Local::new(progress);
    let outcome = analyze(&graph, body, &limits(), &mut runner)?;
    to_json(&outcome)
}

/// Shard `shard` of `shards` of an analysis: its share of the Monte Carlo
/// calls, simulated, as a JSON array for [`analysis_finish_json`]. `progress`
/// reports this shard's simulations against the whole analysis's total. An
/// analysis that cannot be split is run whole by shard 0, and the other shards
/// answer `[]` at once.
pub fn analysis_shard_json(
    graph: &str,
    library: &str,
    body: &str,
    shard: usize,
    shards: usize,
    progress: Progress<'_>,
) -> EngineResult<String> {
    let graph = attached(graph, library)?;
    let body: CreateAnalysis = parse("analysis", body)?;
    let shards = if shardable(&body) { shards.max(1) } else { 1 };
    if shard >= shards {
        return Ok("[]".to_string());
    }
    let mut runner = Shard {
        local: Local::new(progress),
        shard,
        shards,
        index: 0,
        answers: Vec::new(),
    };
    analyze(&graph, body, &limits(), &mut runner)?;
    to_json(&runner.answers)
}

/// The outcome of a sharded analysis: `answers` is a `string[]` JSON of what
/// [`analysis_shard_json`] returned, in any order. The analysis's loop is run
/// once more, answered from them; a call they do not answer is simulated
/// here (reported through `progress`), so the outcome is
/// [`analysis_run_json`]'s whatever the shards did.
pub fn analysis_finish_json(
    graph: &str,
    library: &str,
    body: &str,
    answers: &str,
    progress: Progress<'_>,
) -> EngineResult<String> {
    let graph = attached(graph, library)?;
    let body: CreateAnalysis = parse("analysis", body)?;
    let parts: Vec<String> = parse("shard answers", answers)?;
    let mut found = HashMap::new();
    for part in parts {
        let part: Vec<CallAnswer> = parse("shard answers", &part)?;
        found.extend(part.into_iter().map(|answer| (answer.index, answer)));
    }
    let mut runner = Replay {
        local: Local::new(progress),
        index: 0,
        answers: found,
    };
    let outcome = analyze(&graph, body, &limits(), &mut runner)?;
    to_json(&outcome)
}

/// `POST …/what-if/quick`: the quick pass of a what-if, a `WhatIfOutcome`
/// JSON. `body` is a `QuickWhatIf`.
pub fn quick_what_if_json(
    graph: &str,
    library: &str,
    body: &str,
    progress: Progress<'_>,
) -> EngineResult<String> {
    let graph = attached(graph, library)?;
    let body: QuickWhatIf = parse("what-if", body)?;
    let iterations = quick_iterations(body.iterations, LOCAL_ITERATION_CAP);
    let mut runner = Local::new(progress);
    let outcome = analyze(
        &graph,
        CreateAnalysis::WhatIf {
            layers: body.layers,
            iterations: Some(iterations),
        },
        &limits(),
        &mut runner,
    )?;
    match outcome {
        AnalysisOutcome::WhatIf(outcome) => to_json(&outcome),
        _ => Err(EngineError::internal("what-if produced another analysis")),
    }
}

/// `POST …/what-if/apply`: write the layers into the plan, or, when the body
/// names `new_scenario_name`, into a copy of it (`new_id`, stamped `now`).
/// Returns the written plan's `ScenarioGraph` JSON. Atomic.
pub fn apply_what_if_json(
    graph: &str,
    library: &str,
    body: &str,
    new_id: i64,
    now: &str,
) -> EngineResult<String> {
    let graph = attached(graph, library)?;
    let body: ApplyWhatIf = parse("what-if", body)?;
    let mut target = match body.new_scenario_name.as_deref().map(str::trim) {
        Some(name) => create::duplicate(&graph, new_id, name, now)?,
        None => graph,
    };
    what_if::apply(&mut target, &body.layers)?;
    target.scenario.updated_at = now.to_string();
    to_json(&target)
}

/// What [`local_review_json`] returns.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct LocalReviewResult {
    pub review: LocalReview,
    /// One per note of `review.suggestions`, in order: the key a dismissal is
    /// stored under, and what `silenced` is made of on the next review.
    pub fingerprints: Vec<String>,
}

/// The rule-based review of a finished run: `graph` is the plan the run was
/// made from (its snapshot), `run_results` the run's stored `RunResults`,
/// `silenced` a `string[]` of fingerprints the person has set aside.
pub fn local_review_json(
    graph: &str,
    library: &str,
    run_results: &str,
    run_id: i64,
    reviewed_at: &str,
    silenced: &str,
) -> EngineResult<String> {
    let graph = attached(graph, library)?;
    let run_results: RunResults = parse("run results", run_results)?;
    let silenced: Vec<String> = parse("silenced notes", silenced)?;
    let silenced: HashSet<String> = silenced.into_iter().collect();
    let review = review::local_review(&graph, &run_results, run_id, reviewed_at, &silenced)?;
    let fingerprints = review
        .suggestions
        .iter()
        .map(review::suggestion_fingerprint)
        .collect();
    to_json(&LocalReviewResult {
        review,
        fingerprints,
    })
}

/// `PUT …/what-if`'s check: `body` is a `WhatIfStack`. Returns it back (so a
/// host stores what was validated), or the route's refusal.
pub fn check_what_if_stack_json(body: &str) -> EngineResult<String> {
    let stack: WhatIfStack = parse("what-if", body)?;
    stack.validate()?;
    to_json(&stack)
}

/// One step of a note's path, as `apply_note` takes it.
#[derive(Debug, Deserialize)]
struct NoteStep {
    key: String,
    changes: Vec<Change>,
}

/// Apply the steps of one note's path (`steps`: `[{"key", "changes"}]`, in
/// order) to the plan, or, with `copy` (`new_id`, name), to a copy of it: the
/// twin of `POST /suggestions/{id}/apply`. Each step is resolved against the
/// plan the ones before it left, all or nothing. Returns the written plan's
/// `ScenarioGraph` JSON.
///
/// A refused batch throws the server's body for it: 409 when a change's
/// `expect` no longer matches the plan (it moved since the note was written),
/// 422 for anything else, with `problems` and `by_step` listed.
pub fn apply_note_json(
    graph: &str,
    library: &str,
    path_key: &str,
    steps: &str,
    copy: Option<(i64, &str)>,
    now: &str,
) -> EngineResult<String> {
    let graph = attached(graph, library)?;
    let steps: Vec<NoteStep> = parse("steps", steps)?;
    let library: Library = parse("library", library)?;
    let base = match copy {
        Some((id, name)) => create::duplicate(&graph, id, name, now)?,
        None => graph,
    };
    let batches: Vec<Vec<Change>> = steps.iter().map(|s| s.changes.clone()).collect();
    let stepped = match resolve_steps(&base, &batches, &Created::new())? {
        Ok(stepped) => stepped,
        Err(failed) => {
            let stale = failed
                .problems
                .iter()
                .any(|p| matches!(p, ChangeProblem::Stale { .. }));
            let (status, code, message) = if stale {
                (
                    409,
                    "conflict",
                    "the plan changed since this note was written",
                )
            } else {
                (
                    422,
                    "unprocessable",
                    "the changes cannot be applied to this plan",
                )
            };
            let problems = serde_json::to_value(&failed.problems)
                .map_err(|e| EngineError::internal(e.to_string()))?;
            let step = steps.get(failed.step).map(|s| s.key.clone());
            return Err(EngineError {
                status,
                code: code.to_string(),
                message: message.to_string(),
                body: Some(json!({
                    "error": {"code": code, "message": message},
                    "problems": problems,
                    "by_step": [{"path": path_key, "step": step, "problems": problems}],
                })),
            });
        }
    };
    let mut written = stepped.graph;
    written.scenario.updated_at = now.to_string();
    library.attach(&mut written);
    to_json(&written)
}

// ── drawdown ───────────────────────────────────────────────────────────────

/// `POST /runs/{id}/drawdown`: the yearly rows of a run's path under each
/// strategy. `snapshot` is the run's input snapshot, `seed` the decimal seed of
/// its median path (a `u64` does not fit a JS number), `request` a
/// `DrawdownRequest`. A `DrawdownBody` JSON.
pub fn drawdown_json(snapshot: &str, seed: &str, request: &str) -> EngineResult<String> {
    let graph: ScenarioGraph = parse("snapshot", snapshot)?;
    let seed: u64 = seed
        .trim()
        .parse()
        .map_err(|_| EngineError::bad_request("seed is not a whole number"))?;
    let request: DrawdownRequest = parse("drawdown request", request)?;
    to_json(&drawdown::project(&graph, seed, &request)?)
}

/// `POST /runs/{id}/drawdown/compare`: a Monte Carlo per strategy on one common
/// seed. `body` is a `CompareRequest`; `progress` is called with
/// `(simulations done, expected)`. A `DrawdownComparison` JSON.
pub fn drawdown_compare_json(
    snapshot: &str,
    body: &str,
    progress: Progress<'_>,
) -> EngineResult<String> {
    let graph: ScenarioGraph = parse("snapshot", snapshot)?;
    let body: CompareRequest = parse("comparison request", body)?;
    let mut runner = Local::new(progress);
    to_json(&drawdown::compare(&graph, &body, &mut runner)?)
}
