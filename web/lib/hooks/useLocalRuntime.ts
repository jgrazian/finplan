"use client";

import { useSyncExternalStore } from "react";
import { type LocalRuntime, getLocalRuntime, subscribeLocalRuntime } from "@/lib/local/runtime";

/**
 * The local engine's runtime, or `undefined` until it has registered (local
 * mode off, the workers still loading, a browser without IndexedDB). Screens
 * render what depends on it only once it is here.
 */
export function useLocalRuntime(): LocalRuntime | undefined {
  return useSyncExternalStore(subscribeLocalRuntime, getLocalRuntime, () => undefined);
}
