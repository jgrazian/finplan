"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/api/client";
import { planApiFor } from "@/lib/nav/api";
import { ApiError } from "@/lib/api/http";
import { loadLocalModePolicy } from "@/lib/local/flag";
import { serverMonitor } from "@/lib/status/monitor";
import type { PlanArchive } from "@/lib/api/generated/PlanArchive";
import type { UserResponse } from "@/lib/api/types";
import { GUEST_PLAN_PREFIX } from "@/lib/view/guest";

/**
 * A guest's plans waiting on a decision after they signed in to an account:
 * bring them across, or let them go with the guest.
 */
export interface Adoption {
  /** How many plans the guest's archive holds. */
  plans: number;
  busy: boolean;
  /** Why the import was refused, e.g. the account has no free plan slot. */
  error?: string;
}

export interface Session {
  /** `undefined` while the cookie is still being checked, `null` when signed out. */
  user: UserResponse | null | undefined;
  error: string | undefined;
  busy: boolean;
  signIn: (email: string, password: string) => Promise<void>;
  signUp: (
    email: string,
    password: string,
    passwordConfirmation: string,
    displayName?: string,
  ) => Promise<void>;
  /** Turn the guest into an account in place; its plans stay where they are. */
  claim: (
    email: string,
    password: string,
    passwordConfirmation: string,
    displayName?: string,
  ) => Promise<void>;
  signOut: () => Promise<void>;
  /** Set after a guest signs in with plans to bring along; the UI asks. */
  adoption: Adoption | undefined;
  /** Import the guest's held archive into the account just signed in to. */
  adopt: () => Promise<void>;
  /** Let the guest's plans go. */
  declineAdoption: () => void;
  /** Bumped when the account's data changed under the screen: remount on it. */
  revision: number;
  /** Forget the last error, e.g. when a form is opened afresh. */
  clearError: () => void;
  /** Replace the cached user after the account screen saves a change. */
  update: (user: UserResponse) => void;
  /** The account no longer exists; the server has already cleared the cookie. */
  forget: () => void;
}

/**
 * The signed-in user, resolved from the session cookie on mount.
 *
 * A 401 from `/auth/me` starts a guest session (spec 17) so a first visit
 * lands in the workbench, unless local mode is on (spec 19): then there is no
 * guest, and the visitor is signed out with local plans. Only when guest access is refused — guest access off (403)
 * or rate limited (429) — is it the signed-out state, which shows the sign-in
 * form as before. Every other error surfaces so a server that is down does
 * not look like a logout.
 */
export function useSession(): Session {
  const [user, setUser] = useState<UserResponse | null | undefined>(undefined);
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [adoption, setAdoption] = useState<Adoption>();
  const [revision, setRevision] = useState(0);
  // The guest's archive is read before the sign-in replaces its session, and
  // held here, never in state or storage, until it is adopted or declined.
  const held = useRef<PlanArchive>(undefined);
  // One key per adoption, so a retry after a failure cannot import twice.
  const adoptionKey = useRef<string>(undefined);

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
        if (!(err instanceof ApiError && err.isUnauthorized)) {
          setUser(null);
          setError(err instanceof Error ? err.message : String(err));
          return;
        }
        // Local mode (spec 19) retires guest sessions: plans live in this
        // browser, so a visitor needs no server identity to start. The answer
        // is the signed-out state, which the app opens on local plans.
        void loadLocalModePolicy().then((local) => {
          if (!current) return;
          if (local) {
            setUser(null);
            return;
          }
          startGuest();
        });
      },
    );
    function startGuest() {
      api.auth.guest().then(
        (guest) => {
          if (!current) return;
          setUser(guest);
          // So a later 401 still means the guest's session expired.
          serverMonitor.sessionStarted();
        },
        (refused: unknown) => {
          if (!current) return;
          setUser(null);
          // Guest access off or rate limited: the sign-in form is the floor.
          if (!(refused instanceof ApiError && (refused.status === 403 || refused.status === 429))) {
            setError(refused instanceof Error ? refused.message : String(refused));
          }
        },
      );
    }
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
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      return false;
    } finally {
      setBusy(false);
    }
  }, []);

  const clearError = useCallback(() => setError(undefined), []);

  const signIn = async (email: string, password: string) => {
    // Signing in replaces the guest, and the server deletes it afterwards, so
    // what it built has to be in hand first.
    let archive: PlanArchive | undefined;
    if (user?.guest) {
      setBusy(true);
      setError(undefined);
      try {
        archive = await planApiFor("cloud").archives.exportAll();
      } catch {
        setError("Could not read your guest plan, so you have not been signed in. Try again.");
        setBusy(false);
        return;
      }
      setBusy(false);
    }
    if (!(await attempt(() => api.auth.login({ email, password })))) return;
    if (archive && archive.plans.length > 0) {
      held.current = archive;
      adoptionKey.current = crypto.randomUUID();
      setAdoption({ plans: archive.plans.length, busy: false });
    }
    // The account's data replaces the guest's whatever the answer.
    setRevision((n) => n + 1);
  };

  const adopt = async () => {
    const archive = held.current;
    const key = adoptionKey.current;
    if (!archive || !key) return;
    const plans = archive.plans.length;
    setAdoption({ plans, busy: true });
    try {
      await planApiFor("cloud").archives.import({
        archive,
        name_prefix: GUEST_PLAN_PREFIX,
        request_id: key,
        from_guest: true,
      });
      held.current = undefined;
      adoptionKey.current = undefined;
      setAdoption(undefined);
      setRevision((n) => n + 1);
    } catch (err) {
      // Shown, not swallowed: a full plan slot is the likely one, and the
      // choice then is to try again after deleting a plan, or let it go.
      setAdoption({ plans, busy: false, error: err instanceof Error ? err.message : String(err) });
    }
  };

  const declineAdoption = () => {
    held.current = undefined;
    adoptionKey.current = undefined;
    setAdoption(undefined);
  };

  return {
    user,
    error,
    busy,
    signIn,
    signUp: async (email, password, passwordConfirmation, displayName) => {
      await attempt(() =>
        api.auth.register({
          email,
          password,
          password_confirmation: passwordConfirmation,
          display_name: displayName,
        }),
      );
    },
    claim: async (email, password, passwordConfirmation, displayName) => {
      // The user id does not change, but the account does: remount on it.
      if (
        await attempt(() =>
          api.auth.claimGuest({
            email,
            password,
            password_confirmation: passwordConfirmation,
            display_name: displayName,
          }),
        )
      ) {
        setRevision((n) => n + 1);
      }
    },
    adoption,
    adopt,
    declineAdoption,
    revision,
    clearError,
    // A guest has no sign-out in the header; this is what the expired-session
    // dialog and the status bar reach, and it ends the session either way.
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
