/**
 * How hard a run works, and how hard this account may ask it to.
 *
 * Iterations are a cost/precision dial rather than a plan parameter — the same
 * scenario at 100 and at 5,000 is the same scenario — so they are offered as a
 * handful of steps you slide between while looking at the results. The gaps
 * widen because the precision does not: halving the noise costs four times
 * the iterations.
 */

/**
 * How hard the next run should work.
 *
 * `converge` turns `iterations` from the count into the minimum sample the
 * server takes before it starts testing whether the median has settled; it
 * keeps going, up to its own ceiling, until it has.
 */
export interface RunEffort {
  iterations: number;
  converge: boolean;
}

/**
 * The minimum sample a converging run takes before the median is first tested.
 *
 * Small enough that an already-stable plan stops early, large enough that the
 * first median it compares against is not noise.
 */
export const CONVERGE_FLOOR = 500;

/** The stops, coarse to fine and then open-ended. */
export const EFFORT_STOPS: ReadonlyArray<RunEffort> = [
  { iterations: 100, converge: false },
  { iterations: 250, converge: false },
  { iterations: 1_000, converge: false },
  { iterations: 2_000, converge: false },
  { iterations: 5_000, converge: false },
  { iterations: CONVERGE_FLOOR, converge: true },
];

export function effortLabel(effort: RunEffort): string {
  return effort.converge ? "Converge" : effort.iterations.toLocaleString("en-US");
}

/**
 * The stops a run may use when the server caps iterations at `max`.
 *
 * Converge goes first, since its floor is a count of its own; if no fixed stop
 * fits (a cap under 100) the cap itself is the one stop. No cap known yet
 * offers everything.
 */
export function effortStops(max?: number): ReadonlyArray<RunEffort> {
  if (max == null) return EFFORT_STOPS;
  const fits = EFFORT_STOPS.filter((stop) =>
    stop.converge ? max >= CONVERGE_FLOOR : stop.iterations <= max,
  );
  return fits.some((stop) => !stop.converge)
    ? fits
    : [{ iterations: Math.max(1, Math.floor(max)), converge: false }, ...fits];
}

/** The stop closest to a plain iteration count, for seeding from a preference. */
export function nearestStop(iterations: number): RunEffort {
  const fixed = EFFORT_STOPS.filter((stop) => !stop.converge);
  return fixed.reduce((best, stop) =>
    Math.abs(stop.iterations - iterations) < Math.abs(best.iterations - iterations)
      ? stop
      : best,
  );
}

/**
 * The effort a run can actually be started at: the one asked for if the cap
 * allows it, otherwise the highest fixed stop that fits.
 */
export function clampEffort(effort: RunEffort, max?: number): RunEffort {
  const stops = effortStops(max);
  if (stops.some((s) => s.converge === effort.converge && s.iterations === effort.iterations)) {
    return effort;
  }
  const fixed = stops.filter((s) => !s.converge);
  return fixed[fixed.length - 1];
}
