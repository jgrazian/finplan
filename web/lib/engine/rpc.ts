/**
 * The typed RPC between the page and the store worker (spec 19, "Architecture").
 *
 * A call is `{id, path, args}` posted to the worker; the answer is `{id, ok,
 * value | error}` posted back. Both are structured-cloned, so arguments and
 * answers are plain data, and a refusal crosses as its fields
 * (`status`, `code`, `message`, `body`) and is re-hydrated on the page into the
 * error class the page wants (`ApiError`, in `client.ts`). `path` names a
 * function of the object the worker serves: `api.scenarios.list`,
 * `runtime.estimateRun`.
 *
 * An `AbortSignal` cannot be cloned. The client replaces one in the arguments
 * with a marker and sends `{type: "abort", id}` if it fires; the server hands
 * the function a signal of its own and aborts it then.
 *
 * Messages with no `id` are the worker talking unprompted: `ready` once it can
 * answer, `failed` if it never will (no IndexedDB, WASM refused to load) and
 * `event` for what other tabs changed.
 *
 * Nothing here knows about the engine, so a test drives both ends over a
 * `MessageChannel`, and neither end touches `fetch`.
 */
import { type ErrorFields, toLocalError } from "./errors.ts";

export type RpcRequest =
  | { type: "call"; id: number; path: string; args: unknown[] }
  | { type: "abort"; id: number };

export type RpcResponse =
  | { type: "result"; id: number; ok: true; value: unknown }
  | { type: "result"; id: number; ok: false; error: ErrorFields }
  | { type: "ready" }
  | { type: "failed"; message: string }
  | { type: "event"; name: "external-change"; plan: number | null };

/** What an `AbortSignal` argument is replaced with on the wire. */
const ABORT_MARKER = { __rpc: "abort-signal" } as const;
const isMarker = (value: unknown): boolean =>
  typeof value === "object" && value !== null && (value as { __rpc?: unknown }).__rpc === ABORT_MARKER.__rpc;

/** The two ends of a message channel: a `Worker`, a `MessagePort`, or a worker's own scope. */
export interface Port {
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
  removeEventListener?(type: "message", listener: (event: { data: unknown }) => void): void;
}

// ── the serving end (inside the worker) ──────────────────────────────────────

/** Finds `a.b.c` in `root`, with `this` bound to its parent, or undefined. */
function resolve(root: object, path: string): ((...args: unknown[]) => unknown) | undefined {
  let parent: unknown = root;
  let current: unknown = root;
  for (const part of path.split(".")) {
    if (part === "__proto__" || part === "constructor" || part === "prototype") return undefined;
    if (current === null || typeof current !== "object" && typeof current !== "function") return undefined;
    parent = current;
    current = (current as Record<string, unknown>)[part];
  }
  if (typeof current !== "function") return undefined;
  return (...args) => (current as (...a: unknown[]) => unknown).apply(parent, args);
}

/** Answer calls on `port` with the functions of `target`. Returns a function that stops listening. */
export function serveRpc(port: Port, target: object): () => void {
  const aborts = new Map<number, AbortController>();
  const listener = (event: { data: unknown }) => {
    const request = event.data as RpcRequest | undefined;
    if (!request || typeof request !== "object") return;
    if (request.type === "abort") {
      aborts.get(request.id)?.abort();
      return;
    }
    if (request.type !== "call") return;
    const { id, path } = request;
    const reply = (response: RpcResponse) => port.postMessage(response);
    const fn = resolve(target, path);
    if (!fn) {
      reply({ type: "result", id, ok: false, error: { status: 404, code: "not_found", message: `no such method: ${path}` } });
      return;
    }
    const controller = new AbortController();
    const args = request.args.map((arg) => (isMarker(arg) ? controller.signal : arg));
    aborts.set(id, controller);
    Promise.resolve()
      .then(() => fn(...args))
      .then(
        (value) => reply({ type: "result", id, ok: true, value }),
        (thrown: unknown) => reply({ type: "result", id, ok: false, error: toLocalError(thrown).fields() }),
      )
      .finally(() => aborts.delete(id));
  };
  port.addEventListener("message", listener);
  return () => port.removeEventListener?.("message", listener);
}

// ── the calling end (on the page) ────────────────────────────────────────────

export interface RpcClient {
  /** Call the function at `path` with `args`. */
  call(path: string, args: unknown[]): Promise<unknown>;
  /** An object whose nested functions call `prefix.<their path>` on the worker. */
  proxy<T extends object>(prefix: string): T;
  /** Resolves when the worker says it can answer; rejects if it says it never will. */
  ready: Promise<void>;
  /** Called for what other tabs changed; returns unsubscribe. */
  onExternalChange(listener: (plan: number | null) => void): () => void;
}

/**
 * `rehydrate` makes the error a failed call throws from its fields; the page
 * passes one that builds an `ApiError`, a test one that builds an `Error`.
 */
export function createRpcClient(port: Port, rehydrate: (fields: ErrorFields) => Error): RpcClient {
  let nextId = 1;
  const pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();
  const listeners = new Set<(plan: number | null) => void>();
  let settleReady: { resolve: () => void; reject: (error: Error) => void };
  const ready = new Promise<void>((resolve, reject) => {
    settleReady = { resolve, reject };
  });
  // A failure nobody awaits (no call was made yet) is not an unhandled rejection.
  ready.catch(() => undefined);

  port.addEventListener("message", (event) => {
    const message = event.data as RpcResponse | undefined;
    if (!message || typeof message !== "object") return;
    switch (message.type) {
      case "ready":
        settleReady.resolve();
        return;
      case "failed":
        settleReady.reject(new Error(message.message));
        return;
      case "event":
        for (const listener of [...listeners]) listener(message.plan);
        return;
      case "result": {
        const call = pending.get(message.id);
        if (!call) return;
        pending.delete(message.id);
        if (message.ok) call.resolve(message.value);
        else call.reject(rehydrate(message.error));
        return;
      }
    }
  });

  const call = (path: string, args: unknown[]): Promise<unknown> =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      let signal: AbortSignal | undefined;
      const wire = args.map((arg) => {
        if (typeof AbortSignal !== "undefined" && arg instanceof AbortSignal) {
          signal = arg;
          return ABORT_MARKER;
        }
        return arg;
      });
      pending.set(id, { resolve, reject });
      if (signal) {
        const onAbort = () => port.postMessage({ type: "abort", id } satisfies RpcRequest);
        if (signal.aborted) onAbort();
        else signal.addEventListener("abort", onAbort, { once: true });
      }
      port.postMessage({ type: "call", id, path, args: wire } satisfies RpcRequest);
    });

  const proxy = <T extends object>(prefix: string): T => {
    const make = (path: string): object =>
      new Proxy(() => undefined, {
        get(target, name) {
          // `then` would make a group look like a promise to an `await`.
          if (typeof name === "symbol" || name === "then") return undefined;
          // A method is still a function: `method.apply(group, args)` (what the
          // `localApi` seam does) must call it, not name a path called "apply".
          if (name === "apply" || name === "call" || name === "bind") return Reflect.get(target, name);
          return make(`${path}.${name}`);
        },
        apply(_target, _this, args: unknown[]) {
          return call(path, args);
        },
      });
    return make(prefix) as T;
  };

  return {
    call,
    proxy,
    ready,
    onExternalChange(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}
