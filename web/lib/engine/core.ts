/**
 * What every group of the local backend shares: the engine, the store, the
 * compute pool, the clock, and the few operations they are all built from
 * (read a plan, edit a plan atomically, announce a change).
 *
 * The rules this file keeps, which the groups rely on:
 *
 *  - **An edit is one transaction.** `editPlan` reads the plan, applies the edit
 *    in WASM and writes the new graph inside one store transaction, under the
 *    plan's Web Lock. The engine refuses an edit by throwing, which aborts the
 *    transaction, so a failed edit writes nothing.
 *  - **Reads see one view.** A read is a read-only transaction.
 *  - **The library is the device's.** It is created on first write (from the
 *    engine's seed); until then a read uses the seed without storing it.
 *  - **Other tabs hear about writes** (`announce`) once they have committed.
 */
import type { Engine } from "./engine.ts";
import { type LocalError, badRequest, conflict, guard, notFound } from "./errors.ts";
import { type ChangeChannel, planLockName, withLock } from "./locks.ts";
import type { ComputePool } from "./pool.ts";
import type { PlanRecord, PlanStore, StoreTx } from "./store.ts";

/** What the device reports about itself, for run estimates. */
export interface Hardware {
  cores?: number;
  /** `navigator.deviceMemory`, in GB, where the browser says. */
  memoryGb?: number;
}

export interface CoreDeps {
  engine: Engine;
  store: PlanStore;
  pool: ComputePool;
  channel?: ChangeChannel;
  hardware?: Hardware;
  /** The clock; tests pin it. */
  now?: () => Date;
  /** Uniform in [0, 1); the run seed is drawn from it. Defaults to `crypto`. */
  random?: () => number;
}

/** SQLite's `datetime('now')` form, UTC: `2026-10-03 14:05:09`, what the plan rows use. */
export function sqliteTime(date: Date): string {
  return date.toISOString().slice(0, 19).replace("T", " ");
}

/** The slug of a local plan: `l<id>`. */
export const localSlug = (id: number) => `l${id}`;

/** What a plan's scenario body carries beyond its graph. */
export interface ScenarioExtras {
  slug: string;
  status: "active";
  last_run_at: string | null;
  last_success_rate: number | null;
}

export class Core {
  readonly engine: Engine;
  readonly store: PlanStore;
  readonly pool: ComputePool;
  readonly hardware: Hardware;
  private channel: ChangeChannel | undefined;
  private clock: () => Date;
  private rand: () => number;

  constructor(deps: CoreDeps) {
    this.engine = deps.engine;
    this.store = deps.store;
    this.pool = deps.pool;
    this.channel = deps.channel;
    this.hardware = deps.hardware ?? {};
    this.clock = deps.now ?? (() => new Date());
    this.rand = deps.random ?? (() => globalThis.crypto.getRandomValues(new Uint32Array(1))[0] / 2 ** 32);
  }

  now(): Date {
    return this.clock();
  }

  nowText(): string {
    return sqliteTime(this.clock());
  }

  /** A run seed: whole, in 0..2^53-1, drawn from 53 random bits. */
  seed(): number {
    const high = Math.floor(this.rand() * 2 ** 21);
    const low = Math.floor(this.rand() * 2 ** 32);
    return high * 2 ** 32 + low;
  }

  announce(plan: number | null): void {
    this.channel?.announce(plan);
  }

  // ── engine calls, with errors as `LocalError` ────────────────────────────

  call<T>(fn: (engine: Engine) => string): T {
    return JSON.parse(guard(() => fn(this.engine))) as T;
  }

  // ── the library ──────────────────────────────────────────────────────────

  /** The stored library, or the seed where none is stored yet. */
  async library(tx: StoreTx): Promise<string> {
    return (await tx.getLibrary()) ?? guard(() => this.engine.library_seed());
  }

  // ── plans ────────────────────────────────────────────────────────────────

  async requirePlan(tx: StoreTx, id: number): Promise<PlanRecord> {
    const plan = Number.isInteger(id) ? await tx.getPlan(id) : undefined;
    if (!plan) throw notFound("scenario");
    return plan;
  }

  /** Plan names are unique on a device, as on the server (`UNIQUE (user_id, name)`). */
  async assertNameFree(tx: StoreTx, name: string, except?: number): Promise<void> {
    const plans = await tx.listPlans();
    if (plans.some((plan) => plan.name === name && plan.id !== except)) {
      throw conflict("a scenario with that name already exists");
    }
  }

  /** The scenario body's extras: its slug, and its latest successful run's figures. */
  async extras(tx: StoreTx, planId: number): Promise<ScenarioExtras> {
    const runs = await tx.listRuns(planId);
    const latest = runs
      .filter((run) => run.status === "succeeded" && run.finished_at)
      .sort((a, b) => (b.finished_at ?? "").localeCompare(a.finished_at ?? "") || b.id - a.id)[0];
    return {
      slug: localSlug(planId),
      status: "active",
      last_run_at: latest?.finished_at ?? null,
      last_success_rate: latest?.success_rate ?? null,
    };
  }

  /** Answer `query` about plan `planId` (a `ReadQuery` object). */
  async read<T>(planId: number, query: object): Promise<T> {
    return this.store.transact("r", (tx) => this.readIn<T>(tx, planId, query));
  }

  /** [`read`], inside a transaction already open. */
  async readIn<T>(tx: StoreTx, planId: number, query: object): Promise<T> {
    const plan = await this.requirePlan(tx, planId);
    const library = await this.library(tx);
    const q = await this.withExtras(tx, planId, query);
    return this.call<T>((engine) => engine.read(plan.graph, library, JSON.stringify(q)));
  }

  /** A `scenario` query carries the extras the graph lacks. */
  private async withExtras(tx: StoreTx, planId: number, query: object): Promise<object> {
    if ((query as { query?: string }).query !== "scenario") return query;
    return { ...query, extras: await this.extras(tx, planId) };
  }

  /**
   * One plan write: apply `op` (an `EditOp` object) to plan `planId` and store
   * the result, atomically, then answer with `then`'s read of the new plan.
   * `then` is given the outcome's id (the row a `create_*` made) and answers a
   * `ReadQuery`, or nothing.
   */
  async editPlan<T = undefined>(
    planId: number,
    op: object,
    then?: (createdId: number | null) => object | undefined,
  ): Promise<{ id: number | null; read: T }> {
    const result = await withLock(planLockName(planId), () =>
      this.store.transact("rw", async (tx) => {
        const plan = await this.requirePlan(tx, planId);
        const library = await this.library(tx);
        const now = this.nowText();
        const out = this.call<{ graph: { scenario: { name: string } }; outcome: { id: number | null } }>(
          (engine) => engine.apply_edit(plan.graph, library, JSON.stringify(op), now),
        );
        const name = out.graph.scenario.name;
        // A rename must stay unique among the device's plans.
        if (name !== plan.name) await this.assertNameFree(tx, name, planId);
        const graph = JSON.stringify(out.graph);
        await tx.putPlan({
          ...plan,
          name,
          graph,
          updated_at: now,
          edits_since_export: plan.edits_since_export + 1,
        });
        const query = then?.(out.outcome.id);
        let read: unknown;
        if (query) {
          const q = await this.withExtras(tx, planId, query);
          read = this.call((engine) => engine.read(graph, library, JSON.stringify(q)));
        }
        return { id: out.outcome.id, read: read as T };
      }),
    );
    this.announce(planId);
    return result;
  }

  /** Every plan's graph, as the `ScenarioGraph[]` JSON the library calls take. */
  async graphsJson(tx: StoreTx): Promise<{ plans: PlanRecord[]; json: string }> {
    const plans = await tx.listPlans();
    return { plans, json: `[${plans.map((plan) => plan.graph).join(",")}]` };
  }

  // ── small checks ─────────────────────────────────────────────────────────

  /** A `Partial` body the screens send, as a plain object. */
  object(value: unknown, what: string): Record<string, unknown> {
    if (value === undefined || value === null) return {};
    if (typeof value !== "object" || Array.isArray(value)) throw badRequest(`${what} must be an object`);
    return value as Record<string, unknown>;
  }
}

export type { LocalError };
