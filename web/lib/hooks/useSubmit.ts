"use client";

import { useCallback, useState } from "react";

/**
 * Runs one write against the API and tracks it.
 *
 * The server does the real validation — a duplicate name, a bracket table that
 * does not start at zero, an `Age` trigger with no birth date — so its message
 * is what the form shows rather than a second rule set guessing at the same
 * thing on the client.
 */
export function useSubmit(): {
  busy: boolean;
  error: string | undefined;
  run: (action: () => Promise<unknown>, onDone: () => void) => void;
  /** Report a problem the form can see without asking the server. */
  fail: (message: string) => void;
} {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const run = useCallback((action: () => Promise<unknown>, onDone: () => void) => {
    setBusy(true);
    setError(undefined);
    action()
      .then(() => onDone())
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setBusy(false));
  }, []);

  return { busy, error, run, fail: setError };
}
