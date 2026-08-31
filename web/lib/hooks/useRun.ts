"use client";

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api/client";
import type { Results, Run, Scenario as ApiScenario } from "@/lib/api/types";
import { isTerminal } from "@/lib/api/types";
import { serverMonitor } from "@/lib/status/monitor";
import type { ResultsData } from "@/lib/types";
import type { PlanAxis } from "@/lib/view/axis";
import { toResultsData } from "@/lib/view/results";

/** How often a queued or running job is re-checked. */
const POLL_MS = 700;

/** The percentiles the chart's fan needs; requested so the bands exist. */
const PERCENTILES = [0.05, 0.5, 0.95];

export interface RunState {
  run: Run | undefined;
  results: ResultsData | undefined;
  /** True while a run is queued or executing. */
  active: boolean;
  loading: boolean;
  error: string | undefined;
  start: (iterations: number) => Promise<void>;
  cancel: () => Promise<void>;
}

/**
 * The scenario's latest run, and the results behind it.
 *
 * On mount the most recent successful run is adopted, so the Results screen has
 * something to show without the user pressing Run again. Starting a run polls
 * until it reaches a terminal status — the API has no push channel — and only
 * then fetches results, which are large.
 */
/** Everything loaded for one scenario, tagged so a stale load is ignorable. */
interface Loaded {
  scenarioId: number;
  run?: Run;
  raw?: Results;
  error?: string;
  loading: boolean;
}

export function useRun(
  scenario: ApiScenario | undefined,
  axis: PlanAxis | undefined,
): RunState {
  const [loaded, setLoaded] = useState<Loaded>();
  const scenarioId = scenario?.id;

  // State is keyed by scenario rather than cleared when one changes, so the
  // previous scenario's run is never briefly on screen under the new name.
  const current = loaded?.scenarioId === scenarioId ? loaded : undefined;
  const { run, raw, error } = current ?? {};
  const loading = current?.loading ?? scenarioId != null;

  const update = useCallback(
    (id: number, patch: Partial<Loaded>) =>
      setLoaded((prev) =>
        prev?.scenarioId === id
          ? { ...prev, ...patch }
          : { scenarioId: id, loading: false, ...patch },
      ),
    [],
  );

  // Adopt the newest successful run for this scenario.
  useEffect(() => {
    if (scenarioId == null) return;
    let live = true;

    api.runs
      .list(scenarioId)
      .then(async (runs) => {
        const latest = runs.find((r) => r.status === "succeeded");
        if (!live) return;
        if (!latest) return update(scenarioId, { loading: false });
        const results = await api.runs.results(latest.id);
        if (live) update(scenarioId, { run: latest, raw: results, loading: false });
      })
      .catch((err: Error) => live && update(scenarioId, { error: err.message, loading: false }));

    return () => {
      live = false;
    };
  }, [scenarioId, update]);

  const active = run != null && !isTerminal(run.status);

  // A failed run is server state, so it belongs in the status bar rather than
  // only in the screen it happened to be started from. Synced rather than
  // pushed at the moment of failure, so switching scenarios clears it too.
  useEffect(() => {
    if (run?.status === "failed") {
      serverMonitor.runFailed({
        completed: run.completed_iterations,
        total: run.iterations,
        message: run.error_message,
        hasResults: raw != null,
      });
    } else {
      serverMonitor.runCleared();
    }
  }, [run, raw]);

  // Poll while the job is in flight; the API has no push channel.
  useEffect(() => {
    if (scenarioId == null || !run || isTerminal(run.status)) return;
    let live = true;

    const timer = setInterval(async () => {
      try {
        const next = await api.runs.get(run.id);
        if (!live) return;
        if (next.status === "succeeded") {
          // Results are large, so they are fetched once the job is done.
          update(scenarioId, { run: next, raw: await api.runs.results(next.id) });
        } else if (next.status === "failed") {
          update(scenarioId, { run: next, error: next.error_message ?? "the run failed" });
        } else {
          update(scenarioId, { run: next });
        }
      } catch (err) {
        if (live) {
          update(scenarioId, { error: err instanceof Error ? err.message : String(err) });
        }
      }
    }, POLL_MS);

    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [run, scenarioId, update]);

  const start = useCallback(
    async (iterations: number) => {
      if (scenarioId == null) return;
      try {
        // The server compiles the scenario synchronously, so a misconfigured
        // plan reports here rather than as a failed job seconds later.
        const queued = await api.runs.create(scenarioId, {
          iterations,
          percentiles: PERCENTILES,
        });
        update(scenarioId, { run: queued, raw: undefined, error: undefined, loading: false });
      } catch (err) {
        update(scenarioId, {
          error: err instanceof Error ? err.message : String(err),
          loading: false,
        });
      }
    },
    [scenarioId, update],
  );

  const cancel = useCallback(async () => {
    if (scenarioId == null || !run) return;
    try {
      update(scenarioId, { run: await api.runs.cancel(run.id) });
    } catch (err) {
      update(scenarioId, { error: err instanceof Error ? err.message : String(err) });
    }
  }, [run, scenarioId, update]);

  const results =
    raw && scenario && axis ? toResultsData(raw, scenario, axis) : undefined;

  return { run, results, active, loading, error, start, cancel };
}
