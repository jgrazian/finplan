//! Local runs, split the way the browser runs them: one coordinator in the
//! store worker, any number of compute workers.
//!
//! ```text
//! store worker                                   compute worker(s)
//! coordinator_new(snapshot, settings) -> h
//! loop {                                          prepare(snapshot, settings) -> p   (once)
//!   specs = coordinator_next_round(h)  ──specs──► run_batch(p, spec) -> output       (per batch)
//!   if specs is undefined: break       ◄─output──
//!   coordinator_absorb(h, outputs)
//! }
//! results = coordinator_finish(h)                 release(p)
//! coordinator_drop(h)
//! ```
//!
//! A run is a plan of batches (how many, how big, which seed each starts from),
//! and the plan is a function of the settings alone, never of how many workers
//! there are or which finishes first: `coordinator_absorb` merges in batch order
//! whatever order the outputs arrive in. The same settings and seed therefore
//! give the same `RunResults` however the batches were spread, and the same as
//! `finplan_plan::results::project` over `monte_carlo_simulate_with_config`.
//!
//! **A `BatchOutput` and a `BatchSpec` are opaque strings.** Iteration seeds are
//! full 64-bit integers, which `JSON.parse` rounds. Pass them between workers as
//! strings; build the array `coordinator_absorb` wants by joining them
//! (`"[" + outputs.join(",") + "]"`). Never parse and re-stringify one.
//!
//! Handles are small integers from separate registries for coordinators and for
//! prepared runs, valid in the instance that made them. Drop what you are done
//! with (`coordinator_drop`, `release`): an instance lives as long as its
//! worker.

use std::cell::RefCell;
use std::collections::HashMap;

use finplan_core::model::MonteCarloConfig;
use finplan_core::simulation::{
    BatchOutput, BatchSpec, MonteCarloCoordinator, PreparedRun, base_seed, prepare_run, run_batch,
    run_batch_observed,
};
use finplan_plan::PlanError;
use finplan_plan::compile::{CompiledScenario, compile};
use finplan_plan::graph::ScenarioGraph;
use finplan_plan::results::{RunSettings, project};
use finplan_plan::run::{CreateRun, NO_ITERATION_CAP, ValidatedRun, run_cost};
use serde::Serialize;
use ts_rs::TS;

use crate::error::{EngineError, EngineResult, parse, to_json};

/// The largest seed a run takes: seeds travel as JSON numbers, which are exact
/// up to 2^53 - 1. (The browser draws 32 bits.)
pub const MAX_SEED: i64 = (1 << 53) - 1;

/// What [`run_cost_json`] returns.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct RunCost {
    /// `sample x duration_years x max(accounts + assets + events, 1)`: the
    /// unit the server's run limits and the browser's time estimate are in.
    pub cost: i64,
    /// The iterations the cost was taken over: the count, or a converging
    /// run's ceiling (the most it can spend).
    pub sample: i64,
    /// The batches a round is split into: how many workers a run can use at once.
    pub parallel_batches: i64,
}

/// What a coordinator says about its run: [`coordinator_info_json`].
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct RunInfo {
    /// Fixed runs: the count. Converging runs: the minimum sample.
    pub iterations: i64,
    /// A converging run's ceiling; null on a fixed run.
    pub max_iterations: Option<i64>,
    pub batch_size: i64,
    pub parallel_batches: i64,
    pub seed: i64,
    pub cost: i64,
    /// The settings as run, defaults filled in, for the run's inputs.
    pub converge: bool,
    pub compute_mean: bool,
    pub percentiles: Vec<f64>,
}

/// What a run costs, and whether the settings are acceptable: the same checks
/// the server applies, with no iteration cap. `graph` is a `ScenarioGraph` or a
/// snapshot (the same JSON); `settings` is a `CreateRun`.
pub fn run_cost_json(graph: &str, settings: &str) -> EngineResult<String> {
    let graph: ScenarioGraph = parse("plan", graph)?;
    let settings: CreateRun = parse("run settings", settings)?;
    let validated = settings.validate(NO_ITERATION_CAP)?;
    let sample = validated.sample();
    to_json(&RunCost {
        cost: run_cost(&graph, sample)?,
        sample,
        parallel_batches: settings.parallel_batches,
    })
}

/// A run's settings and the plan, checked, compiled and turned into the engine's
/// configuration: what the coordinator and the compute side must agree on.
struct Setup {
    compiled: CompiledScenario,
    settings: CreateRun,
    validated: ValidatedRun,
    config: MonteCarloConfig,
    cost: i64,
}

fn setup(snapshot: &str, settings: &str) -> EngineResult<Setup> {
    let graph: ScenarioGraph = parse("snapshot", snapshot)?;
    let settings: CreateRun = parse("run settings", settings)?;
    let validated = settings.validate(NO_ITERATION_CAP)?;
    if graph.scenario.duration_years < 1 {
        return Err(EngineError::bad_request("duration must be at least 1 year"));
    }
    let cost = run_cost(&graph, validated.sample())?;
    let compiled = compile(&graph)?;
    let config = settings.mc_config(&validated, settings.seed);
    Ok(Setup {
        compiled,
        settings,
        validated,
        config,
        cost,
    })
}

// ── handles ────────────────────────────────────────────────────────────────

struct Registry<T> {
    next: u32,
    items: HashMap<u32, T>,
}

impl<T> Registry<T> {
    fn new() -> Self {
        Registry {
            next: 1,
            items: HashMap::new(),
        }
    }

    fn insert(&mut self, item: T) -> u32 {
        let handle = self.next;
        self.next += 1;
        self.items.insert(handle, item);
        handle
    }

    fn get_mut(&mut self, handle: u32) -> EngineResult<&mut T> {
        self.items
            .get_mut(&handle)
            .ok_or_else(|| PlanError::NotFound("run").into())
    }

    fn remove(&mut self, handle: u32) {
        self.items.remove(&handle);
    }
}

struct LocalRun {
    compiled: CompiledScenario,
    info: RunInfo,
    /// Taken by `coordinator_finish`.
    coordinator: Option<MonteCarloCoordinator>,
}

thread_local! {
    // wasm32-unknown-unknown has one thread, so one registry per instance; a
    // native test thread gets its own.
    static COORDINATORS: RefCell<Registry<LocalRun>> = RefCell::new(Registry::new());
    static PREPARED: RefCell<Registry<PreparedRun>> = RefCell::new(Registry::new());
}

// ── coordinator side (store worker) ────────────────────────────────────────

/// Start a run: check the settings, compile the plan and plan the batches.
/// `snapshot` is a snapshot JSON (what `snapshot` returns); `settings` is a
/// `CreateRun` whose `seed` is required (there is no OS entropy on wasm32: draw
/// one with `crypto.getRandomValues`), within 0..=2^53-1.
pub fn coordinator_new(snapshot: &str, settings: &str) -> EngineResult<u32> {
    let setup = setup(snapshot, settings)?;
    let seed = setup.settings.seed.ok_or_else(|| {
        EngineError::bad_request("a local run needs a seed (draw one with crypto.getRandomValues)")
    })?;
    if !(0..=MAX_SEED).contains(&seed) {
        return Err(EngineError::bad_request(format!(
            "seed must be between 0 and {MAX_SEED}"
        )));
    }
    let coordinator = MonteCarloCoordinator::new(&setup.config, base_seed(&setup.config)?)?;
    let info = RunInfo {
        iterations: setup.validated.iterations,
        max_iterations: setup.validated.ceiling,
        batch_size: setup.settings.batch_size,
        parallel_batches: setup.settings.parallel_batches,
        seed,
        cost: setup.cost,
        converge: setup.settings.converge,
        compute_mean: setup.settings.compute_mean,
        percentiles: setup.validated.percentiles.clone(),
    };
    Ok(COORDINATORS.with_borrow_mut(|registry| {
        registry.insert(LocalRun {
            compiled: setup.compiled,
            info,
            coordinator: Some(coordinator),
        })
    }))
}

fn with_coordinator<T>(
    handle: u32,
    f: impl FnOnce(&mut LocalRun, &mut MonteCarloCoordinator) -> EngineResult<T>,
) -> EngineResult<T> {
    COORDINATORS.with_borrow_mut(|registry| {
        let run = registry.get_mut(handle)?;
        let mut coordinator = run
            .coordinator
            .take()
            .ok_or_else(|| EngineError::new(409, "conflict", "the run has already finished"))?;
        let result = f(run, &mut coordinator);
        run.coordinator = Some(coordinator);
        result
    })
}

/// The run's settings as planned: `RunInfo` JSON.
pub fn coordinator_info_json(handle: u32) -> EngineResult<String> {
    COORDINATORS.with_borrow_mut(|registry| to_json(&registry.get_mut(handle)?.info))
}

/// The next round of batches to run, as `BatchSpec[]` JSON, or `None` when the
/// run is over (the count is reached, the metric converged, or the ceiling is
/// hit). A converging run's rounds depend on the outputs absorbed so far, so
/// ask again after each `coordinator_absorb`. Asking twice before absorbing
/// returns the same round.
pub fn coordinator_next_round(handle: u32) -> EngineResult<Option<String>> {
    with_coordinator(handle, |_, coordinator| {
        coordinator
            .next_round()
            .map(|specs| to_json(&specs))
            .transpose()
    })
}

/// Merge a finished round: `outputs` is the `BatchOutput[]` JSON, one per
/// `BatchSpec` of the round, in any order. A round that does not match what
/// `coordinator_next_round` handed out is refused and merges nothing.
pub fn coordinator_absorb(handle: u32, outputs: &str) -> EngineResult<()> {
    let outputs: Vec<BatchOutput> = parse("batch outputs", outputs)?;
    with_coordinator(handle, |_, coordinator| Ok(coordinator.absorb(outputs)?))
}

/// Iterations merged so far, for progress.
pub fn coordinator_completed(handle: u32) -> EngineResult<u32> {
    with_coordinator(handle, |_, coordinator| Ok(coordinator.completed() as u32))
}

/// Finish the run: the statistics, phase 2 (the percentile paths re-simulated
/// with their ledgers) and the projection into `RunResults` JSON. Consumes the
/// run; the handle is still to be dropped.
pub fn coordinator_finish(handle: u32) -> EngineResult<String> {
    COORDINATORS.with_borrow_mut(|registry| {
        let run = registry.get_mut(handle)?;
        let coordinator = run
            .coordinator
            .take()
            .ok_or_else(|| EngineError::new(409, "conflict", "the run has already finished"))?;
        let summary = coordinator.finish(&run.compiled.config, true)?;
        to_json(&project(&run.compiled, &summary, &RunSettings::default()))
    })
}

/// Forget a run.
pub fn coordinator_drop(handle: u32) {
    COORDINATORS.with_borrow_mut(|registry| registry.remove(handle));
}

// ── compute side (compute workers) ─────────────────────────────────────────

/// Prepare a plan for running batches: the same checks and compilation as
/// `coordinator_new` (a plan the engine cannot run fails here, before any batch
/// is dispatched), keeping only what batches need. `settings` is the run's
/// `CreateRun`, as given to the coordinator; the seed is not used.
pub fn prepare(snapshot: &str, settings: &str) -> EngineResult<u32> {
    let setup = setup(snapshot, settings)?;
    let prepared = prepare_run(&setup.compiled.config, &setup.config)?;
    Ok(PREPARED.with_borrow_mut(|registry| registry.insert(prepared)))
}

/// Run one batch: `spec` is a `BatchSpec` JSON from `coordinator_next_round`;
/// returns `BatchOutput` JSON for `coordinator_absorb`.
pub fn run_batch_json(handle: u32, spec: &str) -> EngineResult<String> {
    let spec: BatchSpec = parse("batch spec", spec)?;
    PREPARED.with_borrow_mut(|registry| {
        let prepared = registry.get_mut(handle)?;
        to_json(&run_batch(prepared, &spec, None)?)
    })
}

/// How many progress reports a batch makes at most, besides its last.
const REPORTS_PER_BATCH: usize = 50;

/// [`run_batch_json`], calling `report(done, total)` as the batch's
/// simulations finish (about [`REPORTS_PER_BATCH`] times, and always at the
/// end), so a worker can post progress while the batch holds its thread.
/// Returning true from `report` cancels the batch. The output is
/// [`run_batch_json`]'s: reporting does not touch the simulation.
pub fn run_batch_reporting_json(
    handle: u32,
    spec: &str,
    report: &mut dyn FnMut(usize, usize) -> bool,
) -> EngineResult<String> {
    let spec: BatchSpec = parse("batch spec", spec)?;
    let total = spec.iterations;
    let step = (total / REPORTS_PER_BATCH).max(1);
    PREPARED.with_borrow_mut(|registry| {
        let prepared = registry.get_mut(handle)?;
        let output = run_batch_observed(prepared, &spec, None, |done| {
            (done % step == 0 || done == total) && report(done, total)
        })?;
        to_json(&output)
    })
}

/// Forget a prepared plan.
pub fn release(handle: u32) {
    PREPARED.with_borrow_mut(|registry| registry.remove(handle));
}
