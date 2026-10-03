import type { PlanHome } from "./url.ts";

/** One home's answer to "list your plans": its rows, or why it could not. */
export interface HomeList<T> {
  rows?: T[];
  error?: Error;
}

export interface MergedPlans<T> {
  /** Every plan either home returned, most recently updated first. */
  rows: T[];
  /** Homes that were asked and could not answer; their plans are missing from `rows`. */
  failed: PlanHome[];
  /** Set only when no home answered: one home down must not hide the other. */
  error?: Error;
}

/** `updated_at` as a time. Both homes write "YYYY-MM-DD HH:MM:SS" UTC or ISO; NaN sorts last. */
function stamp(updatedAt: string): number {
  const text = updatedAt.replace(" ", "T");
  return Date.parse(/(?:Z|[+-]\d\d:?\d\d)$/.test(text) ? text : `${text}Z`);
}

/**
 * The scenario list across homes. A home passed as `undefined` is not asked
 * (local mode off, or not signed in) and counts as neither an answer nor a
 * failure; one with `error` is a failure that leaves the other's plans listed.
 *
 * Sorted newest first across both, as each home already sorts its own, so the
 * default plan is the one touched last wherever it lives.
 */
export function mergePlanLists<T extends { updated_at: string }>(
  homes: Partial<Record<PlanHome, HomeList<T> | undefined>>,
): MergedPlans<T> {
  const asked = (Object.entries(homes) as [PlanHome, HomeList<T> | undefined][]).filter(
    (entry): entry is [PlanHome, HomeList<T>] => entry[1] !== undefined,
  );
  const failed = asked.filter(([, list]) => list.error !== undefined).map(([home]) => home);
  // One home's list is already in its own order, which is left alone.
  const rows =
    asked.length === 1
      ? (asked[0][1].rows ?? [])
      : asked
          .flatMap(([, list]) => list.rows ?? [])
          .map((row, index) => ({ row, index, time: stamp(row.updated_at) }))
          .sort((a, b) => {
            const left = Number.isNaN(a.time) ? -Infinity : a.time;
            const right = Number.isNaN(b.time) ? -Infinity : b.time;
            return left === right ? a.index - b.index : right > left ? 1 : -1;
          })
          .map(({ row }) => row);
  const everyFailed = asked.length > 0 && failed.length === asked.length;
  return {
    rows,
    failed,
    error: everyFailed ? asked.find(([, list]) => list.error)?.[1].error : undefined,
  };
}
