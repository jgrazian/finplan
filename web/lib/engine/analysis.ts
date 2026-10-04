/**
 * Local analysis (spec 19, phase 4): sweeps, sensitivity rankings, goal seeks
 * and what-if stacks, run in a compute worker.
 *
 * The loops that decide what to simulate are `finplan_plan::analysis`, behind
 * the engine's `analysis_run`; a compute worker runs one analysis to completion
 * on its own thread, reporting progress as simulations finish, so the page and
 * the store worker stay free. Jobs are not persisted (like the server's, an id
 * outlives only the process that issued it); the newest sweep of each plan is
 * kept in `meta` so a reload finds its grid again, with the layout of graphs the
 * person arranged over it.
 */
import type { AnalysisPlan } from "../api/generated/AnalysisPlan.ts";
import type { AnalysisApi, WhatIfApi } from "../api/plan.ts";
import type {
  Analysis,
  AnalysisOutcome,
  AnalysisParameter,
  CachedSweep,
  Scenario,
  WhatIfOutcome,
  WhatIfStack,
} from "../api/types.ts";
import type { Core } from "./core.ts";
import { abortError, conflict, guard, notFound, toLocalError } from "./errors.ts";
import { planLockName, withLock } from "./locks.ts";
import type { AnalysisHandle } from "./pool.ts";
import type { PlanRecord } from "./store.ts";

/** How many finished analyses are remembered for `get` and `results`. */
const KEEP_JOBS = 20;

interface Job {
  id: number;
  planId: number;
  kind: string;
  status: "queued" | "running" | "succeeded" | "failed" | "canceled";
  completed: number;
  total: number;
  error: string | null;
  startedAt: number;
  elapsedMs: number | null;
  outcome: string | undefined;
  handle: AnalysisHandle | undefined;
}

const sweepKey = (planId: number) => `sweep:${planId}`;
const layoutKey = (planId: number) => `sweep-layout:${planId}`;
const stackKey = (planId: number) => `whatif:${planId}`;

export class AnalysisJobs {
  private core: Core;
  private jobs = new Map<number, Job>();
  private nextId = 1;

  constructor(core: Core) {
    this.core = core;
  }

  /** A plan is being deleted: stop its analyses. */
  forgetPlan(planId: number): void {
    for (const job of this.jobs.values()) {
      if (job.planId === planId && (job.status === "queued" || job.status === "running")) {
        job.handle?.cancel();
      }
    }
  }

  private view(job: Job): Analysis {
    return {
      id: job.id,
      scenario_id: job.planId,
      kind: job.kind,
      status: job.status,
      completed: job.completed,
      total: job.total,
      error_message: job.error,
      elapsed_ms: job.elapsedMs,
    };
  }

  private require(id: number): Job {
    const job = this.jobs.get(id);
    if (!job) throw notFound("analysis");
    return job;
  }

  private async load(planId: number): Promise<{ plan: PlanRecord; library: string }> {
    return this.core.store.transact("r", async (tx) => ({
      plan: await this.core.requirePlan(tx, planId),
      library: await this.core.library(tx),
    }));
  }

  private async start(planId: number, body: object): Promise<Analysis> {
    const { plan, library } = await this.load(planId);
    const bodyJson = JSON.stringify(body);
    // Refused here, as the server refuses at POST: the screen hears it at once.
    const costed = this.core.call<AnalysisPlan>((engine) => engine.analysis_plan(plan.graph, library, bodyJson));
    const job: Job = {
      id: this.nextId++,
      planId,
      kind: costed.kind,
      status: "running",
      completed: 0,
      total: costed.total,
      error: null,
      startedAt: Date.now(),
      elapsedMs: null,
      outcome: undefined,
      handle: undefined,
    };
    this.jobs.set(job.id, job);
    for (const old of [...this.jobs.keys()].slice(0, Math.max(0, this.jobs.size - KEEP_JOBS))) {
      if (this.jobs.get(old)?.status !== "running") this.jobs.delete(old);
    }
    const handle = this.core.pool.analysis(
      { graph: plan.graph, library, body: bodyJson, quick: false },
      (done, total) => {
        job.completed = done;
        job.total = total;
      },
    );
    job.handle = handle;
    handle.promise.then(
      async (outcome) => {
        job.outcome = outcome;
        job.elapsedMs = Date.now() - job.startedAt;
        job.completed = Math.max(job.completed, job.total);
        if (job.status === "canceled") return;
        job.status = "succeeded";
        if (job.kind === "sweep") await this.keepSweep(planId, outcome).catch(() => undefined);
      },
      (thrown: unknown) => {
        job.elapsedMs = Date.now() - job.startedAt;
        if (job.status === "canceled") return;
        job.status = "failed";
        job.error = toLocalError(thrown).message;
      },
    );
    return this.view(job);
  }

  /** Keep the newest sweep, minus its `kind` tag, as the cached grid. */
  private async keepSweep(planId: number, outcomeJson: string): Promise<void> {
    const results = JSON.parse(outcomeJson) as { kind?: string } & Record<string, unknown>;
    delete results.kind;
    await this.core.store.transact("rw", async (tx) => {
      // The plan may have been deleted while the sweep ran.
      if (!(await tx.getPlan(planId))) return;
      await tx.putMeta(sweepKey(planId), { created_at: this.core.nowText(), results });
    });
  }

  analysis: AnalysisApi = {
    parameters: (scenarioId) => this.core.read<AnalysisParameter[]>(scenarioId, { query: "analysis_parameters" }),

    start: (scenarioId, body) => this.start(scenarioId, body),

    get: async (id) => this.view(this.require(id)),

    cancel: async (id) => {
      const job = this.require(id);
      if (job.status === "queued" || job.status === "running") {
        job.status = "canceled";
        job.elapsedMs = Date.now() - job.startedAt;
        job.handle?.cancel();
      }
      return this.view(job);
    },

    results: async (id) => {
      const job = this.require(id);
      if (job.status !== "succeeded" || job.outcome === undefined) {
        throw conflict("the analysis has not finished");
      }
      return JSON.parse(job.outcome) as AnalysisOutcome;
    },

    cachedSweep: (scenarioId) =>
      this.core.store.transact("r", async (tx) => {
        await this.core.requirePlan(tx, scenarioId);
        const kept = await tx.getMeta<{ created_at: string; results: CachedSweep["results"] }>(sweepKey(scenarioId));
        if (!kept) return null;
        const layout = await tx.getMeta<unknown[]>(layoutKey(scenarioId));
        return {
          scenario_id: scenarioId,
          created_at: kept.created_at,
          results: kept.results,
          layout: layout ?? null,
        } satisfies CachedSweep;
      }),

    saveSweepLayout: (scenarioId, graphs) =>
      this.core.store.transact("rw", async (tx) => {
        await this.core.requirePlan(tx, scenarioId);
        await tx.putMeta(layoutKey(scenarioId), graphs);
      }),
  };

  whatIf: WhatIfApi = {
    get: (scenarioId) =>
      this.core.store.transact("r", async (tx) => {
        await this.core.requirePlan(tx, scenarioId);
        return (await tx.getMeta<WhatIfStack>(stackKey(scenarioId))) ?? { entries: [] };
      }),

    save: async (scenarioId, body) => {
      const stack = this.core.call<WhatIfStack>((engine) => engine.check_what_if_stack(JSON.stringify(body)));
      await this.core.store.transact("rw", async (tx) => {
        await this.core.requirePlan(tx, scenarioId);
        await tx.putMeta(stackKey(scenarioId), stack);
      });
    },

    apply: async (scenarioId, body) => {
      const bodyJson = JSON.stringify(body);
      const copy = typeof body.new_scenario_name === "string";
      const scenario = await withLock(planLockName(scenarioId), () =>
        this.core.store.transact("rw", async (tx) => {
          const plan = await this.core.requirePlan(tx, scenarioId);
          const library = await this.core.library(tx);
          const now = this.core.nowText();
          const id = copy ? await tx.nextId("plan") : scenarioId;
          const graph = guard(() => this.core.engine.apply_what_if(plan.graph, library, bodyJson, id, now));
          const name = (JSON.parse(graph) as { scenario: { name: string } }).scenario.name;
          if (copy) {
            await this.core.assertNameFree(tx, name);
            await tx.putPlan({
              id,
              name,
              graph,
              created_at: now,
              updated_at: now,
              last_exported_at: null,
              edits_since_export: 0,
            });
          } else {
            await tx.putPlan({ ...plan, graph, updated_at: now, edits_since_export: plan.edits_since_export + 1 });
            // The plan now says what the stack said; keeping the stack would apply it twice.
            await tx.deleteMeta(stackKey(scenarioId));
          }
          return this.core.readIn<Scenario>(tx, id, { query: "scenario" });
        }),
      );
      this.core.announce(copy ? null : scenarioId);
      return scenario;
    },

    quick: async (scenarioId, body, signal) => {
      const { plan, library } = await this.load(scenarioId);
      if (signal?.aborted) throw abortError();
      const handle = this.core.pool.analysis(
        { graph: plan.graph, library, body: JSON.stringify(body), quick: true },
        undefined,
      );
      const onAbort = () => handle.cancel();
      signal?.addEventListener("abort", onAbort, { once: true });
      try {
        return JSON.parse(await handle.promise) as WhatIfOutcome;
      } catch (thrown) {
        // The caller withdrew the question: an abort, as a fetch's would be.
        if (signal?.aborted) throw abortError();
        throw thrown;
      } finally {
        signal?.removeEventListener("abort", onAbort);
      }
    },
  };
}

