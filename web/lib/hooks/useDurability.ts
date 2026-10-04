"use client";

import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { useLocalRuntime } from "@/lib/hooks/useLocalRuntime";
import {
  type KeyValue,
  type PersistAnswer,
  SAFARI_DISMISSED_KEY,
  backupDue,
  isInstalled,
  recordedPersist,
  requestPersistOnce,
  showSafariNudge,
} from "@/lib/local/durability";
import type { LocalPlanMeta } from "@/lib/local/runtime";

function storage(): KeyValue | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

const noSubscription = () => () => undefined;

export interface Durability {
  /** The plan's export bookkeeping; undefined for a cloud plan or before the runtime answers. */
  meta: LocalPlanMeta | undefined;
  /** Five saved edits and a fortnight since the last export: offer the backup. */
  backupDue: boolean;
  /** The browser refused to keep this site's storage. */
  persistDenied: boolean;
  /** Call after each saved edit to a local plan; asks for persistence the first time only. */
  noteSavedEdit: () => void;
  /** Safari without the site installed, and not dismissed. */
  safariNudge: boolean;
  dismissSafariNudge: () => void;
  /** Re-read `meta`, e.g. after an export. */
  refresh: () => void;
}

/**
 * The durability state of the open plan. `localId` is undefined for a cloud
 * plan, which has none of it.
 */
export function useDurability(
  localId: number | undefined,
  plan: { createdAt?: string; updatedAt?: string },
): Durability {
  const runtime = useLocalRuntime();
  // `due` is decided when the meta arrives: the clock is not read while rendering.
  const [read, setRead] = useState<{ meta?: LocalPlanMeta; due: boolean }>({ due: false });
  const [nonce, setNonce] = useState(0);
  const [persist, setPersist] = useState<PersistAnswer | undefined>(() => recordedPersist(storage()));
  const [dismissed, setDismissed] = useState(() => {
    try {
      return storage()?.getItem(SAFARI_DISMISSED_KEY) === "1";
    } catch {
      return false;
    }
  });

  useEffect(() => {
    if (localId == null || !runtime) return;
    let live = true;
    runtime.planMeta(localId).then(
      (next) =>
        live &&
        setRead({
          meta: next,
          due: backupDue({ meta: next, createdAt: plan.createdAt, now: Date.now() }),
        }),
      () => live && setRead({ due: false }),
    );
    return () => {
      live = false;
    };
  }, [runtime, localId, plan.createdAt, plan.updatedAt, nonce]);

  const noteSavedEdit = useCallback(() => {
    void requestPersistOnce(runtime, storage()).then((answer) => answer && setPersist(answer));
  }, [runtime]);

  const safariNudge = useSyncExternalStore(
    noSubscription,
    () =>
      showSafariNudge({
        userAgent: navigator.userAgent,
        installed: isInstalled({
          matchMedia: window.matchMedia?.bind(window),
          navigator: navigator as { standalone?: boolean },
        }),
        dismissed: false,
      }),
    () => false,
  );

  return {
    meta: localId == null ? undefined : read.meta,
    backupDue: localId != null && read.due,
    persistDenied: persist === "denied",
    noteSavedEdit,
    safariNudge: safariNudge && !dismissed,
    dismissSafariNudge: () => {
      setDismissed(true);
      try {
        storage()?.setItem(SAFARI_DISMISSED_KEY, "1");
      } catch {
        // Storage blocked: the nudge comes back next visit.
      }
    },
    refresh: () => setNonce((n) => n + 1),
  };
}
