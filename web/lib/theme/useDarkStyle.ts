"use client";

import { useCallback, useSyncExternalStore } from "react";
import { DARK_STYLE_KEY, type DarkStyle, applyDarkStyle, parseDarkStyle } from "./palettes";

/** Fired on this tab when the style changes; `storage` covers the others. */
const CHANGED = "finplan:dark-style";

function read(): DarkStyle {
  try {
    return parseDarkStyle(localStorage.getItem(DARK_STYLE_KEY));
  } catch {
    return "charcoal";
  }
}

function subscribe(onChange: () => void): () => void {
  const onStorage = (e: StorageEvent) => {
    if (e.key !== DARK_STYLE_KEY) return;
    applyDarkStyle(document.documentElement, read());
    onChange();
  };
  window.addEventListener("storage", onStorage);
  window.addEventListener(CHANGED, onChange);
  return () => {
    window.removeEventListener("storage", onStorage);
    window.removeEventListener(CHANGED, onChange);
  };
}

/**
 * The device's dark style, and a setter that applies it at once — there is no
 * Save for it, because there is nothing on the account to save it to. The
 * first paint is stamped by the script in `app/layout.tsx`.
 */
export function useDarkStyle(): [DarkStyle, (style: DarkStyle) => void] {
  const style = useSyncExternalStore(subscribe, read, () => "charcoal" as const);
  const set = useCallback((next: DarkStyle) => {
    try {
      localStorage.setItem(DARK_STYLE_KEY, next);
    } catch {
      // Blocked storage: the page still turns, it just will not be remembered.
    }
    applyDarkStyle(document.documentElement, next);
    window.dispatchEvent(new Event(CHANGED));
  }, []);
  return [style, set];
}
