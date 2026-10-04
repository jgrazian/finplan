/**
 * Local runs (spec 19, phases 2 and 3): the coordinator in the store worker, the
 * pool of compute workers, the stored results, and the estimate and calibration
 * that tell the UI what a run will cost on this device.
 *
 * A run's life:
 *
 *  1. `create` snapshots the plan, asks the engine for a coordinator (which
 *     checks the settings and compiles the plan, so a bad run is refused here,
 *     as the server refuses it at POST), stores a `queued` run and answers with
 *     it. Nothing waits for the simulation.
 *  2. In the background, holding the run's Web Lock, the coordinator asks for a
 *     round of batches, the pool runs them in parallel, the coordinator merges
 *     them; a converging run goes round after round until the metric settles.
 *     Progress (`completed_iterations`) is kept in memory and merged into what
 *     `get` and `list` answer, so a poll costs no write.
 *  3. The coordinator finishes (the percentile paths are re-simulated with their
 *     ledgers and the projection is made), and the run and its `RunResults` are
 *     stored in one transaction, the plan's older runs beyond the newest five
 *     removed with it.
 *
 * Cancelling stops the dispatch and terminates the workers running the run's
 * batches. A run found `queued` or `running` in the store with nobody holding
 * its lock is the leftover of a closed page, and is marked failed at startup.
 */
import type { LedgerPage } from "../api/generated/LedgerPage.ts";
import type { RunComparison } from "../api/generated/RunComparison.ts";
import type { RunInputs } from "../api/generated/RunInputs.ts";
import type { RunReport } from "../api/generated/RunReport.ts";
import type { RunInfo } from "../api/generated/RunInfo.ts";
import type { RunCost } from "../api/generated/RunCost.ts";
import type { PlanArchive } from "../api/generated/PlanArchive.ts";
import type { RunsApi } from "../api/plan.ts";
import type { CreateRun, Results, Run } from "../api/types.ts";
import type { LocalPlanMeta, RunEstimate } from "../local/runtime.ts";
import type { Core } from "./core.ts";
import { joinOutputs, splitSpecs } from "./engine.ts";
import { CANCELLED, badRequest, conflict, guard, localError, notFound, toLocalError } from "./errors.ts";
import { planLockName, tryLock, withLock } from "./locks.ts";
import { KEEP_RUNS_PER_PLAN, type RunRecord, type StoreTx } from "./store.ts";

const runLockName = (runId: number) => `finplan-run-${runId}`;

/** Cost units per second per worker before a calibration has measured one: about a mid-range laptop on the default plan. */
export const DEFAULT_RATE = 400_000;
const CALIBRATION_KEY = "calibration";

/** What a calibration stores in `meta`. */
export interface CalibrationRecord {
  /** Cost units per second, per worker. */
  rate: number;
  measured_at: string;
  model_version: string;
}

/** A converging run usually stops within a couple of times its minimum sample. */
const CONVERGE_FACTOR = 2;

const isTerminal = (status: RunRecord["status"]) =>
  status === "succeeded" || status === "failed" || status === "canceled";

function toRun(record: RunRecord, progress?: number): Run {
  return {
    id: record.id,
    scenario_id: record.plan_id,
    status: record.status,
    iterations: record.iterations,
    completed_iterations: progress ?? record.completed_iterations,
    converge: record.settings.converge,
    max_iterations: record.max_iterations,
    seed: record.seed,
    input_hash: record.input_hash,
    model_version: record.model_version,
    error_message: record.error_message,
    created_at: record.created_at,
    started_at: record.started_at,
    finished_at: record.finished_at,
  };
}

interface ActiveRun {
  planId: number;
  cancelled: boolean;
  completed: number;
}

/** The newest few opened `RunResults`, so reading one run's views parses it once. */
class ResultsCache {
  private handles = new Map<number, number>();
  private core: Core;
  private limit: number;

  constructor(core: Core, limit = 3) {
    this.core = core;
    this.limit = limit;
  }

  async open(runId: number): Promise<number> {
    const known = this.handles.get(runId);
    if (known !== undefined) {
      // Most recently used goes last.
      this.handles.delete(runId);
      this.handles.set(runId, known);
      return known;
    }
    const json = await this.core.store.transact("r", (tx) => tx.getResults(runId));
    if (json === undefined) throw conflict("this run has no results");
    const handle = guard(() => this.core.engine.results_open(json));
    this.handles.set(runId, handle);
    while (this.handles.size > this.limit) {
      const [oldest] = this.handles.keys();
      this.forget(oldest);
    }
    return handle;
  }

  /** Keep an already-opened handle (a run that just finished is read next). */
  adopt(runId: number, handle: number): void {
    this.forget(runId);
    this.handles.set(runId, handle);
    while (this.handles.size > this.limit) {
      const [oldest] = this.handles.keys();
      this.forget(oldest);
    }
  }

  forget(runId: number): void {
    const handle = this.handles.get(runId);
    if (handle === undefined) return;
    this.handles.delete(runId);
    try {
      this.core.engine.results_close(handle);
    } catch {
      // The instance is gone; nothing to close.
    }
  }
}

export class Runs {
  private core: Core;
  private active = new Map<number, ActiveRun>();
  private cache: ResultsCache;
  private calibrating: Promise<CalibrationRecord | undefined> | undefined;

  constructor(core: Core) {
    this.core = core;
    this.cache = new ResultsCache(core);
  }

  // ── startup ──────────────────────────────────────────────────────────────

  /** Mark runs a closed page left half-done as failed, unless another tab is still running them. */
  async recoverInterrupted(): Promise<void> {
    const orphaned = await this.core.store.transact("r", async (tx) => {
      const out: RunRecord[] = [];
      for (const plan of await tx.listPlans()) {
        for (const run of await tx.listRuns(plan.id)) {
          if (!isTerminal(run.status)) out.push(run);
        }
      }
      return out;
    });
    for (const run of orphaned) {
      if (this.active.has(run.id)) continue;
      // The lock is free only if no tab is executing the run.
      const free = await tryLock(runLockName(run.id), async () => {
        await this.core.store.transact("rw", async (tx) => {
          const current = await tx.getRun(run.id);
          if (current && !isTerminal(current.status)) {
            await tx.putRun({
              ...current,
              status: "failed",
              error_message: "The page was closed before the run finished.",
              finished_at: this.core.nowText(),
            });
          }
        });
      });
      if (free) this.core.announce(run.plan_id);
    }
  }

  // ── the API ──────────────────────────────────────────────────────────────

  api: RunsApi = {
    list: (scenarioId) =>
      this.core.store.transact("r", async (tx) => {
        await this.core.requirePlan(tx, scenarioId);
        return (await tx.listRuns(scenarioId)).map((run) => this.view(run));
      }),

    create: (scenarioId, body) => this.create(scenarioId, body ?? {}),

    get: (id) =>
      this.core.store.transact("r", async (tx) => this.view(await this.requireRun(tx, id))),

    cancel: (id) => this.cancel(id),

    remove: async (id) => {
      const run = await this.core.store.transact("r", (tx) => this.requireRun(tx, id));
      this.stop(id);
      await withLock(planLockName(run.plan_id), () =>
        this.core.store.transact("rw", (tx) => tx.deleteRun(id)),
      );
      this.cache.forget(id);
      this.core.announce(run.plan_id);
    },

    results: async (id, series) => {
      const run = await this.core.store.transact("r", (tx) => this.requireRun(tx, id));
      this.requireSucceeded(run);
      const handle = await this.cache.open(id);
      return this.core.call<Results>((engine) =>
        engine.results_view_open(handle, id, run.plan_id, series ?? null),
      );
    },

    ledger: async (id, query) => {
      const run = await this.core.store.transact("r", (tx) => this.requireRun(tx, id));
      this.requireSucceeded(run);
      const handle = await this.cache.open(id);
      return this.core.call<LedgerPage>((engine) =>
        engine.ledger_page_open(handle, id, JSON.stringify(query ?? {})),
      );
    },

    inputs: (id) => this.core.store.transact("r", async (tx) => this.inputsOf(await this.requireRun(tx, id))),

    report: async (id) => this.reportOf(id),

    compare: async (left, right) =>
      ({ left: await this.reportOf(left), right: await this.reportOf(right) }) satisfies RunComparison,

    archive: async (id) => {
      const run = await this.core.store.transact("r", (tx) => this.requireRun(tx, id));
      if (!run.snapshot) throw conflict("This legacy run has no restorable inputs.");
      return this.core.call<PlanArchive>((engine) => engine.export_archive(`[${run.snapshot}]`));
    },
  };

  // ── helpers ──────────────────────────────────────────────────────────────

  private view(record: RunRecord): Run {
    const active = this.active.get(record.id);
    return toRun(record, active && !isTerminal(record.status) ? active.completed : undefined);
  }

  private async requireRun(tx: StoreTx, id: number): Promise<RunRecord> {
    const run = Number.isInteger(id) ? await tx.getRun(id) : undefined;
    if (!run) throw notFound("run");
    return run;
  }

  private requireSucceeded(run: RunRecord): void {
    if (run.status !== "succeeded") throw conflict("this run has no results yet");
  }

  private inputsOf(run: RunRecord): RunInputs {
    return {
      run_id: run.id,
      seed: run.seed,
      iterations: run.iterations,
      converge: run.settings.converge,
      max_iterations: run.max_iterations,
      batch_size: run.settings.batch_size,
      parallel_batches: run.settings.parallel_batches,
      compute_mean: run.settings.compute_mean,
      percentiles: run.settings.percentiles,
      input_hash: run.input_hash,
      model_version: run.model_version,
      snapshot: run.snapshot ? (JSON.parse(run.snapshot) as unknown) : null,
    };
  }

  private async reportOf(id: number): Promise<RunReport> {
    const run = await this.core.store.transact("r", (tx) => this.requireRun(tx, id));
    return { results: await this.api.results(id), inputs: this.inputsOf(run) };
  }

  /** Keep the plan's newest runs; the rest, and their results, are removed. */
  private async prune(tx: StoreTx, planId: number): Promise<number[]> {
    const runs = await tx.listRuns(planId);
    const doomed = runs.slice(KEEP_RUNS_PER_PLAN).filter((run) => isTerminal(run.status));
    for (const run of doomed) await tx.deleteRun(run.id);
    return doomed.map((run) => run.id);
  }

  // ── making a run ─────────────────────────────────────────────────────────

  private async create(planId: number, body: Partial<CreateRun>): Promise<Run> {
    this.core.object(body, "the run settings");
    let coordinator = -1;
    let snapshot = "";
    let settings = "";
    let info: RunInfo;
    const record = await withLock(planLockName(planId), () =>
      this.core.store.transact("rw", async (tx) => {
        const plan = await this.core.requirePlan(tx, planId);
        const library = await this.core.library(tx);
        const shot = this.core.call<{ json: string; hash: string }>((engine) =>
          engine.snapshot(plan.graph, library),
        );
        snapshot = shot.json;
        settings = JSON.stringify({ ...body, seed: body.seed ?? this.core.seed() });
        coordinator = guard(() => this.core.engine.coordinator_new(snapshot, settings));
        info = this.core.call<RunInfo>((engine) => engine.coordinator_info(coordinator));
        const id = await tx.nextId("run");
        const created: RunRecord = {
          id,
          plan_id: planId,
          status: "queued",
          settings: {
            iterations: info.iterations,
            percentiles: info.percentiles,
            seed: info.seed,
            batch_size: info.batch_size,
            parallel_batches: info.parallel_batches,
            compute_mean: info.compute_mean,
            converge: info.converge,
          },
          iterations: info.iterations,
          max_iterations: info.max_iterations,
          completed_iterations: 0,
          seed: info.seed,
          input_hash: shot.hash,
          model_version: this.core.engine.model_version(),
          engine: "wasm",
          error_message: null,
          created_at: this.core.nowText(),
          started_at: null,
          finished_at: null,
          snapshot,
          success_rate: null,
        };
        await tx.putRun(created);
        for (const gone of await this.prune(tx, planId)) this.cache.forget(gone);
        return created;
      }).catch((error: unknown) => {
        // The transaction failed or was refused: nothing was stored, so the coordinator is not needed.
        if (coordinator >= 0) this.core.engine.coordinator_drop(coordinator);
        throw error;
      }),
    );
    this.active.set(record.id, { planId, cancelled: false, completed: 0 });
    void withLock(runLockName(record.id), () => this.execute(record, coordinator, snapshot, settings)).catch(
      () => undefined,
    );
    this.core.announce(planId);
    return toRun(record);
  }

  private async patch(runId: number, change: Partial<RunRecord>): Promise<RunRecord | undefined> {
    return this.core.store.transact("rw", async (tx) => {
      const current = await tx.getRun(runId);
      if (!current) return undefined;
      const next = { ...current, ...change };
      await tx.putRun(next);
      return next;
    });
  }

  private async execute(record: RunRecord, coordinator: number, snapshot: string, settings: string): Promise<void> {
    const { engine, pool } = this.core;
    const job = `run-${record.id}`;
    const state = this.active.get(record.id) as ActiveRun;
    const stopped = () => localError(409, "conflict", `run ${CANCELLED}`);
    try {
      if (state.cancelled) throw stopped();
      await this.patch(record.id, { status: "running", started_at: this.core.nowText() });
      for (;;) {
        if (state.cancelled) throw stopped();
        const round = guard(() => engine.coordinator_next_round(coordinator));
        if (round === undefined) break;
        const outputs = await pool.runBatches({ job, snapshot, settings }, splitSpecs(round));
        if (state.cancelled) throw stopped();
        guard(() => engine.coordinator_absorb(coordinator, joinOutputs(outputs)));
        state.completed = guard(() => engine.coordinator_completed(coordinator));
      }
      if (state.cancelled) throw stopped();
      const results = guard(() => engine.coordinator_finish(coordinator));
      await this.store(record, results, state);
    } catch (thrown) {
      const error = toLocalError(thrown);
      const cancelledByUser = state.cancelled;
      pool.cancelJob(job);
      await this.patch(record.id, {
        status: cancelledByUser ? "canceled" : "failed",
        error_message: cancelledByUser ? null : error.message,
        finished_at: this.core.nowText(),
        completed_iterations: state.completed,
      }).catch(() => undefined);
    } finally {
      pool.endJob(job);
      try {
        engine.coordinator_drop(coordinator);
      } catch {
        // The instance was replaced.
      }
      this.active.delete(record.id);
      this.core.announce(record.plan_id);
    }
  }

  /** The finished run and its results, in one transaction, with the plan's older runs pruned. */
  private async store(record: RunRecord, results: string, state: ActiveRun): Promise<void> {
    // Parse once: the success rate for the plan list, and the handle the first read will want.
    const handle = guard(() => this.core.engine.results_open(results));
    const view = this.core.call<Results>((engine) =>
      engine.results_view_open(handle, record.id, record.plan_id, null),
    );
    const doomed = await this.core.store.transact("rw", async (tx) => {
      const current = await tx.getRun(record.id);
      // Deleted or cancelled while the last step ran: there is nothing to keep.
      if (!current || current.status === "canceled") return undefined;
      await tx.putResults(record.id, results);
      await tx.putRun({
        ...current,
        status: "succeeded",
        completed_iterations: view.stats.num_iterations,
        success_rate: view.stats.success_rate,
        error_message: null,
        finished_at: this.core.nowText(),
      });
      return this.prune(tx, record.plan_id);
    });
    if (doomed === undefined) {
      this.core.engine.results_close(handle);
      return;
    }
    for (const gone of doomed) this.cache.forget(gone);
    this.cache.adopt(record.id, handle);
    state.completed = view.stats.num_iterations;
  }

  /** Stop a run in progress: no more dispatching, and the workers on it are terminated. */
  private stop(runId: number): void {
    const state = this.active.get(runId);
    if (!state) return;
    state.cancelled = true;
    this.core.pool.cancelJob(`run-${runId}`);
  }

  private async cancel(id: number): Promise<Run> {
    const run = await this.core.store.transact("r", (tx) => this.requireRun(tx, id));
    if (isTerminal(run.status)) return this.view(run);
    this.stop(id);
    const next = await this.patch(id, {
      status: "canceled",
      finished_at: this.core.nowText(),
      completed_iterations: this.active.get(id)?.completed ?? run.completed_iterations,
    });
    this.core.announce(run.plan_id);
    return toRun(next ?? run);
  }

  /** A plan is being deleted: stop its runs and forget their cached results. */
  forgetPlan(planId: number): void {
    for (const [id, state] of this.active) {
      if (state.planId === planId) {
        this.stop(id);
        this.cache.forget(id);
      }
    }
  }

  // ── offloaded runs ───────────────────────────────────────────────────────

  /** Store results the server computed (offload) as a run of this plan. */
  async saveServerRun(
    planId: number,
    run: { settings: Partial<CreateRun>; inputHash: string; modelVersion: string; results: unknown },
  ): Promise<Run> {
    const resultsJson = JSON.stringify(run.results);
    const handle = guard(() => this.core.engine.results_open(resultsJson));
    let saved: RunRecord;
    try {
      const view = this.core.call<Results>((engine) => engine.results_view_open(handle, 0, planId, null));
      saved = await withLock(planLockName(planId), () =>
        this.core.store.transact("rw", async (tx) => {
          const plan = await this.core.requirePlan(tx, planId);
          const library = await this.core.library(tx);
          const shot = this.core.call<{ json: string; hash: string }>((engine) =>
            engine.snapshot(plan.graph, library),
          );
          // The settings as the engine would normalise them, defaults filled in.
          const probe = guard(() =>
            this.core.engine.coordinator_new(shot.json, JSON.stringify({ ...run.settings, seed: run.settings.seed ?? 1 })),
          );
          let info: RunInfo;
          try {
            info = this.core.call<RunInfo>((engine) => engine.coordinator_info(probe));
          } finally {
            this.core.engine.coordinator_drop(probe);
          }
          const id = await tx.nextId("run");
          const now = this.core.nowText();
          const record: RunRecord = {
            id,
            plan_id: planId,
            status: "succeeded",
            settings: {
              iterations: info.iterations,
              percentiles: info.percentiles,
              seed: run.settings.seed ?? null,
              batch_size: info.batch_size,
              parallel_batches: info.parallel_batches,
              compute_mean: info.compute_mean,
              converge: info.converge,
            },
            iterations: info.iterations,
            max_iterations: info.max_iterations,
            completed_iterations: view.stats.num_iterations,
            seed: run.settings.seed ?? null,
            input_hash: run.inputHash,
            model_version: run.modelVersion,
            engine: "server",
            error_message: null,
            created_at: now,
            started_at: now,
            finished_at: now,
            // Kept only when it is the very plan the server ran.
            snapshot: shot.hash === run.inputHash ? shot.json : null,
            success_rate: view.stats.success_rate,
          };
          await tx.putRun(record);
          await tx.putResults(id, resultsJson);
          for (const gone of await this.prune(tx, planId)) this.cache.forget(gone);
          return record;
        }),
      );
    } finally {
      this.core.engine.results_close(handle);
    }
    this.core.announce(planId);
    return toRun(saved);
  }

  // ── estimates and calibration ────────────────────────────────────────────

  /** The calibration rate stored for this device, or the default when none has been measured. */
  async rate(): Promise<{ rate: number; calibrated: boolean }> {
    const stored = await this.core.store.transact("r", (tx) => tx.getMeta<CalibrationRecord>(CALIBRATION_KEY));
    const current = this.core.engine.model_version();
    return stored && stored.rate > 0 && stored.model_version === current
      ? { rate: stored.rate, calibrated: true }
      : { rate: stored?.rate && stored.rate > 0 ? stored.rate : DEFAULT_RATE, calibrated: false };
  }

  /**
   * How long a run would take here: its cost (the server's own formula, from
   * the engine) over the calibrated rate times the workers that can share it.
   * Cheap: one engine call and one store read; nothing is benchmarked.
   */
  async estimate(planId: number, body: Partial<CreateRun>): Promise<RunEstimate> {
    const graph = await this.core.store.transact("r", async (tx) => (await this.core.requirePlan(tx, planId)).graph);
    const converge = body.converge === true;
    // A converging run's cost is its ceiling, the most it can spend; what to
    // expect is a couple of times its minimum sample.
    const sized = JSON.stringify({ ...body, converge: false });
    const cost = this.core.call<RunCost>((engine) => engine.run_cost(graph, sized));
    const { rate, calibrated } = await this.rate();
    const workers = Math.max(1, Math.min(this.core.pool.size, cost.parallel_batches));
    const seconds = (cost.cost / (rate * workers)) * (converge ? CONVERGE_FACTOR : 1);
    const { cores, memoryGb } = this.core.hardware;
    return {
      seconds,
      workers,
      constrained: (cores !== undefined && cores <= 2) || (memoryGb !== undefined && memoryGb <= 2),
      calibrated,
    };
  }

  /**
   * Measure this device once and store the rate. Runs on the pool, so it is
   * never on the page's thread; concurrent calls share one measurement.
   */
  calibrate(force = false): Promise<CalibrationRecord | undefined> {
    this.calibrating ??= this.measure(force).finally(() => {
      this.calibrating = undefined;
    });
    return this.calibrating;
  }

  private async measure(force: boolean): Promise<CalibrationRecord | undefined> {
    const version = this.core.engine.model_version();
    if (!force) {
      const stored = await this.core.store.transact("r", (tx) => tx.getMeta<CalibrationRecord>(CALIBRATION_KEY));
      if (stored && stored.model_version === version && stored.rate > 0) return stored;
    }
    const { engine } = this.core;
    const library = guard(() => engine.library_seed());
    const profile = (JSON.parse(library) as { return_profiles: { id: number }[] }).return_profiles[0].id;
    const graph = guard(() =>
      engine.setup_plan(JSON.stringify(calibrationAnswers(profile)), library, 1, "2026-01-01 00:00:00"),
    );
    const { json } = this.core.call<{ json: string }>((e) => e.snapshot(graph, library));
    const settings = JSON.stringify({ iterations: 200, batch_size: 100, parallel_batches: 2, seed: 7 });
    const sample = await this.core.pool.calibrate(json, settings);
    if (!(sample.seconds > 0) || sample.iterations <= 0) return undefined;
    const cost = this.core.call<RunCost>((e) => e.run_cost(graph, JSON.stringify({ iterations: sample.iterations })));
    const record: CalibrationRecord = {
      rate: cost.cost / sample.seconds,
      measured_at: this.core.nowText(),
      model_version: version,
    };
    await this.core.store.transact("rw", (tx) => tx.putMeta(CALIBRATION_KEY, record));
    return record;
  }

  // ── what the runtime reads ───────────────────────────────────────────────

  async planMeta(planId: number): Promise<LocalPlanMeta | undefined> {
    return this.core.store.transact("r", async (tx) => {
      const plan = await tx.getPlan(planId);
      return (
        plan && {
          id: plan.id,
          lastExportedAt: plan.last_exported_at,
          editsSinceExport: plan.edits_since_export,
          updatedAt: plan.updated_at,
        }
      );
    });
  }

  async snapshot(planId: number): Promise<{ json: string; hash: string; modelVersion: string }> {
    return this.core.store.transact("r", async (tx) => {
      const plan = await this.core.requirePlan(tx, planId);
      const library = await this.core.library(tx);
      const shot = this.core.call<{ json: string; hash: string }>((engine) => engine.snapshot(plan.graph, library));
      return { json: shot.json, hash: shot.hash, modelVersion: this.core.engine.model_version() };
    });
  }

  async markExported(ids: number[]): Promise<void> {
    const now = this.core.nowText();
    await this.core.store.transact("rw", async (tx) => {
      for (const id of ids) {
        const plan = await tx.getPlan(id);
        if (plan) await tx.putPlan({ ...plan, last_exported_at: now, edits_since_export: 0 });
      }
    });
  }
}

/** The typical plan calibration measures: a household with a salary, a 401(k) and spending, over 30 years. */
function calibrationAnswers(profileId: number): object {
  return {
    request_id: "calibration-plan",
    name: "Calibration",
    start_date: "2026-01-01",
    birth_date: "1985-06-15",
    duration_years: 30,
    retirement_age: 65,
    cash: 20_000,
    retirement_401k: 150_000,
    investments: 40_000,
    stock_percent: 70,
    cash_profile_id: profileId,
    stock_profile_id: profileId,
    bond_profile_id: profileId,
    investment_tax_status: "Taxable",
    annual_income: 120_000,
    retirement_401k_contribution_percent: 10,
    annual_spending: 60_000,
    retirement_spending: 55_000,
    inflation_profile_id: null,
    tax_config_id: null,
    fund_from_investments: true,
    assumptions_confirmed: true,
  };
}

export { badRequest };
