"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/api/client";
import type { CreateRun, Run } from "@/lib/api/types";
import { type OffloadState, offloadActive, runOffload } from "@/lib/local/offload";
import { getLocalRuntime } from "@/lib/local/runtime";

export interface Offload {
  state: OffloadState;
  active: boolean;
  /** Sends one local plan's run to FinPlan's servers. Only ever called from an explicit action. */
  start: (
    scenarioId: number,
    settings: Pick<CreateRun, "iterations" | "converge" | "percentiles">,
  ) => Promise<void>;
  cancel: () => void;
  /** Forget a finished outcome (done, failed, canceled). */
  dismiss: () => void;
}

/**
 * The offload state machine, held for the workbench so its progress shows on
 * the same header bar a local run's does. `onSaved` is called with the run the
 * server's results were stored as, once they are on this device.
 */
export function useOffload(onSaved: (run: Run) => void): Offload {
  const [state, setState] = useState<OffloadState>({ phase: "idle" });
  const abort = useRef<AbortController>(undefined);
  const saved = useRef(onSaved);
  useEffect(() => {
    saved.current = onSaved;
  }, [onSaved]);
  // Leaving the workbench cancels the job: nobody is left to receive its results.
  useEffect(() => () => abort.current?.abort(), []);

  const start = useCallback<Offload["start"]>(async (scenarioId, settings) => {
    const runtime = getLocalRuntime();
    if (!runtime) {
      setState({ phase: "failed", message: "Local plans are not available in this browser." });
      return;
    }
    const controller = new AbortController();
    abort.current = controller;
    const end = await runOffload({
      compute: api.compute,
      runtime,
      scenarioId,
      settings,
      signal: controller.signal,
      onState: setState,
    });
    if (end.phase === "done") saved.current(end.run);
  }, []);

  const cancel = useCallback(() => abort.current?.abort(), []);
  const dismiss = useCallback(() => setState({ phase: "idle" }), []);
  return { state, active: offloadActive(state), start, cancel, dismiss };
}
