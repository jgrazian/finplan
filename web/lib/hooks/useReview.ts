"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "@/lib/api/client";
import type { Review } from "@/lib/api/suggestions";
import type { LocalReviewApi } from "@/lib/engine/review";

export interface ReviewState {
  /** `null` when this scenario has never been reviewed; undefined while loading. */
  review: Review | null | undefined;
  error: Error | undefined;
  /** Re-reads the stored review, e.g. after a note is applied or dismissed. */
  reload: () => void;
  /** Writes a fresh review of `runId` (the latest succeeded run when null). */
  reviewRun: (runId: number | null) => Promise<void>;
  reviewing: boolean;
}

/** How often a review whose model-written pass is running is read again. */
const POLL_MS = 3_000;

/**
 * The scenario's last review, shared by the Review tab and the "review note"
 * hooks on Portfolio and Plan, so both show the same notes.
 */
export function useReview(
  rawScenarioId: number | undefined,
  /**
   * Whether the plan's home has reviews at all. A local plan has none to read
   * (they are written by the server against a stored plan, and a local plan's
   * id means nothing there), so it is never asked: the board is simply empty.
   */
  available = true,
  /**
   * The review written on this device, for a local plan: rule-based notes with
   * no model, in place of the server's. Absent for a cloud plan.
   */
  local?: LocalReviewApi,
): ReviewState {
  const scenarioId = available ? rawScenarioId : undefined;
  const reviews = useMemo(
    () => (local ? { get: local.get, run: local.run } : { get: api.review.get, run: api.review.run }),
    [local],
  );
  const [loaded, setLoaded] = useState<{ id?: number; review?: Review | null; error?: Error }>({});
  const [nonce, setNonce] = useState(0);
  const [reviewing, setReviewing] = useState(false);

  useEffect(() => {
    if (scenarioId == null) return;
    let live = true;
    reviews.get(scenarioId).then(
      (review) => live && setLoaded({ id: scenarioId, review: review ?? null }),
      (error: Error) => live && setLoaded({ id: scenarioId, error }),
    );
    return () => {
      live = false;
    };
  }, [scenarioId, nonce, reviews]);

  const reload = useCallback(() => setNonce((n) => n + 1), []);

  // While the model-written pass runs, read the review again until it lands.
  const running = loaded.id === scenarioId && loaded.review?.ai?.status === "running";
  useEffect(() => {
    if (scenarioId == null || !running) return;
    let live = true;
    const timer = setInterval(() => {
      reviews.get(scenarioId).then(
        (review) => live && setLoaded((previous) => (previous.id === scenarioId ? { id: scenarioId, review: review ?? null } : previous)),
        // A failed poll is retried on the next tick; the board keeps what it has.
        () => undefined,
      );
    }, POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [scenarioId, running, reviews]);

  const reviewRun = useCallback(
    async (runId: number | null) => {
      if (scenarioId == null) return;
      setReviewing(true);
      try {
        const review = await reviews.run(scenarioId, runId);
        setLoaded({ id: scenarioId, review });
      } catch (error) {
        setLoaded((previous) => ({ ...previous, id: scenarioId, error: error as Error }));
      } finally {
        setReviewing(false);
      }
    },
    [scenarioId, reviews],
  );

  // A superseded scenario's review must not describe this one.
  const current = loaded.id === scenarioId ? loaded : {};
  return { review: current.review, error: current.error, reload, reviewRun, reviewing };
}
