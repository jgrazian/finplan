/**
 * The local backend: `PlanApi` over the browser's own store and engine (spec
 * 19). Everything the screens call on a plan in this browser lands here, in the
 * store worker (`store.worker.ts`), reached through the typed RPC of `rpc.ts`.
 *
 * `createLocalEngine` assembles the groups over one `Core` (engine, store,
 * compute pool, clock). It is written against interfaces, so the tests run it
 * over the in-memory store and an in-process pool, with `fetch` stubbed to throw
 * to show that nothing here reaches the network.
 *
 * Semantics follow `remoteApi`: the same generated types come back, the same
 * refusals (status, code, message, body) are thrown as `LocalError`, which the
 * main-thread client rethrows as `ApiError`.
 */
import type { PlanApi } from "../api/plan.ts";
import type { CreateRun, Run } from "../api/types.ts";
import type { LocalPlanMeta, RunEstimate } from "../local/runtime.ts";
import { AnalysisJobs } from "./analysis.ts";
import { Core, type CoreDeps } from "./core.ts";
import { accountsGroup, assetsGroup, eventsGroup, expressionsGroup, historyPresets, parametersGroup } from "./edits.ts";
import { type LocalReviewApi, reviewGroup } from "./review.ts";
import { Runs } from "./runs.ts";
import {
  archivesGroup,
  inflationProfilesGroup,
  returnProfilesGroup,
  scenariosGroup,
  taxConfigsGroup,
} from "./scenarios.ts";

/** The runtime half the worker answers; the main-thread client adds what only a window can (storage, localStorage, the other tabs). */
export interface EngineRuntime {
  estimateRun(scenarioId: number, body: Partial<CreateRun>): Promise<RunEstimate>;
  planMeta(scenarioId: number): Promise<LocalPlanMeta | undefined>;
  markExported(scenarioIds: number[]): Promise<void>;
  saveServerRun(
    scenarioId: number,
    run: { settings: Partial<CreateRun>; inputHash: string; modelVersion: string; results: unknown },
  ): Promise<Run>;
  snapshot(scenarioId: number): Promise<{ json: string; hash: string; modelVersion: string }>;
  /** How many plans the store holds: with a `localStorage` marker, how a cleared store is told. */
  planCount(): Promise<number>;
  /** Keep the browser's answer to "persist this site's storage" in `meta`. */
  recordPersistence(answer: string): Promise<void>;
  review: LocalReviewApi;
}

export interface LocalEngine {
  api: PlanApi;
  runtime: EngineRuntime;
  /**
   * Housekeeping after the store is open: fail runs a closed page left
   * half-done, and measure this device in the background when that has not been
   * done. Resolves when both are finished (a test waits; the worker does not).
   */
  start(): Promise<void>;
  /** Measure the device now, whether or not it has been measured. */
  calibrate(): Promise<number | undefined>;
  /** Stop everything: terminates the compute pool. */
  dispose(): void;
}

const PERSISTENCE_KEY = "persistence";

export function createLocalEngine(deps: CoreDeps): LocalEngine {
  const core = new Core(deps);
  const runs = new Runs(core);
  const jobs = new AnalysisJobs(core);
  const lifecycle = {
    forgetPlan(planId: number) {
      runs.forgetPlan(planId);
      jobs.forgetPlan(planId);
    },
  };

  const api: PlanApi = {
    scenarios: scenariosGroup(core, lifecycle),
    assets: assetsGroup(core),
    accounts: accountsGroup(core),
    events: eventsGroup(core),
    parameters: parametersGroup(core),
    expressions: expressionsGroup(core),
    returnProfiles: returnProfilesGroup(core),
    inflationProfiles: inflationProfilesGroup(core),
    historyPresets: () => historyPresets(core),
    taxConfigs: taxConfigsGroup(core),
    analysis: jobs.analysis,
    whatIf: jobs.whatIf,
    runs: runs.api,
    archives: archivesGroup(core),
  };

  const runtime: EngineRuntime = {
    estimateRun: (id, body) => runs.estimate(id, body),
    planMeta: (id) => runs.planMeta(id),
    markExported: (ids) => runs.markExported(ids),
    saveServerRun: (id, run) => runs.saveServerRun(id, run),
    snapshot: (id) => runs.snapshot(id),
    planCount: () => core.store.transact("r", async (tx) => (await tx.listPlans()).length),
    recordPersistence: (answer) =>
      core.store.transact("rw", (tx) => tx.putMeta(PERSISTENCE_KEY, { answer, at: core.nowText() })),
    review: reviewGroup(core),
  };

  return {
    api,
    runtime,
    async start() {
      await runs.recoverInterrupted();
      await runs.calibrate().catch(() => undefined);
    },
    calibrate: async () => (await runs.calibrate(true))?.rate,
    dispose() {
      core.pool.terminate();
      core.store.close();
    },
  };
}
