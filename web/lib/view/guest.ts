/**
 * What a guest sees (spec 17): the retention notice, the iteration upsell and
 * how noisy a small run is. Pure, so the wording and the arithmetic are
 * tested without a render.
 */

/** The iteration count a free account runs at, named in the upsell. */
export const FREE_ITERATIONS = 1_000;

export const SIGN_UP_FOR_ITERATIONS = `Sign up free for ${FREE_ITERATIONS.toLocaleString("en-US")} iterations`;

export const CREATE_ACCOUNT = "Create a free account";

/** The banner, in three parts so the middle one can be the link that opens Sign up. */
export function guestNotice(retentionDays: number | null | undefined): {
  lead: string;
  action: string;
  tail: string;
} {
  const lead =
    retentionDays == null
      ? "Guest plan — deleted if you stop visiting. "
      : `Guest plan — deleted after ${retentionDays} ${retentionDays === 1 ? "day" : "days"} without a visit. `;
  return { lead, action: CREATE_ACCOUNT, tail: " to keep it." };
}

/**
 * Half-width of the 95% interval on a success rate, in percentage points:
 * `1.96·√(p(1−p)/n)`, with `p` a fraction and `n` the iterations that
 * produced it. Undefined when there is nothing to measure.
 */
export function successIntervalPoints(rate: number, iterations: number): number | undefined {
  if (!Number.isFinite(rate) || !Number.isFinite(iterations) || iterations <= 0) return undefined;
  const p = Math.min(1, Math.max(0, rate));
  return 1.96 * Math.sqrt((p * (1 - p)) / iterations) * 100;
}

/** "8" for 7.84 points, "0.6" for a tight interval: the figure after "±". */
export function intervalLabel(points: number): string {
  return points >= 1 ? String(Math.round(points)) : points.toFixed(1);
}

/** Where the guest's archive is adopted: the prefix every imported plan gets. */
export const GUEST_PLAN_PREFIX = "Guest – ";
