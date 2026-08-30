/** Shared formatting used while turning API shapes into display strings. */

/** `0.0345` → `$3` / `$1,200` — amounts on the wire are plain dollars. */
export function money(value: number): string {
  const rounded = Math.round(value);
  return (rounded < 0 ? "−$" : "$") + Math.abs(rounded).toLocaleString("en-US");
}

/** A rate in [0,1] as a percentage figure, e.g. `0.099` → `9.9`. */
export function ratePercent(rate: number): number {
  return rate * 100;
}

/** The year component of an ISO date, without constructing a Date. */
export function yearOf(isoDate: string): number {
  return Number(isoDate.slice(0, 4));
}

/**
 * Whole years between two ISO dates. Month/day aware, so someone born in
 * December is not aged a year early on 1 January.
 */
export function yearsBetween(fromIso: string, toIso: string): number {
  const [fy, fm, fd] = fromIso.split("-").map(Number);
  const [ty, tm, td] = toIso.split("-").map(Number);
  const years = ty - fy;
  return tm < fm || (tm === fm && td < fd) ? years - 1 : years;
}

/** ISO date `n` years after `isoDate`, clamping Feb 29 onto Feb 28. */
export function addYears(isoDate: string, years: number): string {
  const [y, m, d] = isoDate.split("-").map(Number);
  const day = m === 2 && d === 29 ? 28 : d;
  return `${y + years}-${String(m).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}
