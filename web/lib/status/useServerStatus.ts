"use client";

import { useSyncExternalStore } from "react";
import { type ServerStatus, serverMonitor } from "./monitor";

/**
 * The live server status.
 *
 * `useSyncExternalStore` rather than a context: the store is written from
 * `http.ts`, which is not a component and has no provider to reach.
 */
export function useServerStatus(): ServerStatus {
  return useSyncExternalStore(
    serverMonitor.subscribe,
    serverMonitor.snapshot,
    serverMonitor.initial,
  );
}
