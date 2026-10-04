"use client";

import { useEffect, useRef, useState } from "react";
import { api } from "@/lib/api/client";
import { hasLocalBackend, localApi } from "@/lib/api/local";
import { remoteApi } from "@/lib/api/remote";
import { type GuestMigration, migrateGuest } from "@/lib/local/homes";
import type { Session } from "@/lib/hooks/useSession";

/** How long to wait for the local store to come up before giving the guest up as not movable. */
const READY_TIMEOUT_MS = 15_000;
const READY_POLL_MS = 150;

async function localReady(): Promise<boolean> {
  const deadline = Date.now() + READY_TIMEOUT_MS;
  while (!hasLocalBackend()) {
    if (Date.now() > deadline) return false;
    await new Promise((resolve) => setTimeout(resolve, READY_POLL_MS));
  }
  return true;
}

export interface GuestMigrationState {
  /** The guest's plans are being copied to this device. */
  running: boolean;
  /** The outcome, once there is one. */
  result: GuestMigration | undefined;
  /** Clear the outcome once it has been read. */
  dismiss: () => void;
}

/**
 * Spec 17's guests, on their first visit with local mode on (spec 19): copy
 * their plans into the local store, then end the guest session. Once, per page
 * load; a failure leaves the guest exactly as it was.
 */
export function useGuestMigration(session: Session, localMode: boolean): GuestMigrationState {
  const needed = localMode && session.user?.guest === true;
  const [result, setResult] = useState<GuestMigration>();
  const started = useRef(false);
  const forget = useRef(session.forget);
  useEffect(() => {
    forget.current = session.forget;
  }, [session.forget]);

  useEffect(() => {
    if (!needed || started.current) return;
    started.current = true;
    void migrateGuest({
      local: localApi,
      remote: remoteApi,
      logout: () => api.auth.logout(),
      localReady,
      requestId: crypto.randomUUID(),
    }).then((outcome) => {
      setResult(outcome);
      // The server session is over; make the app say so.
      if (outcome.status !== "failed") forget.current();
    });
  }, [needed]);

  return {
    running: needed && result === undefined,
    result,
    dismiss: () => setResult(undefined),
  };
}
