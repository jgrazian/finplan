/**
 * What the web UI needs from the local engine beyond `PlanApi` (spec 19).
 *
 * This file is the contract between two pieces of work. The UI (plan homes,
 * durability, run estimates, server offload) is written against `LocalRuntime`;
 * the local engine — store worker, IndexedDB, compute workers, WASM — implements
 * it and registers the implementation with `setLocalRuntime` once it can
 * answer, next to `setLocalBackend` for the `PlanApi` half. Until then
 * `getLocalRuntime()` is `undefined` and every caller must cope: the screens
 * simply leave out what depends on it.
 *
 * Like `lib/api/local.ts` this is only a holder. No implementation, no `fetch`,
 * no import of `http` or `remoteApi`: the UI may not reach plan data through
 * here by any route but the runtime's own methods.
 *
 * Notes for the implementer:
 *  - `scenarioId` is the local row id (`l12` is 12), the same number the
 *    `PlanApi` methods take.
 *  - `estimateRun` is called before every local run, so it must be cheap
 *    (compute from the calibration rate in `meta`; do not benchmark here).
 *    `calibrated: false` means the rate is still a default because calibration
 *    has not finished. `constrained` is the device report: two cores or fewer,
 *    or a low `navigator.deviceMemory`.
 *  - `snapshot` is `finplan_plan::snapshot`: the JSON text, its hash (the same
 *    `input_hash` the server computes) and `MODEL_VERSION`. Offload sends the
 *    JSON and the version, and passes the hash back to `saveServerRun` so the
 *    stored run's freshness works like any local run's.
 *  - `saveServerRun` stores `results` (the `RunResults` projection the server
 *    returned) as a run with `engine: "server"` and returns it as a `Run`, so
 *    the Results screen shows it like a run made here.
 *  - `storeWasCleared` is true when a `localStorage` marker says this browser
 *    had local plans but the store is now empty (the browser evicted it). The
 *    marker is the runtime's to write.
 *  - `onExternalChange` is fed by a `BroadcastChannel`: `scenarioId` is the plan
 *    that changed in another tab, or `null` when the set of plans changed.
 *    It must not fire for this tab's own writes.
 *
 * `LocalPlanMeta.updatedAt` and `lastExportedAt` are ISO 8601 or the
 * `YYYY-MM-DD HH:MM:SS` UTC form the plan rows use; the UI reads either.
 */
import type { CreateRun, Run } from "../api/types.ts";

export interface RunEstimate {
  /** Wall-clock seconds this run is expected to take on this device. */
  seconds: number;
  /** How many compute workers will share it. */
  workers: number;
  /** Two cores or fewer, or a low-memory hint: this device is a poor place for a big run. */
  constrained: boolean;
  /** False while the rate is a default because calibration has not finished. */
  calibrated: boolean;
}

export interface LocalPlanMeta {
  id: number;
  /** When the plan was last exported or moved as a file; null if never. */
  lastExportedAt: string | null;
  /** Saved edits since `lastExportedAt` (since creation, when never exported). */
  editsSinceExport: number;
  updatedAt: string;
}

export interface LocalRuntime {
  estimateRun(scenarioId: number, body: Partial<CreateRun>): Promise<RunEstimate>;
  planMeta(scenarioId: number): Promise<LocalPlanMeta | undefined>;
  markExported(scenarioIds: number[]): Promise<void>;
  /** Store results computed on the server (offload) as a local run with engine "server". */
  saveServerRun(
    scenarioId: number,
    run: {
      settings: Partial<CreateRun>;
      inputHash: string;
      modelVersion: string;
      results: unknown;
    },
  ): Promise<Run>;
  /** The plan's snapshot JSON (finplan_plan::snapshot), its hash and MODEL_VERSION, for offload. */
  snapshot(scenarioId: number): Promise<{ json: string; hash: string; modelVersion: string }>;
  storage: {
    /** `navigator.storage.persisted()`; undefined where the API does not exist. */
    persisted(): Promise<boolean | undefined>;
    /** `navigator.storage.persist()`; undefined where the API does not exist. */
    requestPersist(): Promise<boolean | undefined>;
  };
  /** True when this browser had local plans before (a localStorage marker) but the store is now empty. */
  storeWasCleared(): Promise<boolean>;
  /** Subscribe to changes made in other tabs (BroadcastChannel); returns unsubscribe. */
  onExternalChange(listener: (scenarioId: number | null) => void): () => void;
}

let runtime: LocalRuntime | undefined;
const listeners = new Set<() => void>();

/**
 * Registers the engine's runtime; `undefined` removes it (tests, local mode
 * torn down). Call it once the runtime can answer, as with `setLocalBackend`.
 */
export function setLocalRuntime(next: LocalRuntime | undefined): void {
  runtime = next;
  for (const listener of [...listeners]) listener();
}

/**
 * Called whenever a runtime is registered or removed, so a screen opened before
 * the engine finished loading picks it up (`useLocalRuntime`). Returns unsubscribe.
 */
export function subscribeLocalRuntime(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getLocalRuntime(): LocalRuntime | undefined {
  return runtime;
}
