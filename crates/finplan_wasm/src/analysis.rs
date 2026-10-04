//! Analyses, what-ifs and the rule-based review, on a plan held on the device.
//!
//! The loops that decide which configurations to simulate (the sweep grid, the
//! goal seek's bisection, the stack of what-if layers) are
//! `finplan_plan::analysis`; this module only gives them somewhere to simulate:
//! [`Sequential`], an `McRunner` that runs each Monte Carlo call to completion
//! on the calling thread. A browser runs it in a compute worker, so a long
//! analysis never blocks the page; the answer is the same as the server's for
//! the same plan and request (the batch plan, [`LOCAL_PARALLEL_BATCHES`], is
//! fixed rather than the machine's core count).
//!
//! **Progress and cancelling.** The call is synchronous, so the worker cannot
//! hear a "cancel" message while it runs; cancelling an analysis there is
//! terminating the worker. A host that can poll (the in-process runner the
//! tests use) returns `true` from the progress callback to stop: the next
//! simulation is not started and the call fails with `409 conflict`.

use std::collections::HashSet;

use finplan_core::analysis::{McRunner, StatsRun};
use finplan_core::config::SimulationConfig;
use finplan_core::error::SimulationError;
use finplan_core::model::{MonteCarloConfig, MonteCarloProgress, MonteCarloSummary};
use finplan_core::simulation::{monte_carlo_simulate_with_progress, monte_carlo_stats_only};
use finplan_plan::analysis::{
    AnalysisError, AnalysisOutcome, CreateAnalysis, Limits, MAX_ANALYSIS_ITERATIONS, analyze,
    prepare,
};
use finplan_plan::create;
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

/// Runs every Monte Carlo call on the calling thread, one after another,
/// reporting after each.
struct Sequential<'a> {
    progress: Progress<'a>,
    done: usize,
    total: usize,
    stop: bool,
}

impl<'a> Sequential<'a> {
    fn new(progress: Progress<'a>) -> Self {
        Sequential {
            progress,
            done: 0,
            total: 0,
            stop: false,
        }
    }

    fn advance(&mut self, iterations: usize) {
        self.done += iterations;
        // A phase may run past the count it announced (a sweep's baseline run
        // follows its grid), so the total grows to meet it.
        self.total = self.total.max(self.done);
        self.stop = (self.progress)(self.done, self.total) || self.stop;
    }
}

impl McRunner for Sequential<'_> {
    fn stats(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<StatsRun, SimulationError> {
        if self.stop {
            return Err(SimulationError::Cancelled);
        }
        let out = monte_carlo_stats_only(config, mc, &MonteCarloProgress::default())?;
        self.advance(mc.iterations);
        Ok(out)
    }

    fn summary(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<MonteCarloSummary, SimulationError> {
        if self.stop {
            return Err(SimulationError::Cancelled);
        }
        let out = monte_carlo_simulate_with_progress(config, mc, &MonteCarloProgress::default())?;
        self.advance(mc.iterations);
        Ok(out)
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

/// Run an analysis to completion: an `AnalysisOutcome` JSON. `progress` is
/// called with `(simulations done, simulations expected)`.
pub fn analysis_run_json(
    graph: &str,
    library: &str,
    body: &str,
    progress: Progress<'_>,
) -> EngineResult<String> {
    let graph = attached(graph, library)?;
    let body: CreateAnalysis = parse("analysis", body)?;
    let mut runner = Sequential::new(progress);
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
    let mut runner = Sequential::new(progress);
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
