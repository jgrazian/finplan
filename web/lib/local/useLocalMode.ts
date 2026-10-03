"use client";

import { useEffect, useState } from "react";
import { loadLocalModePolicy, localModeEnabled } from "./flag";

/**
 * Whether local mode is on, settling once the server has been asked.
 *
 * Starts from the client flag alone and corrects itself when `/health` says the
 * operator turned local mode off, so with the flag off this is `false` from the
 * first render and never makes a request.
 */
export function useLocalMode(): boolean {
  const [enabled, setEnabled] = useState(localModeEnabled);
  useEffect(() => {
    let live = true;
    void loadLocalModePolicy().then((allowed) => {
      if (live) setEnabled(allowed);
    });
    return () => {
      live = false;
    };
  }, []);
  return enabled;
}
