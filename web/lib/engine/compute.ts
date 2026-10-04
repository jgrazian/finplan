/**
 * The compute side of the local engine: what runs inside a compute worker (or,
 * in tests, in the same process), one WASM instance each.
 *
 * A compute worker is a thin loop over a `ComputeHost`: it receives a request,
 * calls the engine, replies. The host is synchronous (the engine is), so while a
 * batch or an analysis runs the worker cannot hear another message; the pool
 * cancels by terminating the worker, never by asking it to stop.
 *
 * Protocol (structured clone; every request has a numeric `id`, and its reply
 * carries the same):
 *
 * | request | reply |
 * |---|---|
 * | `prepare {job, snapshot, settings}` | `ok` |
 * | `batch {job, spec}` | `progress [done, total]` any number of times, then `ok` + `value`: the `BatchOutput` text |
 * | `release {job}` | none |
 * | `analysis {graph, library, body, quick}` | `progress [done, total]` any number of times, then `ok` + `value` |
 * | `drawdown {snapshot, seed, body}` | `ok` + `value`: the `DrawdownBody` text |
 * | `drawdown-compare {snapshot, body}` | `progress [done, total]` any number of times, then `ok` + `value`: the `DrawdownComparison` text |
 * | `analysis-shard {graph, library, body, shard, shards}` | `progress` as above, then `ok` + `value`: this shard's answers (opaque) |
 * | `analysis-finish {graph, library, body, answers}` | `progress` for anything no shard answered, then `ok` + `value`: the outcome |
 * | `calibrate {snapshot, settings}` | `ok` + `value`: `{seconds, iterations}` |
 *
 * `BatchSpec` and `BatchOutput` are opaque strings throughout: their seeds are
 * 64-bit integers, which `JSON.parse` would round.
 */
import { type Engine, joinOutputs, splitSpecs } from "./engine.ts";

export type ComputeRequest =
  | { id: number; kind: "prepare"; job: string; snapshot: string; settings: string }
  | { id: number; kind: "batch"; job: string; spec: string }
  | { id: number; kind: "release"; job: string }
  | { id: number; kind: "analysis"; graph: string; library: string; body: string; quick: boolean }
  | { id: number; kind: "drawdown"; snapshot: string; seed: string; body: string }
  | { id: number; kind: "drawdown-compare"; snapshot: string; body: string }
  | {
      id: number;
      kind: "analysis-shard";
      graph: string;
      library: string;
      body: string;
      shard: number;
      shards: number;
    }
  | { id: number; kind: "analysis-finish"; graph: string; library: string; body: string; answers: string }
  | { id: number; kind: "calibrate"; snapshot: string; settings: string };

export type ComputeReply =
  | { id: number; ok: true; value?: unknown }
  | { id: number; ok: false; error: string }
  | { id: number; progress: [number, number] };

/** What a calibration measured: wall-clock seconds for `iterations` simulations of one batch round. */
export interface CalibrationSample {
  seconds: number;
  iterations: number;
}

export interface ComputeHost {
  /**
   * Handle one request. `stopped` is polled between simulations of an analysis,
   * where a host that can hear a cancel (a test's) uses it to stop early.
   */
  handle(
    request: ComputeRequest,
    reply: (reply: ComputeReply) => void,
    stopped?: () => boolean,
  ): void;
  /** Release everything this host prepared. */
  dispose(): void;
}

function errorText(thrown: unknown): string {
  if (typeof thrown === "string") return thrown;
  return JSON.stringify({
    status: 500,
    code: "internal",
    message: thrown instanceof Error ? thrown.message : String(thrown),
  });
}

export function createComputeHost(
  engine: Engine,
  clock: () => number = () => globalThis.performance.now(),
): ComputeHost {
  const jobs = new Map<string, number>();

  const release = (job: string) => {
    const handle = jobs.get(job);
    if (handle !== undefined) {
      jobs.delete(job);
      engine.release(handle);
    }
  };

  return {
    handle(request, reply, stopped) {
      try {
        switch (request.kind) {
          case "prepare":
            release(request.job);
            jobs.set(request.job, engine.prepare(request.snapshot, request.settings));
            reply({ id: request.id, ok: true });
            return;
          case "batch": {
            const handle = jobs.get(request.job);
            if (handle === undefined) {
              throw JSON.stringify({
                status: 409,
                code: "conflict",
                message: "this worker was not prepared for the run",
              });
            }
            // Progress as the batch runs, as an analysis reports it: a run is
            // one round of a batch per worker, so counting whole batches
            // would sit at zero until they all land together.
            const progress = (done: number, total: number) => {
              reply({ id: request.id, progress: [done, total] });
              return stopped?.() ?? false;
            };
            reply({ id: request.id, ok: true, value: engine.run_batch(handle, request.spec, progress) });
            return;
          }
          case "release":
            release(request.job);
            return;
          case "analysis": {
            const progress = (done: number, total: number) => {
              reply({ id: request.id, progress: [done, total] });
              return stopped?.() ?? false;
            };
            const run = request.quick ? engine.quick_what_if : engine.analysis_run;
            reply({
              id: request.id,
              ok: true,
              value: run(request.graph, request.library, request.body, progress),
            });
            return;
          }
          case "drawdown":
            reply({
              id: request.id,
              ok: true,
              value: engine.drawdown(request.snapshot, request.seed, request.body),
            });
            return;
          case "drawdown-compare": {
            const progress = (done: number, total: number) => {
              reply({ id: request.id, progress: [done, total] });
              return stopped?.() ?? false;
            };
            reply({
              id: request.id,
              ok: true,
              value: engine.drawdown_compare(request.snapshot, request.body, progress),
            });
            return;
          }
          case "analysis-shard": {
            const progress = (done: number, total: number) => {
              reply({ id: request.id, progress: [done, total] });
              return stopped?.() ?? false;
            };
            const { graph, library, body, shard, shards } = request;
            reply({
              id: request.id,
              ok: true,
              value: engine.analysis_shard(graph, library, body, shard, shards, progress),
            });
            return;
          }
          case "analysis-finish": {
            const progress = (done: number, total: number) => {
              reply({ id: request.id, progress: [done, total] });
              return stopped?.() ?? false;
            };
            const { graph, library, body, answers } = request;
            reply({
              id: request.id,
              ok: true,
              value: engine.analysis_finish(graph, library, body, answers, progress),
            });
            return;
          }
          case "calibrate": {
            // One round of the run's first batches, timed: what a real run's
            // opening round costs this device, with the plan prepared once.
            const coordinator = engine.coordinator_new(request.snapshot, request.settings);
            const prepared = engine.prepare(request.snapshot, request.settings);
            try {
              const round = engine.coordinator_next_round(coordinator);
              const specs = round ? splitSpecs(round) : [];
              const started = clock();
              const outputs = specs.map((spec) => engine.run_batch(prepared, spec));
              const seconds = (clock() - started) / 1000;
              engine.coordinator_absorb(coordinator, joinOutputs(outputs));
              const iterations = engine.coordinator_completed(coordinator);
              reply({ id: request.id, ok: true, value: { seconds, iterations } satisfies CalibrationSample });
            } finally {
              engine.coordinator_drop(coordinator);
              engine.release(prepared);
            }
            return;
          }
        }
      } catch (thrown) {
        reply({ id: request.id, ok: false, error: errorText(thrown) });
      }
    },
    dispose() {
      for (const job of [...jobs.keys()]) release(job);
    },
  };
}
