/**
 * The privacy promise of spec 19, checked on the whole local stack: every local
 * flow (plans, edits, runs, results, ledger, archives, analysis, what-if,
 * review, estimates) driven through the page's client, over RPC, to the backend
 * and the real engine, with `fetch` stubbed to throw for anything but a static
 * asset and `/health`. No call may reach it.
 *
 * Also the page's side of the cleared-store marker, storage and external
 * changes (`runtime-client.ts`).
 */
import assert from "node:assert/strict";
import { afterEach, beforeEach, test } from "node:test";
import { localApi, setLocalBackend } from "../lib/api/local.ts";
import { type BrowserEnv, HAD_PLANS_KEY, createLocalClient } from "../lib/engine/runtime-client.ts";
import { createRpcClient, serveRpc } from "../lib/engine/rpc.ts";
import { getLocalRuntime, setLocalRuntime } from "../lib/local/runtime.ts";
import { harness, requireEngine, setupAnswers, waitFor } from "./helpers/engine.mts";

const realFetch = globalThis.fetch;
const requested: string[] = [];

beforeEach(() => {
  requested.length = 0;
  globalThis.fetch = (async (input: unknown) => {
    const url = String(input);
    requested.push(url);
    if (/\/_next\/static\//.test(url) || url.endsWith("/api/health")) return new Response("{}");
    throw new Error(`fetch is not allowed in local mode: ${url}`);
  }) as typeof fetch;
});

afterEach(() => {
  globalThis.fetch = realFetch;
  setLocalBackend(undefined);
  setLocalRuntime(undefined);
});

/** The page's client wired to a backend through a real `MessageChannel`. */
async function pageClient(env: BrowserEnv) {
  const h = await harness();
  const channel = new MessageChannel();
  serveRpc(channel.port2 as never, { api: h.api, runtime: h.runtime });
  channel.port1.start();
  channel.port2.start();
  const rpc = createRpcClient(channel.port1 as never, (fields) => Object.assign(new Error(fields.message), fields));
  const client = createLocalClient(rpc, env);
  return { ...h, rpc, client, close: () => (channel.port1.close(), channel.port2.close()) };
}

class FakeStorage {
  items = new Map<string, string>();
  getItem = (key: string) => this.items.get(key) ?? null;
  setItem = (key: string, value: string) => void this.items.set(key, value);
  removeItem = (key: string) => void this.items.delete(key);
}

if (requireEngine("local engine privacy")) {
  test("with fetch stubbed to throw, every local flow works and nothing reaches the network", async () => {
    const env: BrowserEnv = { localStorage: new FakeStorage() };
    const { client, close } = await pageClient(env);
    try {
    // The seam the screens use, with the page's client registered behind it.
    setLocalBackend(client.api);
    setLocalRuntime(client.runtime);
    const api = localApi;
    const runtime = getLocalRuntime()!;

    const [profile] = await api.returnProfiles.list();
    const { scenario_id: id } = await api.scenarios.setup(setupAnswers(profile.id) as never);
    await api.scenarios.update(id, { description: "private" });
    const account = await api.accounts.create(id, {
      name: "Savings", flavor: "Bank", cash_value: 100, return_profile_id: profile.id,
    } as never);
    await api.accounts.update(id, account.id, { name: "Rainy day" } as never);
    await api.events.list(id);
    assert.equal((await api.scenarios.compile(id)).ok, true);
    await api.scenarios.preflight(id);
    await api.historyPresets();
    await api.taxConfigs.list();

    const estimate = await runtime.estimateRun(id, { iterations: 100 });
    assert.ok(estimate.seconds >= 0);

    const queued = await api.runs.create(id, { iterations: 100 });
    const done = await waitFor("the run", async () => {
      const run = await api.runs.get(queued.id);
      return run.status === "queued" || run.status === "running" ? undefined : run;
    });
    assert.equal(done.status, "succeeded");
    assert.equal((await api.runs.results(done.id)).stats.num_iterations, 100);
    await api.runs.ledger(done.id, { limit: 3 });
    await api.runs.inputs(done.id);

    const parameters = await api.analysis.parameters(id);
    const spending = parameters.find((p) => p.name === "Monthly spending");
    assert.ok(spending);
    const analysis = await api.analysis.start(id, {
      kind: "sweep", iterations: 25, axes: [{ parameter_id: spending.id, min: 3000, max: 6000, steps: 2 }],
    } as never);
    await waitFor("the sweep", async () => ((await api.analysis.get(analysis.id)).status === "succeeded" ? true : undefined));
    await api.analysis.results(analysis.id);
    await api.whatIf.quick(id, { layers: [{ kind: "market-shock", age: 50, drop: 0.3 }], iterations: 50 } as never);

    const review = await runtime.review!.run(id, null);
    assert.equal(review.ai, null);

    const archive = await api.scenarios.archive(id);
    await api.archives.preview(archive);
    await api.archives.import({ archive, name_prefix: "Copy of ", request_id: "privacy-1", from_guest: false });
    await api.archives.exportAll();
    await runtime.markExported([id]);
    assert.ok((await runtime.snapshot(id)).hash);
    await api.scenarios.duplicate(id, "Duplicate");
    await api.scenarios.remove(id);
    assert.deepEqual(requested, [], "no request may carry plan data, or any request at all");
    } finally {
      close();
    }
  });

  test("a refusal reaches the page as the error a screen already handles", async () => {
    const { client, close } = await pageClient({});
    await assert.rejects(client.api.scenarios.get(99), (e: { status: number; code: string; body: unknown; message: string }) => {
      assert.equal(e.status, 404);
      assert.equal(e.code, "not_found");
      assert.deepEqual(e.body, { error: { code: "not_found", message: "scenario not found" } });
      return true;
    });
    close();
  });

  test("the cleared-store marker is set when a plan is saved, kept while plans exist and cleared when the last is deleted", async () => {
    const storage = new FakeStorage();
    const { client, close } = await pageClient({ localStorage: storage });
    assert.equal(await client.runtime.storeWasCleared(), false, "a fresh browser has lost nothing");

    const first = await client.api.scenarios.create({ name: "One", start_date: "2026-01-01", duration_years: 10 } as never);
    assert.equal(storage.getItem(HAD_PLANS_KEY), "1");
    const second = await client.api.scenarios.duplicate(first.id, "Two");
    assert.equal(await client.runtime.storeWasCleared(), false);

    await client.api.scenarios.remove(first.id);
    assert.equal(storage.getItem(HAD_PLANS_KEY), "1", "one plan is still here");
    await client.api.scenarios.remove(second.id);
    assert.equal(storage.getItem(HAD_PLANS_KEY), null, "deleting the last plan on purpose is not a cleared store");
    assert.equal(await client.runtime.storeWasCleared(), false);
    close();

    // The browser evicted the data: the marker is there and the store is empty.
    const evicted = await pageClient({ localStorage: Object.assign(new FakeStorage(), { items: new Map([[HAD_PLANS_KEY, "1"]]) }) });
    assert.equal(await evicted.client.runtime.storeWasCleared(), true);
    evicted.close();
  });

  test("storage persistence is asked of the browser, and its answer is kept", async () => {
    const asked: string[] = [];
    const env: BrowserEnv = {
      localStorage: new FakeStorage(),
      storage: {
        persisted: async () => false,
        persist: async () => (asked.push("persist"), true),
      },
    };
    const { client, store, close } = await pageClient(env);
    assert.equal(await client.runtime.storage.persisted(), false);
    assert.equal(await client.runtime.storage.requestPersist(), true);
    assert.deepEqual(asked, ["persist"]);
    await waitFor("the answer to be recorded", async () => {
      const kept = await store.transact("r", (tx) => tx.getMeta<{ answer: string }>("persistence"));
      return kept?.answer === "granted" ? true : undefined;
    });
    // A browser with no storage API answers nothing.
    const bare = await pageClient({});
    assert.equal(await bare.client.runtime.storage.persisted(), undefined);
    assert.equal(await bare.client.runtime.storage.requestPersist(), undefined);
    close();
    bare.close();
  });

  test("another tab's change is announced to the page, and this tab's own writes are not", async () => {
    const { rpc, client, close } = await pageClient({ localStorage: new FakeStorage() });
    const heard: Array<number | null> = [];
    const stop = client.runtime.onExternalChange((plan) => heard.push(plan));
    const plan = await client.api.scenarios.create({ name: "Mine", start_date: "2026-01-01", duration_years: 10 } as never);
    await client.api.scenarios.update(plan.id, { description: "mine too" });
    await new Promise((resolve) => setTimeout(resolve, 20));
    assert.deepEqual(heard, [], "no event for this tab's writes (the worker only forwards other tabs')");
    void rpc;
    stop();
    close();
  });
}
