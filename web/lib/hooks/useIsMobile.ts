"use client";

import { useSyncExternalStore } from "react";

/**
 * The phone breakpoint. Below it every two-column grid collapses to one, the
 * tabs move to a bottom bar and drawers become pushed pages. CSS uses the same
 * width in `app/mobile/*.css`; keep the two in step.
 */
export const MOBILE_QUERY = "(max-width: 639px)";

function subscribe(onChange: () => void) {
  const query = window.matchMedia(MOBILE_QUERY);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}

/**
 * Whether the viewport is phone-width. For layout that CSS cannot express on
 * its own — swapping a side-by-side drawer for a pushed page. The server
 * render assumes desktop.
 */
export function useIsMobile(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => window.matchMedia(MOBILE_QUERY).matches,
    () => false,
  );
}
