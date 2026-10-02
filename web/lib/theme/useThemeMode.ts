"use client";

import { useCallback, useEffect, useSyncExternalStore } from "react";
import {
  APPEARANCE_KEY,
  type ThemeMode,
  applyThemeMode,
  parseThemeMode,
  serializeThemeMode,
} from "./palettes";

/** Fired on this tab when the mode changes; `storage` covers the others. */
const CHANGED = "finplan:theme-mode";
const DARK_QUERY = "(prefers-color-scheme: dark)";

function read(): ThemeMode {
  try {
    return parseThemeMode(localStorage.getItem(APPEARANCE_KEY));
  } catch {
    return "system";
  }
}

function subscribe(onChange: () => void): () => void {
  const onStorage = (e: StorageEvent) => {
    if (e.key === APPEARANCE_KEY) onChange();
  };
  window.addEventListener("storage", onStorage);
  window.addEventListener(CHANGED, onChange);
  return () => {
    window.removeEventListener("storage", onStorage);
    window.removeEventListener(CHANGED, onChange);
  };
}

/**
 * The device's mode, and a setter that applies it at once — like the dark
 * style, there is no Save, because there is nothing on the account to save it
 * to. The first paint is stamped by the script in `app/layout.tsx`.
 */
export function useThemeMode(): [ThemeMode, (mode: ThemeMode) => void] {
  const mode = useSyncExternalStore(subscribe, read, () => "system" as const);
  const set = useCallback((next: ThemeMode) => {
    try {
      localStorage.setItem(APPEARANCE_KEY, serializeThemeMode(next));
    } catch {
      // Blocked storage: the page still turns, it just will not be remembered.
    }
    applyThemeMode(document.documentElement, next);
    window.dispatchEvent(new Event(CHANGED));
  }, []);
  return [mode, set];
}

/**
 * Keep `<html>` on the device's mode: re-stamp whenever it changes (here or in
 * another tab), and follow the machine while it is `system` —
 * `prefers-color-scheme` can change under a running tab, at sunset or at a
 * keystroke. Mounted once, at the root.
 */
export function useApplyThemeMode(): void {
  const [mode] = useThemeMode();
  useEffect(() => {
    const root = document.documentElement;
    const stamp = () => applyThemeMode(root, mode);
    stamp();
    if (mode !== "system") return;
    const query = window.matchMedia(DARK_QUERY);
    query.addEventListener("change", stamp);
    return () => query.removeEventListener("change", stamp);
  }, [mode]);
}

function subscribeToScheme(onChange: () => void): () => void {
  const query = window.matchMedia(DARK_QUERY);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}

/**
 * Which ground a mode actually lands on, `system` included — for the controls'
 * "dark right now" note. An external store because the machine's answer is
 * something outside React that changes on its own; the server snapshot is
 * `light`, and the first client render corrects it.
 */
export function useResolvedMode(mode: ThemeMode): "light" | "dark" {
  const prefersDark = useSyncExternalStore(
    subscribeToScheme,
    () => window.matchMedia(DARK_QUERY).matches,
    () => false,
  );
  if (mode !== "system") return mode;
  return prefersDark ? "dark" : "light";
}
