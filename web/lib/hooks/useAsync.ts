"use client";

import { useCallback, useEffect, useState } from "react";

export interface AsyncState<T> {
  data: T | undefined;
  error: Error | undefined;
  loading: boolean;
  /** Re-runs the loader, e.g. after a mutation. */
  reload: () => void;
}

/**
 * Runs `load` whenever `deps` change and tracks its outcome.
 *
 * Responses from a superseded call are dropped rather than applied, so a fast
 * scenario switch cannot leave the previous scenario's data on screen.
 */
export function useAsync<T>(load: () => Promise<T>, deps: unknown[]): AsyncState<T> {
  const [state, setState] = useState<{ data?: T; error?: Error }>({});
  const [loading, setLoading] = useState(true);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let current = true;
    // Loading mirrors the start of this external request, including dependency
    // changes; retaining the previous data while it loads is intentional.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setLoading(true);
    load().then(
      (data) => {
        if (current) {
          setState({ data });
          setLoading(false);
        }
      },
      (error: Error) => {
        if (current) {
          setState({ error });
          setLoading(false);
        }
      },
    );
    return () => {
      current = false;
    };
    // `load` is rebuilt on every render by design; `deps` is the real key.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, nonce]);

  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { data: state.data, error: state.error, loading, reload };
}
