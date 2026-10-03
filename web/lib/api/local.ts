/**
 * The local home of `PlanApi`: plans kept in this browser (spec 19).
 *
 * This module is only the seam. The store worker and the WASM engine are a
 * `PlanApi` of their own, loaded lazily and handed over with
 * `setLocalBackend`; every call here forwards to it. Until one is registered —
 * local mode off, the worker still loading, a browser without IndexedDB — a call
 * is refused in words a screen can show, rather than falling back to anything.
 *
 * It must never reach `fetch`, `http` or `remoteApi`. "In local mode nothing
 * carrying plan data leaves the browser" is only checkable if this file is the
 * whole of what a local call can do, so it imports types and nothing else.
 */
import type { PlanApi } from "./plan.ts";

/**
 * Raised for any local call made with no backend registered.
 *
 * Shaped like `ApiError` (`status`, `code`, `isUnauthorized`) so the code that
 * reads those off a failed call needs no second branch, but not a subclass:
 * `ApiError` lives beside `fetch` in `http.ts`, which this file may not import.
 */
export class LocalUnavailableError extends Error {
  readonly status = 503;
  readonly code = "local_unavailable";
  readonly isUnauthorized = false;

  constructor(message = "Local plans are not available in this browser.") {
    super(message);
    this.name = "LocalUnavailableError";
  }
}

let backend: PlanApi | undefined;

/**
 * Registers the implementation local calls go to; `undefined` removes it. The
 * store worker's client calls this once it can answer, so a screen opened
 * before that gets `LocalUnavailableError` rather than a hang.
 */
export function setLocalBackend(next: PlanApi | undefined): void {
  backend = next;
}

/** Whether a backend is registered, for screens that offer local plans only when they can work. */
export function hasLocalBackend(): boolean {
  return backend !== undefined;
}

function current(): PlanApi {
  if (!backend) throw new LocalUnavailableError();
  return backend;
}

/**
 * A group whose every method looks its backend up when called, not when the
 * group is built: `localApi` is a module constant that exists before the
 * backend does, and a backend registered later (or replaced in a test) must be
 * the one that answers. A forwarded call is always a promise, so a missing
 * backend rejects instead of throwing out of a caller that expects one.
 */
function forward<G extends object>(pick: (api: PlanApi) => G): G {
  return new Proxy({} as G, {
    get(_target, name) {
      // `then` would make the group look like a promise to an `await`.
      if (typeof name === "symbol" || name === "then") return undefined;
      return async (...args: unknown[]) => {
        // A backend may be built ahead of the interface, so a whole group can be missing too.
        const group = pick(current()) as Record<string, unknown> | undefined;
        const method = group?.[name];
        if (typeof method !== "function") {
          throw new LocalUnavailableError(`The local store does not support "${name}" yet.`);
        }
        return method.apply(group, args);
      };
    },
  });
}

export const localApi: PlanApi = {
  scenarios: forward((api) => api.scenarios),
  assets: forward((api) => api.assets),
  accounts: forward((api) => api.accounts),
  events: forward((api) => api.events),
  parameters: forward((api) => api.parameters),
  expressions: forward((api) => api.expressions),
  returnProfiles: forward((api) => api.returnProfiles),
  inflationProfiles: forward((api) => api.inflationProfiles),
  historyPresets: async () => current().historyPresets(),
  taxConfigs: forward((api) => api.taxConfigs),
  analysis: forward((api) => api.analysis),
  whatIf: forward((api) => api.whatIf),
  runs: forward((api) => api.runs),
  archives: forward((api) => api.archives),
};
