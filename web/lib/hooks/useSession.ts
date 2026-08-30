"use client";

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api/client";
import { ApiError } from "@/lib/api/http";
import type { UserResponse } from "@/lib/api/types";

export interface Session {
  /** `undefined` while the cookie is still being checked, `null` when signed out. */
  user: UserResponse | null | undefined;
  error: string | undefined;
  busy: boolean;
  signIn: (email: string, password: string) => Promise<void>;
  signUp: (email: string, password: string, displayName?: string) => Promise<void>;
  signOut: () => Promise<void>;
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
      (me) => current && setUser(me),
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
      await api.auth.logout();
      setUser(null);
    },
  };
}
