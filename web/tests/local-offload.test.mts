import assert from "node:assert/strict";
import { test } from "node:test";
import type { ComputeRun } from "../lib/api/generated/ComputeRun.ts";
import type { Run } from "../lib/api/types.ts";
import {
  type ComputeClient,
  type OffloadState,
  RELOAD_TO_UPDATE,
  offloadActive,
  offloadProgressRun,
  runOffload,
  toComputeSettings,
} from "../lib/local/offload.ts";

const SETTINGS = { iterations: 5000, converge: false, percentiles: [0.1, 0.5, 0.9] };
const RESULTS = { success_rate: 0.9 };
const SAVED = { id: 12, scenario_id: 3 } as unknown as Run;

const job = (over: Partial<ComputeRun>): ComputeRun => ({
  id: 40,
  status: "running",
  completed_iterations: 0,
  iterations: 5000,
  seed: 1,
  results: null,
  ...over,
});

interface Harness {
  log: string[];
  states: OffloadState[];
  compute: ComputeClient;
  runtime: Parameters<typeof runOffload>[0]["runtime"];
  saved: unknown[];
}

function harness(polls: Array<ComputeRun | Error>, options: { createError?: unknown; saveError?: Error } = {}): Harness {
  const log: string[] = [];
  const saved: unknown[] = [];
  const queue = [...polls];
  return {
    log,
    states: [],
    saved,
    compute: {
      create: async (body) => {
        log.push("create");
        assert.equal(typeof body.snapshot, "string", "the snapshot is sent as the JSON text");
        assert.equal(body.model_version, "m1");
        assert.deepEqual(body.settings, toComputeSettings(SETTINGS));
        if (options.createError) throw options.createError;
        return { id: 40 };
      },
      get: async () => {
        log.push("get");
        const next = queue.length > 1 ? queue.shift()! : queue[0];
        if (next instanceof Error) throw next;
        return next;
      },
      remove: async () => void log.push("remove"),
    },
    runtime: {
      snapshot: async () => {
        log.push("snapshot");
        return { json: '{"plan":1}', hash: "h1", modelVersion: "m1" };
      },
      saveServerRun: async (id, run) => {
        log.push("save");
        if (options.saveError) throw options.saveError;
        saved.push([id, run]);
        return SAVED;
      },
    },
  };
}

function run(h: Harness, extra: Partial<Parameters<typeof runOffload>[0]> = {}) {
  return runOffload({
    compute: h.compute,
    runtime: h.runtime,
    scenarioId: 3,
    settings: SETTINGS,
    onState: (state) => h.states.push(state),
    sleep: async () => undefined,
    ...extra,
  });
}

test("snapshot, create, poll through progress, save locally, then delete the job", async () => {
  const h = harness([
    job({ status: "queued" }),
    job({ status: "running", completed_iterations: 2000 }),
    job({ status: "succeeded", completed_iterations: 5000, results: RESULTS }),
  ]);
  const end = await run(h);

  assert.deepEqual(end, { phase: "done", run: SAVED });
  assert.deepEqual(h.log, ["snapshot", "create", "get", "get", "get", "save", "remove"]);
  assert.deepEqual(
    h.states.map((s) => s.phase),
    ["sending", "queued", "running", "saving", "done"],
  );
  assert.deepEqual(h.states[2], { phase: "running", completed: 2000, total: 5000 });
  assert.deepEqual(h.saved, [
    [3, { settings: SETTINGS, inputHash: "h1", modelVersion: "m1", results: RESULTS }],
  ]);
});

test("a model mismatch (409) asks for a reload and polls nothing", async () => {
  const h = harness([], { createError: Object.assign(new Error("conflict"), { status: 409 }) });
  const end = await run(h);
  assert.deepEqual(end, { phase: "stale", message: RELOAD_TO_UPDATE });
  assert.deepEqual(h.log, ["snapshot", "create"]);
});

test("another refusal (budget, size) is shown as returned", async () => {
  const h = harness([], { createError: Object.assign(new Error("Budget spent"), { status: 403 }) });
  assert.deepEqual(await run(h), { phase: "failed", message: "Budget spent" });
});

test("a job that fails on the server is a failure with its reason, and nothing is saved", async () => {
  const h = harness([job({ status: "failed", error: "compile error" })]);
  assert.deepEqual(await run(h), { phase: "failed", message: "compile error" });
  assert.ok(!h.log.includes("save"));
});

test("aborting cancels the job on the server", async () => {
  const controller = new AbortController();
  const h = harness([job({ status: "running" })]);
  h.compute.get = async () => {
    h.log.push("get");
    controller.abort();
    return job({ status: "running" });
  };
  const end = await run(h, { signal: controller.signal });
  assert.deepEqual(end, { phase: "canceled" });
  assert.ok(h.log.includes("remove"));
  assert.ok(!h.log.includes("save"));
});

test("aborting before the plan is sent sends nothing", async () => {
  const controller = new AbortController();
  controller.abort();
  const h = harness([job({})]);
  const end = await run(h, { signal: controller.signal });
  assert.deepEqual(end, { phase: "canceled" });
  assert.ok(!h.log.includes("create"));
});

test("a job canceled elsewhere ends as canceled", async () => {
  const h = harness([job({ status: "canceled" })]);
  assert.deepEqual(await run(h), { phase: "canceled" });
});

test("a brief poll failure is waited out", async () => {
  const h = harness([
    new Error("network"),
    new Error("network"),
    job({ status: "succeeded", results: RESULTS }),
  ]);
  const end = await run(h);
  assert.equal(end.phase, "done");
});

test("polling gives up after repeated failures, and a vanished job at once", async () => {
  const flaky = harness([new Error("network")]);
  assert.deepEqual(await run(flaky), { phase: "failed", message: "network" });

  const gone = harness([Object.assign(new Error("not found"), { status: 404 })]);
  assert.deepEqual(await run(gone), { phase: "failed", message: "not found" });
  assert.equal(gone.log.filter((entry) => entry === "get").length, 1);
});

test("results that cannot be saved locally are a failure and the job is kept for retrieval", async () => {
  const h = harness([job({ status: "succeeded", results: RESULTS })], {
    saveError: new Error("quota"),
  });
  const end = await run(h);
  assert.equal(end.phase, "failed");
  if (end.phase === "failed") assert.match(end.message, /could not be saved on this device: quota/);
  assert.ok(!h.log.includes("remove"));
});

test("a failed delete of a finished job changes nothing the person sees", async () => {
  const h = harness([job({ status: "succeeded", results: RESULTS })]);
  h.compute.remove = async () => {
    throw new Error("gone");
  };
  assert.equal((await run(h)).phase, "done");
});

test("settings carry only what the server takes, with a seed only when there is one", () => {
  assert.deepEqual(toComputeSettings(SETTINGS), SETTINGS);
  assert.deepEqual(toComputeSettings({ ...SETTINGS, seed: 7 }), { ...SETTINGS, seed: 7 });
  assert.deepEqual(toComputeSettings({ ...SETTINGS, seed: null }), SETTINGS);
});

test("the header bar reads the offload as a run while it is active, and not otherwise", () => {
  assert.equal(offloadActive({ phase: "idle" }), false);
  assert.equal(offloadActive({ phase: "stale", message: "x" }), false);
  assert.equal(offloadProgressRun({ phase: "done", run: SAVED }, 3, { iterations: 100 }), undefined);

  const sending = offloadProgressRun({ phase: "sending" }, 3, { iterations: 100 });
  assert.equal(sending?.status, "queued");
  assert.equal(sending?.iterations, 100);

  const running = offloadProgressRun({ phase: "running", completed: 40, total: 100 }, 3, {
    iterations: 100,
  });
  assert.equal(running?.status, "running");
  assert.equal(running?.completed_iterations, 40);
});
