/**
 * Running a local plan's simulation on FinPlan's servers (spec 19, "Server
 * offload"): the state machine behind the "Run on FinPlan servers" offer.
 *
 * The plan leaves the device for this, so it is only ever started by an
 * explicit per-run action. The flow is: snapshot the plan locally, post the
 * snapshot, poll the job, store the results locally as a run with
 * `engine: "server"`, then delete the job. The server keeps nothing of the plan.
 *
 * The compute client and the local runtime are passed in, so the machine is
 * tested with stand-ins and this file reaches no `fetch` itself.
 */
import type { ComputeRun } from "../api/generated/ComputeRun.ts";
import type { ComputeRunCreated } from "../api/generated/ComputeRunCreated.ts";
import type { ComputeRunRequest } from "../api/generated/ComputeRunRequest.ts";
import type { ComputeRunSettings } from "../api/generated/ComputeRunSettings.ts";
import type { CreateRun, Run } from "../api/types.ts";
import type { LocalRuntime } from "./runtime.ts";

/** The remote-only compute routes (`api.compute` in `lib/api/client.ts`). */
export interface ComputeClient {
  create: (body: ComputeRunRequest) => Promise<ComputeRunCreated>;
  get: (id: number) => Promise<ComputeRun>;
  /** Cancels a running job and deletes a finished one. */
  remove: (id: number) => Promise<void>;
}

export type OffloadState =
  | { phase: "idle" }
  /** The snapshot is being made and sent. */
  | { phase: "sending" }
  | { phase: "queued" }
  | { phase: "running"; completed: number; total: number }
  /** The server finished; the results are being stored on this device. */
  | { phase: "saving" }
  | { phase: "done"; run: Run }
  | { phase: "canceled" }
  /** The engine versions differ: this tab is older than the server. */
  | { phase: "stale"; message: string }
  | { phase: "failed"; message: string };

export const RELOAD_TO_UPDATE = "Reload FinPlan to update";

const POLL_MS = 700;
/** Consecutive failed polls tolerated before giving up: a blip is not a failed run. */
const MAX_POLL_FAILURES = 4;

/** The run subset the server takes, from the settings the run was asked with. */
export function toComputeSettings(
  settings: Pick<CreateRun, "iterations" | "converge" | "percentiles"> & { seed?: number | null },
): ComputeRunSettings {
  return {
    iterations: settings.iterations,
    percentiles: settings.percentiles,
    converge: settings.converge,
    ...(settings.seed != null ? { seed: settings.seed } : {}),
  };
}

export interface OffloadOptions {
  compute: ComputeClient;
  runtime: Pick<LocalRuntime, "snapshot" | "saveServerRun">;
  scenarioId: number;
  settings: Pick<CreateRun, "iterations" | "converge" | "percentiles"> & { seed?: number | null };
  onState: (state: OffloadState) => void;
  /** Aborting cancels the job on the server. */
  signal?: AbortSignal;
  /** Replaceable so a test does not wait. */
  sleep?: (ms: number) => Promise<void>;
  pollMs?: number;
}

const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

const statusOf = (error: unknown): number | undefined => {
  const status = (error as { status?: unknown } | null)?.status;
  return typeof status === "number" ? status : undefined;
};

const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * Runs the offload to its end and returns the final state, reporting each
 * step through `onState`. Never throws: every outcome is a state.
 */
export async function runOffload(options: OffloadOptions): Promise<OffloadState> {
  const { compute, runtime, scenarioId, settings, signal } = options;
  const sleep = options.sleep ?? wait;
  const pollMs = options.pollMs ?? POLL_MS;
  const finish = (state: OffloadState): OffloadState => {
    options.onState(state);
    return state;
  };

  options.onState({ phase: "sending" });
  let jobId: number;
  let snapshot: Awaited<ReturnType<LocalRuntime["snapshot"]>>;
  try {
    snapshot = await runtime.snapshot(scenarioId);
    if (signal?.aborted) return finish({ phase: "canceled" });
    jobId = (
      await compute.create({
        snapshot: snapshot.json,
        model_version: snapshot.modelVersion,
        settings: toComputeSettings(settings),
      })
    ).id;
  } catch (error) {
    // 409 is the one answer to a model mismatch: the bundle is older than the server.
    if (statusOf(error) === 409) return finish({ phase: "stale", message: RELOAD_TO_UPDATE });
    return finish({ phase: "failed", message: messageOf(error) });
  }

  const cancel = async (): Promise<OffloadState> => {
    // DELETE cancels a running job; a failure to say so changes nothing the person sees.
    await compute.remove(jobId).catch(() => undefined);
    return finish({ phase: "canceled" });
  };

  let failures = 0;
  for (;;) {
    if (signal?.aborted) return cancel();
    let job: ComputeRun;
    try {
      job = await compute.get(jobId);
      failures = 0;
    } catch (error) {
      failures += 1;
      // A job that is gone (404) or a refused session cannot be waited for.
      const status = statusOf(error);
      if (failures >= MAX_POLL_FAILURES || status === 404 || status === 401) {
        return finish({ phase: "failed", message: messageOf(error) });
      }
      await sleep(pollMs);
      continue;
    }
    if (signal?.aborted) return cancel();

    switch (job.status) {
      case "queued":
        options.onState({ phase: "queued" });
        break;
      case "running":
        options.onState({
          phase: "running",
          completed: job.completed_iterations,
          total: job.iterations,
        });
        break;
      case "canceled":
        return finish({ phase: "canceled" });
      case "failed":
        await compute.remove(jobId).catch(() => undefined);
        return finish({ phase: "failed", message: job.error ?? "The run failed on the server." });
      case "succeeded": {
        options.onState({ phase: "saving" });
        let run: Run;
        try {
          run = await runtime.saveServerRun(scenarioId, {
            settings,
            inputHash: snapshot.hash,
            modelVersion: snapshot.modelVersion,
            results: job.results,
          });
        } catch (error) {
          // The server keeps the results for an hour; leave the job for that hour.
          return finish({
            phase: "failed",
            message: `The run finished but its results could not be saved on this device: ${messageOf(error)}`,
          });
        }
        // Stored here now, so the server's copy has done its job.
        await compute.remove(jobId).catch(() => undefined);
        return finish({ phase: "done", run });
      }
    }
    await sleep(pollMs);
  }
}

/** True while the offload is between "sending" and its end. */
export function offloadActive(state: OffloadState): boolean {
  return (
    state.phase === "sending" ||
    state.phase === "queued" ||
    state.phase === "running" ||
    state.phase === "saving"
  );
}

/**
 * The offload as a `Run` for the header's progress bar, which reads one. It is
 * never stored or listed: the run that is, comes from `saveServerRun`.
 */
export function offloadProgressRun(
  state: OffloadState,
  scenarioId: number,
  asked: { iterations: number },
): Run | undefined {
  if (!offloadActive(state)) return undefined;
  const running = state.phase === "running" || state.phase === "saving";
  const completed =
    state.phase === "running" ? state.completed : state.phase === "saving" ? asked.iterations : 0;
  const total = state.phase === "running" ? state.total : asked.iterations;
  return {
    id: -1,
    scenario_id: scenarioId,
    status: running ? "running" : "queued",
    iterations: total,
    completed_iterations: completed,
    converge: false,
    max_iterations: null,
    seed: null,
    input_hash: null,
    model_version: null,
    error_message: null,
    created_at: "",
    started_at: null,
    finished_at: null,
  };
}
