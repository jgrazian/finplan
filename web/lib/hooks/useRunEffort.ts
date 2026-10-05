"use client";

import { useCallback, useMemo, useSyncExternalStore } from "react";
import {
  EFFORT_STORAGE_KEY,
  nearestStop,
  parseStoredEffort,
  type RunEffort,
} from "@/lib/view/effort";

/** Fired on this tab when the stop changes; `storage` covers the others. */
const CHANGED = "finplan:run-iterations";

/**
 * The stop for this page when storage refuses (a private window, storage
 * switched off): the slider still moves, it is just not remembered.
 */
let fallback: string | null = null;
let blocked = false;

function read(): string | null {
  if (blocked) return fallback;
  try {
    return localStorage.getItem(EFFORT_STORAGE_KEY);
  } catch {
    blocked = true;
    return fallback;
  }
}

function subscribe(onChange: () => void): () => void {
  const onStorage = (e: StorageEvent) => {
    if (e.key === EFFORT_STORAGE_KEY) onChange();
  };
  window.addEventListener("storage", onStorage);
  window.addEventListener(CHANGED, onChange);
  return () => {
    window.removeEventListener("storage", onStorage);
    window.removeEventListener(CHANGED, onChange);
  };
}

/**
 * The iteration stop the next run asks for: the last one this browser's
 * slider was left on, else the account preference's nearest stop. It is the
 * device's, like the theme, so it survives a reload, a tab change and a
 * remounted workbench, and two open tabs follow each other.
 */
export function useRunEffort(defaultIterations: number): [RunEffort, (effort: RunEffort) => void] {
  const raw = useSyncExternalStore(subscribe, read, () => null);
  const effort = useMemo(
    () => parseStoredEffort(raw) ?? nearestStop(defaultIterations),
    [raw, defaultIterations],
  );
  const set = useCallback((next: RunEffort) => {
    fallback = String(next.iterations);
    try {
      localStorage.setItem(EFFORT_STORAGE_KEY, fallback);
    } catch {
      blocked = true;
    }
    window.dispatchEvent(new Event(CHANGED));
  }, []);
  return [effort, set];
}
