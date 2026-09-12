"use client";

import { useEffect, useSyncExternalStore } from "react";
import { APPEARANCE_KEY, type Appearance, applyAppearance } from "./palettes";

/**
 * Put the account's palette on the page, and keep it there.
 *
 * Three jobs, and they are one effect because they share a dependency:
 *
 *  - stamp `data-theme` / `data-accent` on `<html>`, which is all the
 *    stylesheet needs;
 *  - remember the choice locally, so the script in `app/layout.tsx` can stamp
 *    the same thing before the next first paint rather than flashing the
 *    default palette while the session loads;
 *  - follow the machine while the mode is `system` — `prefers-color-scheme`
 *    can change under a running tab, at sunset or at a keystroke.
 */
export function useAppearance(appearance: Appearance): void {
  const { mode, accent } = appearance;

  useEffect(() => {
    const root = document.documentElement;
    const stamp = () => applyAppearance(root, { mode, accent });
    stamp();

    try {
      localStorage.setItem(APPEARANCE_KEY, JSON.stringify({ mode, accent }));
    } catch {
      // Private windows and blocked site data: the palette still applies, the
      // next load just pays one frame of the default before the session lands.
    }

    if (mode !== "system") return;
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    query.addEventListener("change", stamp);
    return () => query.removeEventListener("change", stamp);
  }, [mode, accent]);
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

function subscribeToScheme(onChange: () => void): () => void {
  const query = window.matchMedia(DARK_QUERY);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}

/**
 * Which ground a mode actually lands on, `system` included.
 *
 * An external store rather than state in an effect: the machine's answer is
 * exactly that, something outside React that changes on its own. The server
 * snapshot is `light`, and the first client render corrects it — a guess would
 * be a hydration mismatch, and there is nothing to guess from.
 *
 * Only the controls need this. Applying the palette does not: `useAppearance`
 * re-stamps the attribute from the listener, so the machine changing its mind
 * repaints the page without re-rendering the app.
 */
export function useResolvedMode(mode: Appearance["mode"]): "light" | "dark" {
  const prefersDark = useSyncExternalStore(
    subscribeToScheme,
    () => window.matchMedia(DARK_QUERY).matches,
    () => false,
  );
  if (mode !== "system") return mode;
  return prefersDark ? "dark" : "light";
}
