"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/api/client";
import type { WhatIfEntry } from "@/lib/api/types";
import { useAsync } from "./useAsync";

/**
 * How long a stack edit waits before it is stored. Stepping a value is a burst
 * of presses, and one write for the burst is enough.
 */
const STACK_SAVE_MS = 500;

/** One empty stack, so an unreadable one is the same array render to render. */
const EMPTY: WhatIfEntry[] = [];

/**
 * The scenario's What-if override stack: the stored one until it is edited,
 * the edit after that, written back behind a short debounce.
 *
 * Held the way the sweep layout is — a pending write is flushed when the
 * screen goes away rather than dropped with it — and quiet on failure for the
 * same reason: the stack is scratch work, and losing an edit to a failed write
 * costs a click, not the plan.
 */
export function useWhatIfStack(scenarioId: number) {
  const stored = useAsync(() => api.whatIf.get(scenarioId), [scenarioId]);
  const [edited, setEdited] = useState<{ scenarioId: number; entries: WhatIfEntry[] }>();
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const queued = useRef<{ scenarioId: number; entries: WhatIfEntry[] }>(undefined);

  const flush = useCallback(() => {
    clearTimeout(timer.current);
    const pending = queued.current;
    queued.current = undefined;
    if (pending == null) return;
    void api.whatIf.save(pending.scenarioId, { entries: pending.entries }).catch(() => {});
  }, []);

  const setEntries = useCallback(
    (entries: WhatIfEntry[]) => {
      setEdited({ scenarioId, entries });
      queued.current = { scenarioId, entries };
      clearTimeout(timer.current);
      timer.current = setTimeout(flush, STACK_SAVE_MS);
    },
    [flush, scenarioId],
  );

  /**
   * Empty the stack on screen without writing it: for after an apply, when
   * the server has already cleared its own copy and a queued write of the old
   * stack would put it back.
   */
  const forget = useCallback(() => {
    clearTimeout(timer.current);
    queued.current = undefined;
    setEdited({ scenarioId, entries: [] });
  }, [scenarioId]);

  // Flush on the way out, and when the scenario changes under a pending edit.
  useEffect(() => () => flush(), [flush, scenarioId]);

  const entries =
    edited?.scenarioId === scenarioId
      ? edited.entries
      : stored.data
        ? stored.data.entries
        : // An unreadable stack starts empty rather than blocking the screen.
          stored.error
          ? EMPTY
          : undefined;

  return {
    /** Undefined until the stored stack has been read. */
    entries,
    error: stored.error?.message,
    setEntries,
    forget,
  };
}
