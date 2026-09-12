"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/api/client";
import type { Analysis, AnalysisOutcome, CreateAnalysis } from "@/lib/api/types";
import { isTerminal } from "@/lib/api/types";
import type { GraphSpec } from "@/lib/view/sweep";
import { useAsync } from "./useAsync";

/** How often a queued or running analysis is re-checked. */
const POLL_MS = 500;

export interface AnalysisState<T> {
  /** The job row, while one exists. */
  job: Analysis | undefined;
  /** The finished results, narrowed to the kind this hook was asked for. */
  results: T | undefined;
  /** Queued or executing right now. */
  active: boolean;
  /** A job is in flight, or its results are being fetched. */
  loading: boolean;
  error: string | undefined;
  start: (body: CreateAnalysis) => Promise<void>;
  cancel: () => Promise<void>;
  /** Throw the answer away without starting another — back to the empty state. */
  clear: () => void;
}

/**
 * One analysis of one scenario, from the request to the answer.
 *
 * Same shape as `useRun`, and for the same reason: the API has no push channel,
 * so a started job is polled until it settles and only then are its results —
 * which are much larger than the status row — fetched.
 *
 * `kind` is both the filter and the proof. Results come back tagged, so a hook
 * asked for `"sweep"` will not hand a caller a solve's payload if a stale
 * response arrives after the mode has changed.
 */
export function useAnalysis<K extends AnalysisOutcome["kind"]>(
  scenarioId: number | undefined,
  kind: K,
): AnalysisState<Extract<AnalysisOutcome, { kind: K }>> {
  type Results = Extract<AnalysisOutcome, { kind: K }>;

  const [job, setJob] = useState<Analysis>();
  const [results, setResults] = useState<Results>();
  const [error, setError] = useState<string>();

  // A job belongs to the scenario it was started for. Switching scenarios has
  // to drop it rather than leave another plan's grid on screen under a new name.
  const openedFor = useRef(scenarioId);
  useEffect(() => {
    if (openedFor.current === scenarioId) return;
    openedFor.current = scenarioId;
    setJob(undefined);
    setResults(undefined);
    setError(undefined);
  }, [scenarioId]);

  const jobId = job?.id;
  const status = job?.status;
  const active = job != null && !isTerminal(job.status);

  // Poll while it works.
  useEffect(() => {
    if (jobId == null || status == null || isTerminal(status)) return;
    let live = true;

    const timer = setInterval(async () => {
      try {
        const next = await api.analysis.get(jobId);
        if (!live) return;
        setJob(next);
        if (next.status === "failed") {
          setError(next.error_message ?? "the analysis failed");
        }
      } catch (err) {
        if (live) setError(err instanceof Error ? err.message : String(err));
      }
    }, POLL_MS);

    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [jobId, status]);

  // Read the answer once, when it is there.
  useEffect(() => {
    if (jobId == null || status !== "succeeded") return;
    let live = true;

    api.analysis
      .results(jobId)
      .then((outcome) => {
        if (!live) return;
        // A payload of the wrong kind means the mode changed under a request
        // already in flight; the caller asked a different question by now.
        if (outcome.kind === kind) {
          setResults(outcome as Results);
          setError(undefined);
        }
      })
      .catch((err: Error) => live && setError(err.message));

    return () => {
      live = false;
    };
  }, [jobId, status, kind]);

  const start = useCallback(
    async (body: CreateAnalysis) => {
      if (scenarioId == null) return;
      setError(undefined);
      // The previous answer goes with the previous question: leaving a grid on
      // screen while a differently-shaped one is computed reads as the new one.
      setResults(undefined);
      try {
        setJob(await api.analysis.start(scenarioId, body));
      } catch (err) {
        setJob(undefined);
        setError(err instanceof Error ? err.message : String(err));
      }
    },
    [scenarioId],
  );

  const cancel = useCallback(async () => {
    if (jobId == null) return;
    try {
      setJob(await api.analysis.cancel(jobId));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [jobId]);

  const clear = useCallback(() => {
    setJob(undefined);
    setResults(undefined);
    setError(undefined);
  }, []);

  // A succeeded job with nothing read back yet is still loading: the results
  // are a second request, and they are much larger than the status row.
  const reading = status === "succeeded" && results == null && error == null;

  return {
    job,
    results,
    active,
    loading: active || reading,
    error,
    start,
    cancel,
    clear,
  };
}

/**
 * The scenario's most recent sweep, as the server kept it.
 *
 * A sweep is minutes of CPU and the whole Analysis screen is drawn from it, so
 * unlike the other two analyses its answer outlives the job that produced it:
 * the server stores the newest grid per scenario and this reads it back, which
 * is what makes a page reload land on the graphs rather than on the empty
 * state. `at` is when it was run — the one thing on that screen that may be
 * older than the plan it describes.
 *
 * Held apart from `useAnalysis` rather than folded into it: a restored grid is
 * not a job — it has no id to poll, no progress and nothing to cancel — and the
 * screen chooses between the two, preferring whatever this session has run.
 */
export function useCachedSweep(scenarioId: number | undefined) {
  const { data, loading } = useAsync(
    async () => (scenarioId == null ? null : api.analysis.cachedSweep(scenarioId)),
    [scenarioId],
  );
  const cached = data ?? undefined;

  return {
    results: cached?.results,
    /** When the restored sweep was run, as epoch milliseconds. */
    at: cached ? Date.parse(`${cached.created_at.replace(" ", "T")}Z`) : undefined,
    /** The stored graph layout, unchecked — hand it to `parseLayout`. */
    layout: cached?.layout,
    loading,
  };
}

/**
 * How long an edit waits before it is stored.
 *
 * Most edits are one click and could be written immediately, but turning a
 * surface is a drag that fires on every frame, and a request per frame is a
 * request per frame however small the body is. Short enough that an arrangement
 * is safe by the time anyone reaches for the tab bar.
 */
const LAYOUT_SAVE_MS = 500;

/**
 * The sweep's graph layout: the arrangement on screen, and its storage.
 *
 * Held the same way the swept variables are — the stored layout until someone
 * edits it, the edit after that — so a reload opens on the workspace it closed
 * on. Writes are coalesced, and a pending one is flushed when the screen goes
 * away rather than dropped with it.
 *
 * A failed write costs the arrangement and nothing else: the grid it describes
 * is already stored, and every graph can be put back in a couple of clicks. So
 * it stays quiet rather than interrupting the screen with it.
 */
export function useSweepLayout(
  scenarioId: number | undefined,
  stored: GraphSpec[] | undefined,
) {
  const [edited, setEdited] = useState<GraphSpec[]>();
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const queued = useRef<GraphSpec[]>(undefined);

  const flush = useCallback(() => {
    const graphs = queued.current;
    queued.current = undefined;
    if (scenarioId == null || graphs == null) return;
    void api.analysis.saveSweepLayout(scenarioId, graphs).catch(() => {});
  }, [scenarioId]);

  const setLayout = useCallback(
    (graphs: GraphSpec[]) => {
      setEdited(graphs);
      queued.current = graphs;
      clearTimeout(timer.current);
      timer.current = setTimeout(flush, LAYOUT_SAVE_MS);
    },
    [flush],
  );

  useEffect(
    () => () => {
      clearTimeout(timer.current);
      flush();
    },
    [flush],
  );

  return { layout: edited ?? stored, setLayout };
}

/** The plan's varyable parameters, loaded once per scenario. */
export function useParameters(scenarioId: number | undefined) {
  const { data, loading, error } = useAsync(
    async () => (scenarioId == null ? undefined : api.analysis.parameters(scenarioId)),
    [scenarioId],
  );
  return { parameters: data, loading, error: error?.message };
}
