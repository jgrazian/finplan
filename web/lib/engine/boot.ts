/**
 * Starting the local engine (spec 19): the one place that creates the store
 * worker and registers what it answers with.
 *
 * `startLocalEngine` is called when local mode is on, after the page has
 * painted (`startLocalEngineWhenIdle`): the `.wasm` is fetched by the worker,
 * not by the page, and not before. When the worker says it is ready, the
 * backend and the runtime are registered (`setLocalBackend`, `setLocalRuntime`)
 * and screens waiting on them (`useLocalRuntime`) pick them up. If the browser
 * has no workers or no IndexedDB, or the worker cannot start (IndexedDB
 * refused, the engine would not load), nothing is registered and the UI shows
 * "Local plans are not available in this browser".
 *
 * This file may import `ApiError` (it is on the page's side of the RPC, where
 * `http` lives); it never calls `fetch` itself, and neither does anything it
 * starts: the worker's only request is for the `.wasm` and its own scripts.
 */
import { ApiError } from "../api/http";
import { setLocalBackend, setLocalBackendStarter } from "../api/local";
import { localModeEnabled } from "../local/flag";
import { setLocalRuntime } from "../local/runtime";
import type { ErrorFields } from "./errors";
import { createRpcClient } from "./rpc";
import { createLocalClient } from "./runtime-client";

/** How long the worker has to say it is ready before the page gives up on it. */
const READY_TIMEOUT_MS = 30_000;

/** A refusal from the worker as the error a screen already handles. */
const toApiError = (fields: ErrorFields) =>
  new ApiError(fields.status, fields.code, fields.message, fields.body);

/** Whether this browser can host the local engine at all. */
export function localEngineSupported(): boolean {
  return typeof Worker !== "undefined" && typeof indexedDB !== "undefined";
}

let starting: Promise<boolean> | undefined;

// A screen's first local call, made before the worker is up, starts it and waits.
setLocalBackendStarter(() => (localModeEnabled() ? startLocalEngine() : Promise.resolve(false)));

/**
 * Starts the store worker and registers the local backend and runtime once it
 * answers. Resolves true when they are registered, false when this browser
 * cannot host them; never rejects. Calling it again returns the first attempt.
 */
export function startLocalEngine(): Promise<boolean> {
  starting ??= launch();
  return starting;
}

async function launch(): Promise<boolean> {
  if (!localEngineSupported()) return false;
  let worker: Worker | undefined;
  try {
    worker = new Worker(new URL("./store.worker.ts", import.meta.url), { type: "module" });
    const rpc = createRpcClient(worker, toApiError);
    const failed = new Promise<never>((_resolve, reject) => {
      worker?.addEventListener("error", (event) =>
        reject(new Error(event.message || "the local engine could not start")),
      );
    });
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_resolve, reject) => {
      timer = setTimeout(() => reject(new Error("the local engine took too long to start")), READY_TIMEOUT_MS);
    });
    try {
      await Promise.race([rpc.ready, failed, timeout]);
    } finally {
      clearTimeout(timer);
    }
    const { api, runtime } = createLocalClient(rpc);
    setLocalBackend(api);
    setLocalRuntime(runtime);
    return true;
  } catch {
    worker?.terminate();
    setLocalBackend(undefined);
    setLocalRuntime(undefined);
    return false;
  }
}

/**
 * [`startLocalEngine`] when local mode is on, once the browser is idle (or soon
 * after, where it has no idle callback), so the engine never competes with the
 * first paint. Returns a function that cancels the wait.
 */
export function startLocalEngineWhenIdle(): () => void {
  if (!localModeEnabled()) return () => undefined;
  const run = () => void startLocalEngine();
  const idle = (globalThis as { requestIdleCallback?: (cb: () => void, options?: { timeout: number }) => number })
    .requestIdleCallback;
  if (idle) {
    const handle = idle(run, { timeout: 2_000 });
    return () => (globalThis as { cancelIdleCallback?: (handle: number) => void }).cancelIdleCallback?.(handle);
  }
  const timer = setTimeout(run, 200);
  return () => clearTimeout(timer);
}
