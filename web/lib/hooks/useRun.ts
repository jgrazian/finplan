"use client";

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api } from "@/lib/api/client";
import type { Results, Run, Scenario as ApiScenario } from "@/lib/api/types";
import { SERIES, isTerminal } from "@/lib/api/types";
import { preferredRun, resultsAreStale, rememberRunContext } from "@/lib/run/freshness";
import { serverMonitor } from "@/lib/status/monitor";
import type { RunEffort } from "@/components/results";
import type { Percentile, ResultsData } from "@/lib/types";
import type { PlanAxis } from "@/lib/view/axis";
import { toResultsData } from "@/lib/view/results";

/** How often a queued or running job is re-checked. */
const POLL_MS = 700;

/** Representative nominal-terminal ranks offered by the path selector. */
const PERCENTILES = [0.05, 0.5, 0.95];

export interface RunState {
  run: Run | undefined;
  results: ResultsData | undefined;
  /** True while a run is queued or executing. */
  active: boolean;
  stale: boolean;
  markInputsChanged: () => void;
  loading: boolean;
  error: string | undefined;
  /** Which stored path the per-account series, cash flows and ledger describe. */
  percentile: Percentile;
  setPercentile: (percentile: Percentile) => void;
  start: (effort: RunEffort) => Promise<void>;
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
  rawSeries?: Percentile;
  rawScenario?: ApiScenario;
  rawAxis?: PlanAxis;
  invalidated?: boolean;
  error?: string;
  loading: boolean;
}

export function useRun(
  scenario: ApiScenario | undefined,
  axis: PlanAxis | undefined,
): RunState {
  const [loaded, setLoaded] = useState<Loaded>();
  const [percentile, setPercentile] = useState<Percentile>("p50");
  const scenarioId = scenario?.id;
  const editVersions = useRef(new Map<number, number>());
  const contexts = useRef(new Map<string, { rawScenario: ApiScenario | undefined; rawAxis: PlanAxis | undefined }>());
  const [invalidatedPlans, setInvalidatedPlans] = useState<ReadonlyMap<number, boolean>>(new Map());
  const activeScenarioId = useRef(scenarioId);
  useLayoutEffect(() => {
    activeScenarioId.current = scenarioId;
  }, [scenarioId]);

  // State is keyed by scenario rather than cleared when one changes, so the
  // previous scenario's run is never briefly on screen under the new name.
  const current = loaded?.scenarioId === scenarioId ? loaded : undefined;
  const { run, raw, error } = current ?? {};
  const matchingRaw = raw?.scenario_id === scenarioId ? raw : undefined;
  const loading = (current?.loading ?? scenarioId != null) ||
    (!error && matchingRaw != null && current?.rawSeries !== percentile);

  const update = useCallback(
    (id: number, patch: Partial<Loaded>) =>
      setLoaded((prev) =>
        activeScenarioId.current !== id ? prev :
        prev?.scenarioId === id
          ? { ...prev, ...patch }
          : { scenarioId: id, loading: false, ...patch },
      ),
    [],
  );

  // Adopt the newest successful run for this scenario. Its results are left to
  // the effect below, which is also the one that reads them again when the
  // percentile changes.
  useEffect(() => {
    if (scenarioId == null) return;
    let live = true;

    api.runs
      .list(scenarioId)
      .then((runs) => {
        const latest = preferredRun(runs);
        if (!live) return;
        // Still loading if there is a run to read: the effect below has to
        // fetch its results before the screen has anything to draw.
        update(scenarioId, latest ? { run: latest, loading: latest.status === "succeeded" } : { loading: false });
      })
      .catch((err: Error) => live && update(scenarioId, { error: err.message, loading: false }));

    return () => {
      live = false;
    };
  }, [scenarioId, update]);

  const runId = run?.id;
  const runStatus = run?.status;

  /**
   * The finished run's results, along the selected path.
   *
   * Keep the old payload AND its actual path label until the new payload lands.
   * The independent envelope stays visible. Request/run/scenario tags and the
   * effect cleanup prevent late responses from relabelling another path's detail.
   */
  useEffect(() => {
    if (scenarioId == null || runId == null || runStatus !== "succeeded") return;
    let live = true;
    const context = rememberRunContext(contexts.current, run!, { rawScenario: scenario, rawAxis: axis });

    api.runs
      .results(runId, SERIES[percentile])
      .then((raw) => {
        if (live && raw.run_id === runId && raw.scenario_id === scenarioId) {
          update(scenarioId, { raw, rawSeries: percentile, ...context, loading: false, error: undefined });
        }
      })
      .catch((err: Error) => live && update(scenarioId, { error: err.message, loading: false }));

    return () => {
      live = false;
    };
    // Capture display context with this payload, rather than relabeling old results after edits.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scenarioId, runId, runStatus, percentile, update]);

  const active = run != null && !isTerminal(run.status);

  // A failed run is server state, so it belongs in the status bar rather than
  // only in the screen it happened to be started from. Synced rather than
  // pushed at the moment of failure, so switching scenarios clears it too.
  useEffect(() => {
    if (run?.status === "failed") {
      serverMonitor.runFailed({
        completed: run.completed_iterations,
        // A converging run's ceiling is the figure its progress was read
        // against, so it is the one a stopped-at-N message has to name.
        total: run.max_iterations ?? run.iterations,
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
          // Results are large, so they are fetched once the job is done — by
          // the effect that owns them, which this only has to wait for.
          update(scenarioId, { run: next, loading: true });
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
    async (effort: RunEffort) => {
      if (scenarioId == null) return;
      const editVersion = editVersions.current.get(scenarioId) ?? 0;
      try {
        // The server compiles the scenario synchronously, so a misconfigured
        // plan reports here rather than as a failed job seconds later.
        const queued = await api.runs.create(scenarioId, {
          iterations: effort.iterations,
          converge: effort.converge,
          percentiles: PERCENTILES,
        });
        const invalidated = (editVersions.current.get(scenarioId) ?? 0) !== editVersion;
        setInvalidatedPlans((plans) => new Map(plans).set(scenarioId, invalidated));
        rememberRunContext(contexts.current, queued, { rawScenario: scenario, rawAxis: axis }, true);
        update(scenarioId, { run: queued, invalidated, error: undefined, loading: false });
      } catch (err) {
        update(scenarioId, {
          error: err instanceof Error ? err.message : String(err),
          loading: false,
        });
      }
    },
    [scenarioId, scenario, axis, update],
  );

  const cancel = useCallback(async () => {
    if (scenarioId == null || !run) return;
    try {
      update(scenarioId, { run: await api.runs.cancel(run.id) });
    } catch (err) {
      update(scenarioId, { error: err instanceof Error ? err.message : String(err) });
    }
  }, [run, scenarioId, update]);

  const markInputsChanged = useCallback(() => {
    if (scenarioId != null) {
      editVersions.current.set(scenarioId, (editVersions.current.get(scenarioId) ?? 0) + 1);
      setInvalidatedPlans((plans) => new Map(plans).set(scenarioId, true));
      update(scenarioId, { invalidated: true });
    }
  }, [scenarioId, update]);
  const stale = resultsAreStale(scenario, run, scenarioId != null && invalidatedPlans.get(scenarioId) === true);
  const results = matchingRaw && current?.rawScenario && current.rawAxis
    ? toResultsData(matchingRaw, current.rawScenario, current.rawAxis) : undefined;

  return { run, results, active, stale, markInputsChanged, loading, error, percentile, setPercentile, start, cancel };
}
