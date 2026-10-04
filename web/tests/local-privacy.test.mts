import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { afterEach, beforeEach, test } from "node:test";
import { localApi, setLocalBackend } from "../lib/api/local.ts";
import type { PlanApi } from "../lib/api/plan.ts";
import { decideRun } from "../lib/local/estimate.ts";
import { LOCAL_MODE_STORAGE_KEY, localModeEnabled, setServerLocalMode } from "../lib/local/flag.ts";
import { exportLocal, importIntoDevice, parseArchive } from "../lib/local/homes.ts";
import { getLocalRuntime, setLocalRuntime } from "../lib/local/runtime.ts";
import { LOCAL_VISITOR, isLocalVisitor } from "../lib/local/visitor.ts";
import { planCapabilities } from "../lib/nav/capabilities.ts";

/** The browser's switch for local mode, as `flag.ts` reads it. */
function stubLocalStorage(values: Record<string, string>) {
  const original = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: { getItem: (key: string) => values[key] ?? null, setItem() {}, removeItem() {} },
  });
  return () => {
    if (original) Object.defineProperty(globalThis, "localStorage", original);
    else delete (globalThis as { localStorage?: unknown }).localStorage;
  };
}

/** An in-memory local store: enough PlanApi for create, edit, run listing, archive and import. */
function memoryBackend(): PlanApi {
  const plans = new Map<number, { id: number; slug: string; name: string }>();
  let next = 1;
  return {
    scenarios: {
      create: async (body: { name: string }) => {
        const plan = { id: next, slug: `l${next}`, name: body.name };
        plans.set(next++, plan);
        return plan;
      },
      update: async (id: number, body: { name?: string }) => {
        const plan = plans.get(id)!;
        Object.assign(plan, body);
        return plan;
      },
      list: async () => [...plans.values()],
      archive: async (id: number) => ({
        format: "finplan.plan-archive",
        version: 3,
        plans: [plans.get(id)],
      }),
    },
    archives: {
      import: async (body: { archive: { plans: Array<{ name: string }> } }) => ({
        scenario_ids: body.archive.plans.map((plan) => {
          const id = next++;
          plans.set(id, { id, slug: `l${id}`, name: plan.name });
          return id;
        }),
      }),
    },
  } as unknown as PlanApi;
}

let restoreStorage: () => void;
const realFetch = globalThis.fetch;
const fetched: string[] = [];

beforeEach(() => {
  restoreStorage = stubLocalStorage({ [LOCAL_MODE_STORAGE_KEY]: "1" });
  setServerLocalMode(true);
  fetched.length = 0;
  // Anything but the health probe is a leak.
  globalThis.fetch = (async (input: unknown) => {
    const url = String(input);
    fetched.push(url);
    if (url.endsWith("/api/health")) return new Response('{"status":"ok","local_mode":true}');
    throw new Error(`fetch is not allowed in local mode: ${url}`);
  }) as typeof fetch;
});

afterEach(() => {
  globalThis.fetch = realFetch;
  restoreStorage();
  setServerLocalMode(undefined);
  setLocalBackend(undefined);
  setLocalRuntime(undefined);
});

test("with local mode on and no account, creating, editing, running estimates and exporting never touch the network", async () => {
  assert.equal(localModeEnabled(), true);
  setLocalBackend(memoryBackend());
  const marked: number[][] = [];
  setLocalRuntime({
    estimateRun: async () => ({ seconds: 3, workers: 4, constrained: false, calibrated: true }),
    markExported: async (ids: number[]) => void marked.push(ids),
  } as unknown as NonNullable<ReturnType<typeof getLocalRuntime>>);

  // Signed out: the visitor stand-in, whose capabilities close every server-only door.
  assert.equal(isLocalVisitor(LOCAL_VISITOR), true);
  const capabilities = planCapabilities("l1", { account: !LOCAL_VISITOR.guest });
  assert.equal(capabilities.ai, false);
  assert.equal(capabilities.offload, false);

  const created = await localApi.scenarios.create({ name: "Retirement" } as never);
  await localApi.scenarios.update(created.id, { name: "Retirement v2" } as never);
  assert.equal((await localApi.scenarios.list()).length, 1);

  const estimate = await getLocalRuntime()!.estimateRun(created.id, { iterations: 1000 });
  assert.deepEqual(decideRun(estimate), { kind: "run" });

  const saved: Array<[string, unknown]> = [];
  await exportLocal(
    { local: localApi },
    getLocalRuntime(),
    { kind: "one", id: created.id, name: "Retirement v2" },
    (filename, archive) => void saved.push([filename, archive]),
  );
  assert.equal(saved.length, 1);
  assert.deepEqual(marked, [[created.id]]);

  // And back in from the file.
  const archive = parseArchive(JSON.stringify(saved[0][1]));
  assert.equal((await importIntoDevice({ local: localApi }, archive, "k")).length, 1);

  assert.deepEqual(fetched, [], "no request may carry plan data, or any request at all");
});

test("the stub really does refuse everything but /api/health", async () => {
  await assert.rejects(fetch("/api/scenarios"), /not allowed/);
  assert.equal((await fetch("/api/health")).ok, true);
});

// The files that make up the local flows are the whole of what can reach plan
// data in local mode, so none of them may reach the network or the cloud's api.
const LOCAL_FLOW_FILES = [
  "../lib/api/local.ts",
  "../lib/local/runtime.ts",
  "../lib/local/estimate.ts",
  "../lib/local/durability.ts",
  "../lib/local/homes.ts",
  "../lib/local/offload.ts",
  "../lib/local/visitor.ts",
  // The engine itself: the store worker, the compute workers and everything they
  // run. Only `boot.ts` (the page's side, which hands `ApiError` to the RPC) may
  // import `http`, and it never calls `fetch` either.
  "../lib/engine/analysis.ts",
  "../lib/engine/backend.ts",
  "../lib/engine/compute.ts",
  "../lib/engine/compute.worker.ts",
  "../lib/engine/core.ts",
  "../lib/engine/edits.ts",
  "../lib/engine/engine.ts",
  "../lib/engine/errors.ts",
  "../lib/engine/idb.ts",
  "../lib/engine/locks.ts",
  "../lib/engine/pool.ts",
  "../lib/engine/review.ts",
  "../lib/engine/rpc.ts",
  "../lib/engine/runs.ts",
  "../lib/engine/runtime-client.ts",
  "../lib/engine/scenarios.ts",
  "../lib/engine/store.ts",
  "../lib/engine/store.worker.ts",
];

for (const file of LOCAL_FLOW_FILES) {
  test(`${file} does not reach fetch, http or the cloud api`, () => {
    const source = readFileSync(new URL(file, import.meta.url), "utf8")
      // Comments may talk about all of these.
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/^\s*\/\/.*$/gm, "");
    assert.doesNotMatch(source, /\bfetch\s*\(/);
    assert.doesNotMatch(source, /\bXMLHttpRequest\b|\bsendBeacon\b|\bWebSocket\b/);
    assert.doesNotMatch(source, /from\s+["'][^"']*\/(http|remote|client)(\.ts)?["']/);
  });
}
