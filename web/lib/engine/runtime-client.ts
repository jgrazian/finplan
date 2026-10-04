/**
 * The page's side of the local engine: a `PlanApi` and a `LocalRuntime` that
 * call the store worker over RPC, plus the three things only a window can do.
 *
 *  - **Storage.** `navigator.storage.persisted()` / `persist()` (a worker's
 *    `persist()` cannot show the browser's prompt). The answer is also recorded
 *    in the store's `meta`.
 *  - **The cleared-store marker.** A `localStorage` key says "this browser has
 *    had local plans". It is set when a plan is first saved here and cleared
 *    when the last plan is deleted on purpose, so an empty store with the marker
 *    set can only mean the browser evicted the data (`storeWasCleared`).
 *  - **Other tabs.** The worker forwards `BroadcastChannel` messages; a listener
 *    is called for a plan another tab changed, never for this tab's own writes.
 *
 * No `fetch`, no `http`: errors are re-hydrated by the function `boot.ts` passes
 * in, which is where `ApiError` is imported.
 */
import type { PlanApi } from "../api/plan.ts";
import type { LocalRuntime } from "../local/runtime.ts";
import type { LocalReviewApi } from "./review.ts";
import type { EngineRuntime } from "./backend.ts";
import type { RpcClient } from "./rpc.ts";

/** The `localStorage` key of the cleared-store marker. */
export const HAD_PLANS_KEY = "finplan.hadLocalPlans";

interface KeyValue {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

interface StorageManagerLike {
  persisted?: () => Promise<boolean>;
  persist?: () => Promise<boolean>;
}

export interface BrowserEnv {
  storage?: StorageManagerLike;
  localStorage?: KeyValue;
}

/** What this window offers, or nothing where a thing does not exist or throws. */
export function browserEnv(): BrowserEnv {
  let local: KeyValue | undefined;
  try {
    local = globalThis.localStorage;
  } catch {
    // Blocked site data.
  }
  return { storage: (globalThis as { navigator?: { storage?: StorageManagerLike } }).navigator?.storage, localStorage: local };
}

function safely<T>(fn: () => T): T | undefined {
  try {
    return fn();
  } catch {
    return undefined;
  }
}

/**
 * `group` with some of its members replaced. The groups are proxies that answer
 * any property, so the replacements are looked up first and nothing is copied.
 */
function withOverrides<T extends object>(group: T, overrides: Partial<T>): T {
  return new Proxy(group, {
    get(target, name, receiver) {
      if (typeof name === "string" && Object.hasOwn(overrides, name)) {
        return (overrides as Record<string, unknown>)[name];
      }
      return Reflect.get(target, name, receiver);
    },
  });
}

export interface LocalClient {
  api: PlanApi;
  runtime: LocalRuntime;
}

export function createLocalClient(rpc: RpcClient, env: BrowserEnv = browserEnv()): LocalClient {
  const raw = rpc.proxy<PlanApi>("api");
  const remote = <K extends keyof EngineRuntime>(name: K) =>
    (...args: unknown[]) => rpc.call(`runtime.${name}`, args);

  const mark = () => safely(() => env.localStorage?.setItem(HAD_PLANS_KEY, "1"));
  const unmark = () => safely(() => env.localStorage?.removeItem(HAD_PLANS_KEY));
  const planCount = () => remote("planCount")() as Promise<number>;

  /** A plan was saved here: from now on an empty store is a sign something cleared it. */
  const marking =
    <A extends unknown[], T>(fn: (...args: A) => Promise<T>) =>
    async (...args: A): Promise<T> => {
      const result = await fn(...args);
      mark();
      return result;
    };

  const api = withOverrides(raw, {
    scenarios: withOverrides(raw.scenarios, {
      create: marking(raw.scenarios.create),
      setup: marking(raw.scenarios.setup),
      duplicate: marking(raw.scenarios.duplicate),
      remove: async (id: number) => {
        await raw.scenarios.remove(id);
        // Deleting the last plan is the person's doing, not the browser's.
        if ((await planCount()) === 0) unmark();
      },
    }),
    archives: withOverrides(raw.archives, { import: marking(raw.archives.import) }),
  });

  const runtime: LocalRuntime = {
    estimateRun: remote("estimateRun") as LocalRuntime["estimateRun"],
    planMeta: remote("planMeta") as LocalRuntime["planMeta"],
    markExported: remote("markExported") as LocalRuntime["markExported"],
    saveServerRun: remote("saveServerRun") as LocalRuntime["saveServerRun"],
    snapshot: remote("snapshot") as LocalRuntime["snapshot"],
    storage: {
      persisted: async () => safely(() => env.storage?.persisted?.()),
      requestPersist: async () => {
        const answer = await safely(() => env.storage?.persist?.());
        if (answer !== undefined) {
          void remote("recordPersistence")(answer ? "granted" : "denied");
        }
        return answer;
      },
    },
    storeWasCleared: async () => {
      const had = safely(() => env.localStorage?.getItem(HAD_PLANS_KEY)) === "1";
      return had && (await planCount()) === 0;
    },
    onExternalChange: (listener) => rpc.onExternalChange(listener),
    review: rpc.proxy<LocalReviewApi>("runtime.review"),
  };

  // A browser that has plans from before the marker existed gets it now.
  void planCount().then((count) => count > 0 && mark(), () => undefined);

  return { api, runtime };
}
