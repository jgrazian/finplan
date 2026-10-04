/**
 * The IndexedDB `PlanStore`: database `finplan`, owned by the store worker.
 *
 * | store | key | value |
 * |---|---|---|
 * | `plans` | `id` | `PlanRecord` |
 * | `runs` | `id`, index `plan_id` | `RunRecord` (no results) |
 * | `run_results` | `id` | `{id, results}`, the `RunResults` JSON |
 * | `library` | `key` (`"main"`) | `{key, json}`, the one `Library` document |
 * | `meta` | `key` | `{key, value}`: schema version, counters, calibration, persistence, per-plan stacks and dismissals |
 *
 * A `transact` is one IndexedDB transaction over every store. IndexedDB closes
 * a transaction as soon as the event loop turns with no request pending, so a
 * function given to `transact` must await only this module's own calls (each
 * resolves in the request's callback, inside the transaction); the backend
 * calls the WASM engine, which is synchronous, between them.
 */
import type { Counter, PlanRecord, PlanStore, RunRecord, StoreTx } from "./store.ts";

export const DB_NAME = "finplan";
/** Bumped when a store's shape changes; `upgrade` migrates from the version before. */
export const SCHEMA_VERSION = 1;
const STORES = ["plans", "runs", "run_results", "library", "meta"] as const;
const LIBRARY_KEY = "main";
export const SCHEMA_META_KEY = "schema";

function upgrade(db: IDBDatabase, from: number): void {
  if (from < 1) {
    db.createObjectStore("plans", { keyPath: "id" });
    const runs = db.createObjectStore("runs", { keyPath: "id" });
    runs.createIndex("plan_id", "plan_id");
    db.createObjectStore("run_results", { keyPath: "id" });
    db.createObjectStore("library", { keyPath: "key" });
    db.createObjectStore("meta", { keyPath: "key" });
  }
}

/** A request's result, as a promise that resolves inside its transaction. */
function request<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error("IndexedDB request failed"));
  });
}

class IdbTx implements StoreTx {
  tx: IDBTransaction;

  constructor(tx: IDBTransaction) {
    this.tx = tx;
  }

  private store(name: (typeof STORES)[number]): IDBObjectStore {
    return this.tx.objectStore(name);
  }

  async nextId(counter: Counter): Promise<number> {
    const key = `counter:${counter}`;
    const next = ((await this.getMeta<number>(key)) ?? 0) + 1;
    await this.putMeta(key, next);
    return next;
  }
  async getPlan(id: number) {
    return (await request(this.store("plans").get(id))) as PlanRecord | undefined;
  }
  async listPlans() {
    return (await request(this.store("plans").getAll())) as PlanRecord[];
  }
  async putPlan(plan: PlanRecord) {
    await request(this.store("plans").put(plan));
  }
  async deletePlan(id: number) {
    await request(this.store("plans").delete(id));
  }
  async getLibrary() {
    const row = (await request(this.store("library").get(LIBRARY_KEY))) as
      | { key: string; json: string }
      | undefined;
    return row?.json;
  }
  async putLibrary(json: string) {
    await request(this.store("library").put({ key: LIBRARY_KEY, json }));
  }
  async getRun(id: number) {
    return (await request(this.store("runs").get(id))) as RunRecord | undefined;
  }
  async listRuns(planId: number) {
    const rows = (await request(
      this.store("runs").index("plan_id").getAll(planId),
    )) as RunRecord[];
    return rows.sort((a, b) => b.id - a.id);
  }
  async putRun(run: RunRecord) {
    await request(this.store("runs").put(run));
  }
  async deleteRun(id: number) {
    await request(this.store("runs").delete(id));
    await request(this.store("run_results").delete(id));
  }
  async getResults(runId: number) {
    const row = (await request(this.store("run_results").get(runId))) as
      | { id: number; results: string }
      | undefined;
    return row?.results;
  }
  async putResults(runId: number, results: string) {
    await request(this.store("run_results").put({ id: runId, results }));
  }
  async getMeta<T>(key: string) {
    const row = (await request(this.store("meta").get(key))) as
      | { key: string; value: T }
      | undefined;
    return row?.value;
  }
  async putMeta(key: string, value: unknown) {
    await request(this.store("meta").put({ key, value }));
  }
  async deleteMeta(key: string) {
    await request(this.store("meta").delete(key));
  }
}

export class IdbStore implements PlanStore {
  private db: IDBDatabase;

  private constructor(db: IDBDatabase) {
    this.db = db;
    // Another tab upgrading the schema must not be blocked by this connection.
    db.onversionchange = () => db.close();
  }

  /** Opens (creating or upgrading) the database. Rejects where IndexedDB is unavailable or refused. */
  static open(factory: IDBFactory = indexedDB, name = DB_NAME): Promise<IdbStore> {
    return new Promise((resolve, reject) => {
      let opened: IDBOpenDBRequest;
      try {
        opened = factory.open(name, SCHEMA_VERSION);
      } catch (error) {
        reject(error);
        return;
      }
      opened.onupgradeneeded = (event) => upgrade(opened.result, event.oldVersion);
      opened.onsuccess = () => resolve(new IdbStore(opened.result));
      opened.onerror = () => reject(opened.error ?? new Error("IndexedDB could not be opened"));
      opened.onblocked = () => reject(new Error("IndexedDB is blocked by another tab"));
    });
  }

  transact<T>(mode: "r" | "rw", fn: (tx: StoreTx) => Promise<T>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const tx = this.db.transaction([...STORES], mode === "rw" ? "readwrite" : "readonly");
      let value: T;
      let failure: { error: unknown } | undefined;
      tx.oncomplete = () => (failure ? reject(failure.error) : resolve(value));
      tx.onabort = () =>
        reject(failure ? failure.error : (tx.error ?? new Error("The transaction was aborted")));
      // Called now, not a microtask later: the transaction is active only
      // until this task ends.
      fn(new IdbTx(tx)).then(
        (result) => {
          value = result;
        },
        (error: unknown) => {
          failure = { error };
          // Discards everything the function wrote.
          try {
            tx.abort();
          } catch {
            // Already finished: the abort handler or completion will settle it.
          }
        },
      );
    });
  }

  close(): void {
    this.db.close();
  }
}
