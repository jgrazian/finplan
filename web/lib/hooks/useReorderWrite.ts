"use client";

import { useCallback } from "react";

/**
 * Saving a new order: the write, then the reload that confirms it.
 *
 * A reorder has no form to report into and no field to fail under, so a refusal
 * is said out loud and the list is reloaded — which puts the row back where the
 * server still has it. The rejection is rethrown so `useReorder` releases the
 * order it was holding on screen, rather than showing a move that never landed.
 */
export function useReorderWrite(onChanged: () => void) {
  return useCallback(
    (write: () => Promise<unknown>) =>
      write().then(onChanged, (err: unknown) => {
        alert(
          `The new order could not be saved: ${
            err instanceof Error ? err.message : String(err)
          }`,
        );
        onChanged();
        throw err;
      }),
    [onChanged],
  );
}
