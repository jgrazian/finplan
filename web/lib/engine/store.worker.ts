/**
 * The store worker: owns IndexedDB, the local backend, one WASM instance for
 * plan edits, reads and the run coordinator, and the pool of compute workers
 * (spec 19, "Architecture").
 *
 * Topology:
 *
 * ```
 * page ──RPC── store worker ──postMessage── compute worker × N (≤ 8, lazily)
 *                 ├─ IndexedDB `finplan`
 *                 └─ BroadcastChannel `finplan-local` ── other tabs' store workers
 * ```
 *
 * The compute workers are *nested* dedicated workers, created from here.
 * Chrome, Edge and Firefox have always allowed that, Safari from 15; where
 * `Worker` is missing in a worker (older Safari) the pool falls back to running
 * batches in this worker, one at a time: slower, and edits wait behind a long
 * run, but correct. The page never makes compute workers itself, so it never
 * needs MessagePorts passed around, and a page that closes takes its whole
 * tree with it.
 *
 * If IndexedDB cannot be opened (private modes that refuse it, a locked
 * profile) or the engine cannot load, the worker says `failed` and the page
 * leaves local mode unregistered.
 */
import init, * as wasm from "./pkg/finplan_wasm.js";
import { createLocalEngine } from "./backend.ts";
import type { Engine } from "./engine.ts";
import { IdbStore } from "./idb.ts";
import { openChangeChannel } from "./locks.ts";
import { type ComputePool, InProcessWorker, type WorkerLike, WorkerPool, poolSize } from "./pool.ts";
import { type Port, type RpcResponse, serveRpc } from "./rpc.ts";

interface WorkerScope extends Port {
  postMessage(message: unknown): void;
}
const scope = self as unknown as WorkerScope;
const post = (message: RpcResponse) => scope.postMessage(message);

/** The pool for this browser: nested workers where they exist, in-process where not. */
function makePool(engine: Engine, cores: number | undefined): ComputePool {
  const size = poolSize(cores);
  if (typeof Worker === "undefined") {
    return new WorkerPool(() => new InProcessWorker(engine), 1);
  }
  return new WorkerPool(
    // A `Worker`'s handlers take `MessageEvent`s, the pool's `{data}`: the same thing to it.
    () =>
      new Worker(new URL("./compute.worker.ts", import.meta.url), { type: "module" }) as unknown as WorkerLike,
    size,
  );
}

async function start(): Promise<void> {
  await init({ module_or_path: new URL("./pkg/finplan_wasm_bg.wasm", import.meta.url) });
  const engine = wasm as unknown as Engine;
  const store = await IdbStore.open();
  const nav = (globalThis as { navigator?: { hardwareConcurrency?: number; deviceMemory?: number } }).navigator;
  const channel = openChangeChannel(globalThis.crypto.randomUUID());
  const local = createLocalEngine({
    engine,
    store,
    pool: makePool(engine, nav?.hardwareConcurrency),
    channel,
    hardware: { cores: nav?.hardwareConcurrency, memoryGb: nav?.deviceMemory },
  });
  serveRpc(scope, { api: local.api, runtime: local.runtime });
  channel.onExternal((plan) => post({ type: "event", name: "external-change", plan }));
  post({ type: "ready" });
  // Failing leftover runs and measuring the device wait until the page is being served.
  void local.start().catch(() => undefined);
}

start().catch((error: unknown) => {
  post({ type: "failed", message: error instanceof Error ? error.message : String(error) });
});
