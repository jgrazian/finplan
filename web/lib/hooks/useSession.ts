"use client";

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api/client";
import { ApiError } from "@/lib/api/http";
import { serverMonitor } from "@/lib/status/monitor";
import type { UserResponse } from "@/lib/api/types";

export interface Session {
  /** `undefined` while the cookie is still being checked, `null` when signed out. */
  user: UserResponse | null | undefined;
  error: string | undefined;
  busy: boolean;
  signIn: (email: string, password: string) => Promise<void>;
  signUp: (email: string, password: string, displayName?: string) => Promise<void>;
  signOut: () => Promise<void>;
  /** Replace the cached user after the account screen saves a change. */
  update: (user: UserResponse) => void;
  /** The account no longer exists; the server has already cleared the cookie. */
  forget: () => void;
}

/**
 * The signed-in user, resolved from the session cookie on mount.
 *
 * A 401 from `/auth/me` is the signed-out state, not a failure — every other
 * error surfaces so a server that is down does not look like a logout.
 */
export function useSession(): Session {
  const [user, setUser] = useState<UserResponse | null | undefined>(undefined);
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let current = true;
    api.auth.me().then(
      (me) => {
        if (!current) return;
        setUser(me);
        // A 401 is only an expiry once there is a session to expire; before
        // this, it is simply the signed-out state.
        serverMonitor.sessionStarted();
      },
      (err: unknown) => {
        if (!current) return;
        setUser(null);
        if (!(err instanceof ApiError && err.isUnauthorized)) {
          setError(err instanceof Error ? err.message : String(err));
        }
      },
    );
    return () => {
      current = false;
    };
  }, []);

  const attempt = useCallback(async (action: () => Promise<UserResponse>) => {
    setBusy(true);
    setError(undefined);
    try {
      setUser(await action());
      serverMonitor.sessionStarted();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  return {
    user,
    error,
    busy,
    signIn: (email, password) => attempt(() => api.auth.login({ email, password })),
    signUp: (email, password, displayName) =>
      attempt(() =>
        api.auth.register({ email, password, display_name: displayName }),
      ),
    signOut: async () => {
      try {
        await api.auth.logout();
      } catch {
        // An expired cookie cannot be logged out; the local session ends
        // either way, which is the whole point of pressing it.
      }
      serverMonitor.sessionEnded();
      setUser(null);
    },
    update: setUser,
    forget: () => {
      serverMonitor.sessionEnded();
      setUser(null);
    },
  };
}
