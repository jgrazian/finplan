"use client";

import { useCallback, useState } from "react";

export interface YearFocus {
  /** Where the pointer is on the chart, or null when it is off the plot. */
  hoverIndex: number | null;
  /** The year held on screen after the pointer leaves, or null when none is. */
  pinnedIndex: number | null;
  /**
   * The year every panel reads. The pointer wins while it is on the plot — a
   * pin is what the reading falls back to, not a lock — and the last year of
   * the plan stands in when there is neither.
   */
  index: number;
  /** Which of those three the index came from, said in the panel's own words. */
  hint: string;
  setHover: (index: number | null) => void;
  pin: (index: number) => void;
  unpin: () => void;
}

/**
 * The year the chart and the rail are both pointed at.
 *
 * Lifted out of the chart because two panels read it: hovering the plot is how
 * the account breakdown beside it is scrubbed, and pinning is what lets a
 * reader take their pointer off the chart and still study the year they found.
 */
export function useYearFocus(count: number): YearFocus {
  const [hoverIndex, setHover] = useState<number | null>(null);
  const [pinnedIndex, setPinned] = useState<number | null>(null);

  const unpin = useCallback(() => setPinned(null), []);

  // A run of a different length — another scenario, a shorter horizon — can
  // outlive an index taken from the last one, so every index is clamped rather
  // than trusted.
  const last = Math.max(count - 1, 0);
  const clamp = (i: number | null) => (i == null ? null : Math.min(Math.max(i, 0), last));
  const hover = clamp(hoverIndex);
  const pinned = clamp(pinnedIndex);

  return {
    hoverIndex: hover,
    pinnedIndex: pinned,
    index: hover ?? pinned ?? last,
    hint:
      hover != null
        ? "previewing — click to pin"
        : pinned != null
          ? "pinned"
          : "hover the chart, click to pin",
    setHover,
    pin: setPinned,
    unpin,
  };
}
