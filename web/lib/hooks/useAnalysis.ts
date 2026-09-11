"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/api/client";
import type { Analysis, AnalysisOutcome, CreateAnalysis } from "@/lib/api/types";
import { isTerminal } from "@/lib/api/types";
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

/** The plan's varyable parameters, loaded once per scenario. */
export function useParameters(scenarioId: number | undefined) {
  const { data, loading, error } = useAsync(
    async () => (scenarioId == null ? undefined : api.analysis.parameters(scenarioId)),
    [scenarioId],
  );
  return { parameters: data, loading, error: error?.message };
}
