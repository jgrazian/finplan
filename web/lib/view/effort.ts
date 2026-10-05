/**
 * How hard a run works, and how hard this account may ask it to.
 *
 * Iterations are a cost/precision dial rather than a plan parameter — the same
 * scenario at 100 and at 5,000 is the same scenario — so they are offered as a
 * handful of steps you slide between while looking at the results. The gaps
 * widen because the precision does not: halving the noise costs four times
 * the iterations.
 */

/** How hard the next run should work: how many iterations it takes. */
export interface RunEffort {
  iterations: number;
}

/**
 * The stops, coarse to fine. The top one bounds the 95% interval on the
 * success rate to about ±1 point whatever the rate, which is as fine as the
 * headline is worth reading.
 */
export const EFFORT_STOPS: ReadonlyArray<RunEffort> = [
  { iterations: 100 },
  { iterations: 250 },
  { iterations: 1_000 },
  { iterations: 2_000 },
  { iterations: 5_000 },
  { iterations: 10_000 },
];

export function effortLabel(effort: RunEffort): string {
  return effort.iterations.toLocaleString("en-US");
}

/**
 * The stops a run may use when the server caps iterations at `max`. If none
 * fits (a cap under 100) the cap itself is the one stop. No cap known yet
 * offers everything.
 */
export function effortStops(max?: number): ReadonlyArray<RunEffort> {
  if (max == null) return EFFORT_STOPS;
  const fits = EFFORT_STOPS.filter((stop) => stop.iterations <= max);
  return fits.length > 0 ? fits : [{ iterations: Math.max(1, Math.floor(max)) }];
}

/** The stop closest to a plain iteration count, for seeding from a preference. */
export function nearestStop(iterations: number): RunEffort {
  return EFFORT_STOPS.reduce((best, stop) =>
    Math.abs(stop.iterations - iterations) < Math.abs(best.iterations - iterations)
      ? stop
      : best,
  );
}

/**
 * The effort a run can actually be started at: the one asked for if the cap
 * allows it, otherwise the highest stop that fits.
 */
export function clampEffort(effort: RunEffort, max?: number): RunEffort {
  const stops = effortStops(max);
  if (stops.some((s) => s.iterations === effort.iterations)) return effort;
  return stops[stops.length - 1];
}

/**
 * Where this browser keeps the last stop the slider was left on, so a reload
 * or a remounted workbench comes back to it rather than to the account default.
 */
export const EFFORT_STORAGE_KEY = "finplan:run-iterations";

/**
 * The stop a stored value names, or nothing if it cannot be trusted. Storage
 * is hand-editable and outlives the stops that wrote it, so only a count that
 * is still a stop is taken.
 */
export function parseStoredEffort(raw: string | null): RunEffort | undefined {
  if (raw == null || raw.trim() === "") return undefined;
  const iterations = Number(raw);
  return EFFORT_STOPS.find((stop) => stop.iterations === iterations);
}
