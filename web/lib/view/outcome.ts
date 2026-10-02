/**
 * What a run's headline figures mean to someone who does not read Monte Carlo
 * output for a living: where a success rate sits on a rule-of-thumb scale, and
 * which one-off events the chart should mark so its bends have a cause.
 */
import type { YearlyCashFlow } from "@/lib/types";

export type SuccessBandKey = "fragile" | "workable" | "robust";

export interface SuccessBand {
  key: SuccessBandKey;
  label: string;
  /** Where the band starts, as a fraction; the next band's start ends it. */
  from: number;
  /** One line on what a rate in this band usually means. */
  meaning: string;
}

/**
 * The scale the success rate is read against. Planners disagree on the exact
 * cut points; these are the common rules of thumb, not a recommendation, and
 * the tooltip beside the scale says so.
 */
export const SUCCESS_BANDS: readonly SuccessBand[] = [
  {
    key: "fragile",
    label: "Fragile",
    from: 0,
    meaning: "More than one future in four runs short. Worth changing something before relying on it.",
  },
  {
    key: "workable",
    label: "Workable",
    from: 0.75,
    meaning: "Most futures hold, but a bad sequence of returns could force cuts later on.",
  },
  {
    key: "robust",
    label: "Robust",
    from: 0.9,
    meaning: "Nine futures in ten or better hold. Near 100%, the plan may be spending less than it could.",
  },
];

/** The scale starts here: below half, where in the fragile band is moot. */
export const SUCCESS_SCALE_FLOOR = 0.5;

export function successBand(rate: number): SuccessBand {
  let band = SUCCESS_BANDS[0];
  for (const candidate of SUCCESS_BANDS) if (rate >= candidate.from) band = candidate;
  return band;
}

/** Where a rate sits along the drawn scale, 0–1, clamped to its floor. */
export function successScalePosition(rate: number): number {
  const span = 1 - SUCCESS_SCALE_FLOOR;
  return Math.min(1, Math.max(0, (rate - SUCCESS_SCALE_FLOOR) / span));
}

// ── event markers ────────────────────────────────────────────────────

export interface ChartMarker {
  /** Index into the chart's years. */
  index: number;
  year: number;
  label: string;
}

/**
 * One marker per event that starts in a year the chart draws — the home
 * purchase, the retirement — so two starting in the same year are both named,
 * in the order they fired. The plan's first year is skipped: everything that
 * starts with the plan "fires" there, and a rule on the y axis says nothing.
 */
export function eventMarkers(cashFlows: YearlyCashFlow[], years: number[]): ChartMarker[] {
  const indexOf = new Map(years.map((year, index) => [year, index]));
  const markers: ChartMarker[] = [];
  for (const flow of cashFlows) {
    const index = indexOf.get(flow.year);
    if (index == null || index === 0) continue;
    for (const label of flow.ledger.tags) markers.push({ index, year: flow.year, label });
  }
  // Stable, so same-year markers keep their firing order.
  return markers.sort((a, b) => a.index - b.index);
}

/**
 * Stack marker labels into rows so neighbours never overlap: each label takes
 * the first row whose last label ends left of where this one begins.
 * `width` is the label's drawn width in chart units.
 */
export function stackLabels(
  items: Array<{ x: number; width: number }>,
  gap = 6,
): number[] {
  const rowEnds: number[] = [];
  return items.map(({ x, width }) => {
    let row = rowEnds.findIndex((end) => end + gap <= x);
    if (row === -1) row = rowEnds.length;
    rowEnds[row] = x + width;
    return row;
  });
}
