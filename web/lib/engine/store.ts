/**
 * The local store (spec 19, "Local store"): what a browser keeps for a plan.
 *
 * `PlanStore` is the interface the backend is written against; `idb.ts` is the
 * IndexedDB implementation the store worker uses, and `MemoryStore` here is
 * the in-memory one for tests. Both are transactional: `transact` runs a
 * function over one consistent view and either commits everything it wrote or,
 * when it throws, nothing. That is how "a failed edit writes nothing" holds: the
 * edit runs inside the transaction, between the read of the plan and its write.
 *
 * Ids (`plan`, `run`) are per-store counters kept in `meta`, so they are never
 * reused after a delete. Records are plain structured-cloneable objects.
 *
 * One deviation from the spec's table, for speed: a run's `results` (the
 * `RunResults` JSON, ~8 MB for a default plan) live in their own object store,
 * so listing a plan's runs or reading a run's metadata never loads them.
 */
import type { CreateRun } from "../api/generated/CreateRun.ts";

/** Counters in `meta`; a new id is `previous + 1`. */
export type Counter = "plan" | "run";

export interface PlanRecord {
  id: number;
  name: string;
  /** `ScenarioGraph` JSON, as the engine returned it. Opaque here. */
  graph: string;
  created_at: string;
  updated_at: string;
  /** When the plan was last exported or moved as a file; null if never. */
  last_exported_at: string | null;
  /** Saved edits since `last_exported_at` (since creation, when never exported). */
  edits_since_export: number;
}

export type RunStatus = "queued" | "running" | "succeeded" | "failed" | "canceled";

export interface RunRecord {
  id: number;
  plan_id: number;
  status: RunStatus;
  /** The settings as run, defaults filled in. The seed is always set. */
  settings: CreateRun;
  /** Fixed runs: the count. Converging runs: the minimum sample. */
  iterations: number;
  max_iterations: number | null;
  completed_iterations: number;
  seed: number | null;
  input_hash: string | null;
  model_version: string | null;
  /** Where it was computed: this device's engine, or the server (offload). */
  engine: "wasm" | "server";
  error_message: string | null;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  /** The plan's snapshot JSON the run was made from; null when it was not kept. */
  snapshot: string | null;
  /** The run's success rate once it has succeeded, for the plan list. */
  success_rate: number | null;
}

/** Everything a transaction can read and write. */
export interface StoreTx {
  /** The next id of `counter`; the counter advances with the transaction. */
  nextId(counter: Counter): Promise<number>;

  getPlan(id: number): Promise<PlanRecord | undefined>;
  listPlans(): Promise<PlanRecord[]>;
  putPlan(plan: PlanRecord): Promise<void>;
  deletePlan(id: number): Promise<void>;

  /** The `Library` JSON, or undefined before the first plan is made. */
  getLibrary(): Promise<string | undefined>;
  putLibrary(json: string): Promise<void>;

  getRun(id: number): Promise<RunRecord | undefined>;
  /** A plan's runs, newest (highest id) first. */
  listRuns(planId: number): Promise<RunRecord[]>;
  putRun(run: RunRecord): Promise<void>;
  /** Removes the run and its results. */
  deleteRun(id: number): Promise<void>;
  getResults(runId: number): Promise<string | undefined>;
  putResults(runId: number, json: string): Promise<void>;

  getMeta<T>(key: string): Promise<T | undefined>;
  putMeta(key: string, value: unknown): Promise<void>;
  deleteMeta(key: string): Promise<void>;
}

export interface PlanStore {
  /**
   * Run `fn` over one view of the store. A `"rw"` transaction commits what it
   * wrote when `fn` resolves and discards it when `fn` throws. `fn` must not
   * wait on anything but the store's own calls: an IndexedDB transaction closes
   * when the event loop turns.
   */
  transact<T>(mode: "r" | "rw", fn: (tx: StoreTx) => Promise<T>): Promise<T>;
  close(): void;
}

/** The newest runs kept per plan; older ones are removed when a run is made. */
export const KEEP_RUNS_PER_PLAN = 5;

// ── in-memory implementation ────────────────────────────────────────────────

interface Tables {
  plans: Map<number, PlanRecord>;
  runs: Map<number, RunRecord>;
  results: Map<number, string>;
  library: string | undefined;
  meta: Map<string, unknown>;
}

function copyTables(tables: Tables): Tables {
  return {
    plans: new Map(tables.plans),
    runs: new Map(tables.runs),
    results: new Map(tables.results),
    library: tables.library,
    meta: new Map(tables.meta),
  };
}

/** What a transaction hands out is its own copy, as a database's are. */
const clone = <T>(value: T): T => structuredClone(value);

class MemoryTx implements StoreTx {
  tables: Tables;

  constructor(tables: Tables) {
    this.tables = tables;
  }

  async nextId(counter: Counter): Promise<number> {
    const key = `counter:${counter}`;
    const next = ((this.tables.meta.get(key) as number | undefined) ?? 0) + 1;
    this.tables.meta.set(key, next);
    return next;
  }
  async getPlan(id: number) {
    const plan = this.tables.plans.get(id);
    return plan && clone(plan);
  }
  async listPlans() {
    return [...this.tables.plans.values()].map(clone);
  }
  async putPlan(plan: PlanRecord) {
    this.tables.plans.set(plan.id, clone(plan));
  }
  async deletePlan(id: number) {
    this.tables.plans.delete(id);
  }
  async getLibrary() {
    return this.tables.library;
  }
  async putLibrary(json: string) {
    this.tables.library = json;
  }
  async getRun(id: number) {
    const run = this.tables.runs.get(id);
    return run && clone(run);
  }
  async listRuns(planId: number) {
    return [...this.tables.runs.values()]
      .filter((run) => run.plan_id === planId)
      .sort((a, b) => b.id - a.id)
      .map(clone);
  }
  async putRun(run: RunRecord) {
    this.tables.runs.set(run.id, clone(run));
  }
  async deleteRun(id: number) {
    this.tables.runs.delete(id);
    this.tables.results.delete(id);
  }
  async getResults(runId: number) {
    return this.tables.results.get(runId);
  }
  async putResults(runId: number, json: string) {
    this.tables.results.set(runId, json);
  }
  async getMeta<T>(key: string) {
    const value = this.tables.meta.get(key);
    return value === undefined ? undefined : (clone(value) as T);
  }
  async putMeta(key: string, value: unknown) {
    this.tables.meta.set(key, clone(value));
  }
  async deleteMeta(key: string) {
    this.tables.meta.delete(key);
  }
}

/**
 * A store in memory, for tests and for a browser with no IndexedDB to fall back
 * on in a test harness. Transactions run one at a time, like IndexedDB's
 * conflicting ones, and a throwing one is rolled back.
 */
export class MemoryStore implements PlanStore {
  private tables: Tables = {
    plans: new Map(),
    runs: new Map(),
    results: new Map(),
    library: undefined,
    meta: new Map(),
  };
  private tail: Promise<unknown> = Promise.resolve();

  transact<T>(mode: "r" | "rw", fn: (tx: StoreTx) => Promise<T>): Promise<T> {
    const run = async () => {
      const working = copyTables(this.tables);
      const tx = new MemoryTx(working);
      const value = await fn(tx);
      if (mode === "rw") this.tables = working;
      return value;
    };
    const result = this.tail.then(run, run);
    this.tail = result.catch(() => undefined);
    return result;
  }

  close(): void {}
}
