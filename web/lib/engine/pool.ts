/**
 * The compute pool: N workers, each with its own WASM instance, running whole
 * batches (spec 19, "Parallelism without cross-origin isolation").
 *
 * A run is a plan of batches (`coordinator_next_round`); the pool spreads one
 * round's batches over idle workers and gives the outputs back in spec order.
 * The result of a run depends on the batch plan, not on how many workers there
 * are or which is quicker, so a one-worker pool and an eight-worker pool
 * return the same numbers.
 *
 *  - Workers are created lazily, when work is waiting and none is idle, up to
 *    `size`. A pool that never runs anything never creates one.
 *  - A worker is told about a run once (`prepare`: it compiles the plan) and
 *    then handed batches. Releasing a run tells every worker that prepared it.
 *  - Cancelling stops dispatching a run's batches and terminates the workers
 *    that are executing them (the engine is synchronous: a worker cannot hear a
 *    message while it computes). They are not replaced until work needs them.
 *  - A worker that errors, or traps (an engine panic), is dropped; the work it
 *    held fails, and the next work respawns one.
 *
 * Memory: a batch holds its iterations until it returns its (mergeable, ledger
 * free) output, and the coordinator plans `batch_size` 100 by default (~150 KB of
 * output per batch, a few MB of working memory), far under the spec's budget of
 * about 256 MB per worker. A caller that asks for much bigger batches is
 * choosing it; nothing here raises the batch size.
 *
 * The pool is written against `WorkerLike`, so a test can run it over fake
 * workers that call a `ComputeHost` in process.
 */
import {
  type CalibrationSample,
  type ComputeReply,
  type ComputeRequest,
  createComputeHost,
} from "./compute.ts";
import type { Engine } from "./engine.ts";
import { type LocalError, isPanic, localError, toLocalError } from "./errors.ts";

/** What the pool needs of a `Worker`. */
export interface WorkerLike {
  postMessage(message: unknown): void;
  terminate(): void;
  onmessage: ((event: { data: unknown }) => void) | null;
  onerror: ((event: { message?: string }) => void) | null;
}

export type SpawnWorker = () => WorkerLike;

/** A run as the pool needs to know it: an id for its handles, and what a worker prepares from. */
export interface RunJob {
  job: string;
  snapshot: string;
  /** `CreateRun` JSON. */
  settings: string;
}

export interface AnalysisRequest {
  graph: string;
  library: string;
  body: string;
  /** The quick what-if rather than an analysis job. */
  quick: boolean;
}

/** A Drawdown question about one run: its snapshot, and a `DrawdownRequest` or `CompareRequest` JSON. */
export type DrawdownJob =
  | { compare: false; snapshot: string; /** The median path's decimal seed. */ seed: string; body: string }
  | { compare: true; snapshot: string; body: string };

export interface AnalysisHandle {
  /** The `AnalysisOutcome` (or `WhatIfOutcome`) JSON. */
  promise: Promise<string>;
  /** Terminates the worker running it; `promise` rejects with the cancellation. */
  cancel(): void;
}

export interface ComputePool {
  /** The most workers the pool will run at once. */
  readonly size: number;
  /**
   * Run `specs` (one `BatchSpec` text each); `BatchOutput` texts come back in
   * the same order. `onProgress` hears a batch's finished simulations as it
   * runs, by the batch's index in `specs`, so a round reports progress before
   * any of its batches is whole.
   */
  runBatches(
    job: RunJob,
    specs: readonly string[],
    onProgress?: (index: number, done: number) => void,
  ): Promise<string[]>;
  /** The run is over: workers forget its prepared plan. */
  endJob(job: string): void;
  /** Stop the run now: queued batches fail, workers executing them are terminated. */
  cancelJob(job: string): void;
  analysis(request: AnalysisRequest, onProgress?: (done: number, total: number) => void): AnalysisHandle;
  /** The `DrawdownBody` (or, compared, the `DrawdownComparison`) JSON; on one worker, off the store's thread. */
  drawdown(job: DrawdownJob, onProgress?: (done: number, total: number) => void): AnalysisHandle;
  calibrate(snapshot: string, settings: string): Promise<CalibrationSample>;
  /** Terminate every worker and refuse further work. */
  terminate(): void;
}

/** The pool size for a device: all cores but one for the page, at most eight. */
export function poolSize(hardwareConcurrency: number | undefined): number {
  const cores = Number.isFinite(hardwareConcurrency) ? (hardwareConcurrency as number) : 2;
  return Math.min(8, Math.max(1, Math.floor(cores) - 1));
}

const cancelled = (what: string) => localError(409, "conflict", `${what} canceled`);

interface Slot {
  worker: WorkerLike;
  busy: boolean;
  /** Runs this worker has prepared. */
  prepared: Set<string>;
  /** The run or analysis it is executing, for cancelling. */
  current: { owner: string; fail: (error: LocalError) => void } | undefined;
}

interface Pending {
  slot: Slot;
  resolve: (reply: Extract<ComputeReply, { ok: true }>) => void;
  reject: (error: LocalError) => void;
  onProgress?: (done: number, total: number) => void;
}

/** One unit of work waiting for an idle worker. */
interface Task {
  owner: string;
  start: (slot: Slot) => void;
  fail: (error: LocalError) => void;
}

export class WorkerPool implements ComputePool {
  readonly size: number;
  private spawn: SpawnWorker;
  private slots: Slot[] = [];
  private queue: Task[] = [];
  private pending = new Map<number, Pending>();
  private nextId = 1;
  private terminated = false;

  constructor(spawn: SpawnWorker, size: number) {
    this.spawn = spawn;
    this.size = Math.max(1, size);
  }

  // ── workers ──────────────────────────────────────────────────────────────

  private addWorker(): Slot {
    const worker = this.spawn();
    const slot: Slot = { worker, busy: false, prepared: new Set(), current: undefined };
    worker.onmessage = (event) => this.onReply(event.data as ComputeReply);
    worker.onerror = (event) =>
      this.drop(slot, localError(500, "internal", event.message || "a compute worker failed"));
    this.slots.push(slot);
    return slot;
  }

  /** Remove a worker and fail what it was doing. */
  private drop(slot: Slot, error: LocalError): void {
    const index = this.slots.indexOf(slot);
    if (index >= 0) this.slots.splice(index, 1);
    try {
      slot.worker.terminate();
    } catch {
      // Already gone.
    }
    slot.worker.onmessage = null;
    slot.worker.onerror = null;
    for (const [id, pending] of [...this.pending]) {
      if (pending.slot === slot) {
        this.pending.delete(id);
        pending.reject(error);
      }
    }
    const current = slot.current;
    slot.current = undefined;
    slot.busy = false;
    current?.fail(error);
    this.pump();
  }

  private idle(): Slot | undefined {
    return this.slots.find((slot) => !slot.busy) ?? (this.slots.length < this.size ? this.addWorker() : undefined);
  }

  private pump(): void {
    while (!this.terminated && this.queue.length > 0) {
      const slot = this.idle();
      if (!slot) return;
      const task = this.queue.shift() as Task;
      slot.busy = true;
      slot.current = { owner: task.owner, fail: task.fail };
      task.start(slot);
    }
  }

  private finish(slot: Slot): void {
    slot.busy = false;
    slot.current = undefined;
    this.pump();
  }

  // ── messages ─────────────────────────────────────────────────────────────

  private send(
    slot: Slot,
    request: DistributiveOmit<ComputeRequest, "id">,
    onProgress?: (done: number, total: number) => void,
  ): Promise<Extract<ComputeReply, { ok: true }>> {
    return new Promise((resolve, reject) => {
      const id = this.nextId++;
      this.pending.set(id, { slot, resolve, reject, onProgress });
      slot.worker.postMessage({ ...request, id });
    });
  }

  private onReply(reply: ComputeReply): void {
    const pending = this.pending.get(reply.id);
    if (!pending) return;
    if ("progress" in reply) {
      pending.onProgress?.(reply.progress[0], reply.progress[1]);
      return;
    }
    this.pending.delete(reply.id);
    if (reply.ok) {
      pending.resolve(reply);
      return;
    }
    const error = toLocalError(reply.error);
    pending.reject(error);
    // A panic leaves the instance unusable: replace the worker.
    if (isPanic(reply.error)) this.drop(pending.slot, error);
  }

  // ── runs ─────────────────────────────────────────────────────────────────

  runBatches(
    job: RunJob,
    specs: readonly string[],
    onProgress?: (index: number, done: number) => void,
  ): Promise<string[]> {
    if (this.terminated) return Promise.reject(cancelled("the compute pool is closed; the run"));
    const outputs = specs.map(
          (spec, index) =>
          new Promise<string>((resolve, reject) => {
            this.queue.push({
              owner: job.job,
              fail: reject,
              start: (slot) => {
                const run = async () => {
                  if (!slot.prepared.has(job.job)) {
                    await this.send(slot, {
                      kind: "prepare",
                      job: job.job,
                      snapshot: job.snapshot,
                      settings: job.settings,
                    });
                    slot.prepared.add(job.job);
                  }
                  const reply = await this.send(
                    slot,
                    { kind: "batch", job: job.job, spec },
                    onProgress && ((done) => onProgress(index, done)),
                  );
                  resolve(reply.value as string);
                };
                run().then(
                  () => this.finish(slot),
                  (error) => {
                    reject(toLocalError(error));
                    // A slot dropped by `drop` is gone already.
                    if (this.slots.includes(slot)) this.finish(slot);
                  },
                );
              },
            });
          }),
    );
    this.pump();
    return Promise.all(outputs);
  }

  endJob(job: string): void {
    for (const slot of this.slots) {
      if (slot.prepared.delete(job)) slot.worker.postMessage({ id: 0, kind: "release", job });
    }
  }

  cancelJob(job: string): void {
    const error = cancelled("run");
    // Queued batches never start.
    const waiting = this.queue.filter((task) => task.owner === job);
    this.queue = this.queue.filter((task) => task.owner !== job);
    for (const task of waiting) task.fail(error);
    // Workers executing one are terminated, and respawn when work needs them.
    for (const slot of [...this.slots]) {
      if (slot.current?.owner === job) this.drop(slot, error);
      else slot.prepared.delete(job);
    }
    for (const slot of this.slots) slot.worker.postMessage({ id: 0, kind: "release", job });
  }

  // ── analyses and calibration ─────────────────────────────────────────────

  analysis(request: AnalysisRequest, onProgress?: (done: number, total: number) => void): AnalysisHandle {
    const owner = `analysis-${this.nextId++}`;
    let stopped: LocalError | undefined;
    const stop = (error: LocalError) => {
      stopped ??= error;
      const waiting = this.queue.filter((task) => task.owner === owner);
      this.queue = this.queue.filter((task) => task.owner !== owner);
      for (const task of waiting) task.fail(error);
      for (const slot of [...this.slots]) {
        if (slot.current?.owner === owner) this.drop(slot, error);
      }
    };
    // A quick what-if is one short call; anything else is split across every
    // worker the pool may run (the engine runs an analysis it cannot split
    // whole on the first, and the others answer at once).
    const shards = request.quick ? 1 : this.size;
    const promise: Promise<string> =
      shards === 1
        ? this.task(owner, { kind: "analysis", ...request }, onProgress)
        : this.sharded(owner, request, shards, onProgress, () => stopped).catch((error: unknown) => {
            const failure = toLocalError(error);
            // One shard failing stops the rest: the analysis has no answer.
            stop(failure);
            throw stopped ?? failure;
          });
    return { promise, cancel: () => stop(cancelled("analysis")) };
  }

  drawdown(job: DrawdownJob, onProgress?: (done: number, total: number) => void): AnalysisHandle {
    const owner = `drawdown-${this.nextId++}`;
    const request: DistributiveOmit<ComputeRequest, "id"> = job.compare
      ? { kind: "drawdown-compare", snapshot: job.snapshot, body: job.body }
      : { kind: "drawdown", snapshot: job.snapshot, seed: job.seed, body: job.body };
    const promise = this.task(owner, request, onProgress);
    const stop = (error: LocalError) => {
      const waiting = this.queue.filter((task) => task.owner === owner);
      this.queue = this.queue.filter((task) => task.owner !== owner);
      for (const task of waiting) task.fail(error);
      for (const slot of [...this.slots]) {
        if (slot.current?.owner === owner) this.drop(slot, error);
      }
    };
    return { promise, cancel: () => stop(cancelled("drawdown")) };
  }

  /** One request on the next idle worker, under `owner`. */
  private task(
    owner: string,
    request: DistributiveOmit<ComputeRequest, "id">,
    onProgress?: (done: number, total: number) => void,
  ): Promise<string> {
    return new Promise<string>((resolve, reject) => {
      if (this.terminated) {
        reject(cancelled("the compute pool is closed; the analysis"));
        return;
      }
      this.queue.push({
        owner,
        fail: reject,
        start: (slot) => {
          this.send(slot, request, onProgress).then(
            (reply) => {
              resolve(reply.value as string);
              this.finish(slot);
            },
            (error) => {
              reject(toLocalError(error));
              if (this.slots.includes(slot)) this.finish(slot);
            },
          );
        },
      });
      this.pump();
    });
  }

  /**
   * An analysis on `shards` workers at once: each simulates its share of the
   * Monte Carlo calls, then one pass assembles the outcome from their answers
   * (and simulates anything they did not answer, so it is the one-worker
   * outcome). Progress is the shards' simulations added up.
   */
  private async sharded(
    owner: string,
    request: AnalysisRequest,
    shards: number,
    onProgress: ((done: number, total: number) => void) | undefined,
    stopped: () => LocalError | undefined,
  ): Promise<string> {
    const { graph, library, body } = request;
    const done = new Array<number>(shards).fill(0);
    let total = 0;
    const report = (shard: number) => (count: number, of: number) => {
      done[shard] = count;
      total = Math.max(total, of);
      const sum = done.reduce((a, b) => a + b, 0);
      onProgress?.(sum, Math.max(total, sum));
    };
    const answers = await Promise.all(
      done.map((_, shard) =>
        this.task(owner, { kind: "analysis-shard", graph, library, body, shard, shards }, report(shard)),
      ),
    );
    const halt = stopped();
    if (halt) throw halt;
    const before = done.reduce((a, b) => a + b, 0);
    // The assembly simulates only what no shard answered: shown on top of theirs.
    return this.task(
      owner,
      { kind: "analysis-finish", graph, library, body, answers: JSON.stringify(answers) },
      (count, of) => onProgress?.(before + count, Math.max(total, before + of)),
    );
  }

  calibrate(snapshot: string, settings: string): Promise<CalibrationSample> {
    const owner = `calibrate-${this.nextId++}`;
    return new Promise((resolve, reject) => {
      if (this.terminated) {
        reject(cancelled("the compute pool is closed; the calibration"));
        return;
      }
      this.queue.push({
        owner,
        fail: reject,
        start: (slot) => {
          this.send(slot, { kind: "calibrate", snapshot, settings }).then(
            (reply) => {
              resolve(reply.value as CalibrationSample);
              this.finish(slot);
            },
            (error) => {
              reject(toLocalError(error));
              if (this.slots.includes(slot)) this.finish(slot);
            },
          );
        },
      });
      this.pump();
    });
  }

  terminate(): void {
    this.terminated = true;
    const error = cancelled("the compute pool is closed; the work");
    for (const task of this.queue.splice(0)) task.fail(error);
    for (const slot of [...this.slots]) this.drop(slot, error);
  }
}

/** `Omit` that keeps a union's members apart. */
type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

// ── in-process workers ──────────────────────────────────────────────────────

/**
 * A `WorkerLike` that runs a `ComputeHost` in this process, one message per
 * turn of the event loop. For tests, and as the fallback where a browser has no
 * nested workers: the engine then runs on whatever thread created the pool, one
 * batch at a time. Terminating it drops the replies of whatever is running (a
 * synchronous call cannot be interrupted) and makes an analysis stop at its
 * next progress report.
 */
export class InProcessWorker implements WorkerLike {
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onerror: ((event: { message?: string }) => void) | null = null;
  private host: ReturnType<typeof createComputeHost>;
  private dead = false;

  constructor(engine: Engine) {
    this.host = createComputeHost(engine);
  }

  postMessage(message: unknown): void {
    if (this.dead) return;
    setTimeout(() => {
      if (this.dead) return;
      this.host.handle(
        message as ComputeRequest,
        (reply) => {
          if (!this.dead) this.onmessage?.({ data: reply });
        },
        () => this.dead,
      );
    }, 0);
  }

  terminate(): void {
    this.dead = true;
    this.host.dispose();
  }
}

/** A pool of in-process workers over one engine instance. */
export function inProcessPool(engine: Engine, size = 2): WorkerPool {
  return new WorkerPool(() => new InProcessWorker(engine), size);
}
