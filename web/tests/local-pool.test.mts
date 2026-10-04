/**
 * The compute pool, the batch protocol and the RPC between the page and the
 * store worker, over in-process workers and a `MessageChannel`: no browser, no
 * threads, the real engine.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import { joinOutputs, splitSpecs } from "../lib/engine/engine.ts";
import { LocalError } from "../lib/engine/errors.ts";
import { InProcessWorker, type WorkerLike, WorkerPool, poolSize } from "../lib/engine/pool.ts";
import { createRpcClient, serveRpc } from "../lib/engine/rpc.ts";
import { harness, loadEngine, requireEngine, setupAnswers } from "./helpers/engine.mts";

test("the pool is every core but one, between one and eight", () => {
  assert.equal(poolSize(undefined), 1);
  assert.equal(poolSize(1), 1);
  assert.equal(poolSize(2), 1);
  assert.equal(poolSize(4), 3);
  assert.equal(poolSize(8), 7);
  assert.equal(poolSize(16), 8);
  assert.equal(poolSize(Number.NaN), 1);
});

test("a round of specs is split and joined as text, so a 64-bit seed is never rounded", () => {
  // 2^64 - 1 and 2^53 + 1 are not representable as doubles.
  const round =
    '[{"index":0,"seed":18446744073709551615,"iterations":100},{"index":1,"seed":9007199254740993,"iterations":50}]';
  const specs = splitSpecs(round);
  assert.deepEqual(specs, [
    '{"index":0,"seed":18446744073709551615,"iterations":100}',
    '{"index":1,"seed":9007199254740993,"iterations":50}',
  ]);
  assert.equal(joinOutputs(['{"a":1}', '{"b":2}']), '[{"a":1},{"b":2}]');
  assert.deepEqual(splitSpecs("[]"), []);
  // What JSON.parse would have done to it.
  assert.notEqual(String(JSON.parse(specs[0]).seed), "18446744073709551615");
});

test("the pool does not exist until work needs it, and an RPC carries calls, refusals and aborts", async () => {
  // A pool that is never used creates no worker.
  let spawned = 0;
  const idle = new WorkerPool(() => {
    spawned++;
    throw new Error("not needed");
  }, 4);
  assert.equal(spawned, 0);
  idle.terminate();

  const channel = new MessageChannel();
  const target = {
    api: {
      echo: async (value: unknown) => value,
      add: async (a: number, b: number) => a + b,
      refuse: async () => {
        throw new LocalError(409, "conflict", "no", { error: { code: "conflict", message: "no" } });
      },
      slow: (signal: AbortSignal) =>
        new Promise((resolve, reject) => {
          signal.addEventListener("abort", () => reject(new Error("stopped")));
          setTimeout(() => resolve("finished"), 2_000).unref();
        }),
    },
  };
  serveRpc(channel.port2 as never, target);
  channel.port1.start();
  channel.port2.start();
  const client = createRpcClient(channel.port1 as never, (fields) =>
    Object.assign(new Error(fields.message), fields),
  );
  const api = client.proxy<{
    echo(value: unknown): Promise<unknown>;
    add(a: number, b: number): Promise<number>;
    refuse(): Promise<void>;
    slow(signal: AbortSignal): Promise<unknown>;
    nothing(): Promise<void>;
  }>("api");
  assert.deepEqual(await api.echo({ a: [1, 2, { b: null }] }), { a: [1, 2, { b: null }] });
  assert.equal(await api.add(2, 3), 5);
  await assert.rejects(api.refuse(), (e: { status: number; code: string; body: unknown }) => {
    assert.equal(e.status, 409);
    assert.equal(e.code, "conflict");
    assert.deepEqual(e.body, { error: { code: "conflict", message: "no" } });
    return true;
  });
  await assert.rejects(api.nothing(), (e: { status: number }) => e.status === 404);
  const controller = new AbortController();
  const slow = api.slow(controller.signal);
  setTimeout(() => controller.abort(), 20);
  await assert.rejects(slow, /stopped/);

  const events: Array<number | null> = [];
  client.onExternalChange((plan) => events.push(plan));
  channel.port2.postMessage({ type: "event", name: "external-change", plan: 7 });
  channel.port2.postMessage({ type: "ready" });
  await client.ready;
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.deepEqual(events, [7]);
  channel.port1.close();
  channel.port2.close();
});

if (requireEngine("the compute pool")) {
  /** A whole run through a pool: coordinator, rounds, finish. */
  async function runThrough(pool: WorkerPool, snapshot: string, settings: string, job: string) {
    const engine = await loadEngine();
    const coordinator = engine.coordinator_new(snapshot, settings);
    try {
      for (let round = engine.coordinator_next_round(coordinator); round; round = engine.coordinator_next_round(coordinator)) {
        const outputs = await pool.runBatches({ job, snapshot, settings }, splitSpecs(round));
        engine.coordinator_absorb(coordinator, joinOutputs(outputs));
      }
      return engine.coordinator_finish(coordinator);
    } finally {
      engine.coordinator_drop(coordinator);
      pool.endJob(job);
    }
  }

  async function snapshotOf() {
    const h = await harness();
    const [profile] = await h.api.returnProfiles.list();
    const { scenario_id } = await h.api.scenarios.setup(setupAnswers(profile.id) as never);
    return (await h.runtime.snapshot(scenario_id)).json;
  }

  test("a round's batches run on as many workers as the pool allows, and the answer is the same whatever that is", async () => {
    const engine = await loadEngine();
    const snapshot = await snapshotOf();
    const settings = JSON.stringify({ iterations: 400, seed: 5, batch_size: 100, parallel_batches: 4 });

    let spawned = 0;
    const counted = (size: number) => new WorkerPool(() => (spawned++, new InProcessWorker(engine)), size);
    const one = counted(1);
    const four = counted(4);
    const a = await runThrough(one, snapshot, settings, "a");
    assert.equal(spawned, 1);
    const b = await runThrough(four, snapshot, settings, "b");
    assert.equal(spawned, 1 + 4, "four batches in a round take four workers");
    assert.equal(a, b);
    one.terminate();
    four.terminate();
  });

  test("cancelling a run terminates the workers on it, and the next work respawns them", async () => {
    const engine = await loadEngine();
    const snapshot = await snapshotOf();
    const settings = JSON.stringify({ iterations: 400, seed: 5, batch_size: 100, parallel_batches: 4 });
    let spawned = 0;
    const live = new Set<InProcessWorker>();
    const pool = new WorkerPool(() => {
      spawned++;
      const worker = new InProcessWorker(engine);
      live.add(worker);
      const terminate = worker.terminate.bind(worker);
      worker.terminate = () => {
        live.delete(worker);
        terminate();
      };
      return worker;
    }, 2);

    const coordinator = engine.coordinator_new(snapshot, settings);
    const round = engine.coordinator_next_round(coordinator) as string;
    const running = pool.runBatches({ job: "x", snapshot, settings }, splitSpecs(round));
    const failure = assert.rejects(running, (e: { status: number; message: string }) => e.status === 409 && /canceled/.test(e.message));
    pool.cancelJob("x");
    await failure;
    assert.equal(live.size, 0, "the workers that were running it are gone");
    engine.coordinator_drop(coordinator);

    // Work after a cancel gets fresh workers.
    const done = await runThrough(pool, snapshot, settings, "y");
    assert.ok(done.length > 0);
    assert.ok(spawned >= 3);
    pool.terminate();
    await assert.rejects(pool.runBatches({ job: "z", snapshot, settings }, ["{}"]), (e: { status: number }) => e.status === 409);
  });

  test("a worker that traps is dropped, its work fails, and the pool carries on with another", async () => {
    const engine = await loadEngine();
    const snapshot = await snapshotOf();
    const settings = JSON.stringify({ iterations: 100, seed: 5, batch_size: 100, parallel_batches: 1 });
    let made = 0;
    const panicking: WorkerLike = {
      onmessage: null,
      onerror: null,
      terminate() {},
      postMessage(message: unknown) {
        const request = message as { id: number; kind: string };
        setTimeout(() => {
          panicking.onmessage?.({
            data: {
              id: request.id,
              ok: false,
              error: JSON.stringify({ status: 500, code: "panic", message: "unreachable executed" }),
            },
          });
        }, 0);
      },
    };
    const pool = new WorkerPool(() => (made++ === 0 ? panicking : new InProcessWorker(engine)), 1);
    const coordinator = engine.coordinator_new(snapshot, settings);
    const round = engine.coordinator_next_round(coordinator) as string;
    await assert.rejects(
      pool.runBatches({ job: "p", snapshot, settings }, splitSpecs(round)),
      (e: { status: number; code: string }) => e.code === "panic",
    );
    const outputs = await pool.runBatches({ job: "p", snapshot, settings }, splitSpecs(round));
    engine.coordinator_absorb(coordinator, joinOutputs(outputs));
    assert.equal(engine.coordinator_completed(coordinator), 100);
    assert.equal(made, 2, "the trapped worker was replaced");
    engine.coordinator_drop(coordinator);
    pool.terminate();
  });
}
