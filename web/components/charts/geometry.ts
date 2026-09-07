/** Chart plotting geometry, in the design's 920×300 viewBox units. */
export interface ChartGeometry {
  w: number;
  h: number;
  left: number;
  right: number;
  top: number;
  bottom: number;
}

export const DEFAULT_GEOMETRY: ChartGeometry = {
  w: 920,
  h: 300,
  left: 64,
  right: 12,
  top: 8,
  bottom: 28,
};

/**
 * Where an index sits on the x axis. A `point` mark stands on the index itself,
 * so the first and last sit on the frame's edges; a `band` mark owns a slot and
 * stands in the middle of it. Bars are bands, and the crosshair has to agree
 * with them or it lands beside the column it is reading.
 */
export type ScaleMode = "point" | "band";

/** Linear reads absolute dollars; log reads growth rate. */
export type ScaleKind = "linear" | "log";

/** The stretch of value the plot height is spent on. `min` is 0 when linear. */
export interface Domain {
  min: number;
  max: number;
}

export interface Scale {
  geo: ChartGeometry;
  mode: ScaleMode;
  kind: ScaleKind;
  /** Index → x, at the mode's anchor. */
  x: (i: number) => number;
  /** Value → y, clamped to the plot. */
  y: (v: number) => number;
  /** Bottom of the value domain — zero on a linear axis, never zero on a log one. */
  min: number;
  /** Top of the value domain. */
  max: number;
  /** One index's share of the x span: the bar chart's column pitch. */
  slot: number;
  /** y of the domain floor, where the x axis is drawn. */
  baseline: number;
  count: number;
}

export function makeScale(
  count: number,
  domain: Domain,
  {
    geo = DEFAULT_GEOMETRY,
    mode = "point",
    kind = "linear",
  }: { geo?: ChartGeometry; mode?: ScaleMode; kind?: ScaleKind } = {},
): Scale {
  const span = geo.w - geo.left - geo.right;
  const plotHeight = geo.h - geo.bottom - geo.top;
  const baseline = geo.h - geo.bottom;
  const slot = span / Math.max(count, 1);
  const { min, max } = domain;
  const logSpan = kind === "log" ? Math.log(max / min) : 0;

  const fraction = (v: number) =>
    kind === "log"
      ? logSpan > 0
        ? Math.log(v / min) / logSpan
        : 0
      : max > min
        ? (v - min) / (max - min)
        : 0;

  return {
    geo,
    mode,
    kind,
    min,
    max,
    slot,
    baseline,
    count,
    x:
      mode === "band"
        ? (i) => geo.left + (i + 0.5) * slot
        : (i) => geo.left + (count > 1 ? (i * span) / (count - 1) : span / 2),
    y: (v) => baseline - clamp01(fraction(v)) * plotHeight,
  };
}

/** Keeps a value inside the plot; a non-finite share (log of 0) sits on the floor. */
function clamp01(t: number): number {
  return Number.isFinite(t) ? Math.min(1, Math.max(0, t)) : 0;
}

/** Zero to the design's 5% headroom above the highest figure plotted. */
export function linearDomain(...series: number[][]): Domain {
  return { min: 0, max: peak(series) * 1.05 || 1 };
}

/**
 * A log axis has no zero to stand on, so it needs a floor. The floor drops to
 * the decade below the smallest positive figure — but never much more than four
 * decades under the top, because a path that dips to $40 in one bad year should
 * not cost the other thirty-five most of the plot height.
 */
export function logDomain(...series: number[][]): Domain {
  const top = peak(series);
  if (!(top > 0)) return { min: 1, max: 10 };
  const smallest = smallestPositive(series);
  const floor = smallest > 0 ? decadeBelow(smallest) : decadeBelow(top / 100);
  return { min: Math.max(floor, decadeBelow(top) / 1e3), max: top * 1.05 };
}

/** The domain a kind of axis wants over the same numbers. */
export function domainFor(kind: ScaleKind, series: number[][]): Domain {
  return kind === "log" ? logDomain(...series) : linearDomain(...series);
}

function decadeBelow(v: number): number {
  return Math.pow(10, Math.floor(Math.log10(v)));
}

function peak(series: number[][]): number {
  return Math.max(0, ...series.flatMap((s) => (s.length ? [Math.max(...s)] : [])));
}

function smallestPositive(series: number[][]): number {
  const positives = series.flatMap((s) => s.filter((v) => v > 0));
  return positives.length ? Math.min(...positives) : 0;
}

/** Polyline through every point of `values`. */
export function linePath(values: number[], scale: Scale): string {
  return values
    .map((v, i) => `${i ? "L" : "M"}${scale.x(i).toFixed(1)} ${scale.y(v).toFixed(1)}`)
    .join(" ");
}

/** Closed band between an upper and lower series. */
export function bandPath(upper: number[], lower: number[], scale: Scale): string {
  const back = lower
    .map((_, i) => {
      const j = lower.length - 1 - i;
      return `L${scale.x(j).toFixed(1)} ${scale.y(lower[j]).toFixed(1)}`;
    })
    .join(" ");
  return `${linePath(upper, scale)} ${back} Z`;
}

/** Closed band between a stacked series' running top and bottom edges. */
export function areaPath(top: number[], bottom: number[], scale: Scale): string {
  return bandPath(top, bottom, scale);
}

/**
 * The x extent of one index's column. Bars are the only marks that own width,
 * and both the plain and the segmented ones measure it here so a column, its
 * segments and the crosshair through them all stand in the same place.
 */
export function barColumn(i: number, scale: Scale): { x: number; w: number } {
  const gap = Math.min(3, scale.slot * 0.2);
  const w = Math.max(scale.slot - gap, 1);
  return { x: scale.x(i) - w / 2, w };
}

/**
 * Gridline values: round numbers either way. The linear domain is held tight
 * against the data, so the ticks climb in a round step from the floor and stop
 * wherever the last one fits, rather than dividing the domain into equal shares
 * and labelling the axis $427k, $855k, $1.28M.
 */
export function valueTicks(scale: Scale, steps = 4): Array<{ value: number; y: number }> {
  if (scale.kind === "log") return logTicks(scale);
  const step = niceStep((scale.max - scale.min) / steps);
  const ticks: Array<{ value: number; y: number }> = [];
  for (let v = Math.ceil(scale.min / step) * step; v <= scale.max; v += step) {
    ticks.push({ value: v, y: scale.y(v) });
  }
  return ticks;
}

/** The nearest 1 / 2 / 2.5 / 5 × 10ⁿ at or above `raw`. */
function niceStep(raw: number): number {
  if (!(raw > 0)) return 1;
  const decade = Math.pow(10, Math.floor(Math.log10(raw)));
  const mantissa = raw / decade;
  const nice =
    mantissa <= 1 ? 1 : mantissa <= 2 ? 2 : mantissa <= 2.5 ? 2.5 : mantissa <= 5 ? 5 : 10;
  return nice * decade;
}

/** Tick mantissas, finest first — the coarsest that fits is the one drawn. */
const LOG_MANTISSAS = [[1, 1.5, 2, 3, 5, 7], [1, 2, 5], [1]];
const MAX_LOG_TICKS = 7;

/**
 * Round numbers inside a log domain. A plan spanning several decades wants only
 * the powers of ten; one that never leaves a decade and a half would be left
 * with a single gridline — so the finest set that still fits the frame wins.
 */
function logTicks(scale: Scale): Array<{ value: number; y: number }> {
  const first = Math.floor(Math.log10(scale.min) + 1e-9);
  const last = Math.floor(Math.log10(scale.max) + 1e-9);

  let ticks: number[] = [];
  for (const mantissas of LOG_MANTISSAS) {
    ticks = [];
    for (let e = first; e <= last; e += 1) {
      for (const m of mantissas) {
        const value = m * Math.pow(10, e);
        if (value >= scale.min * (1 - 1e-9) && value <= scale.max) ticks.push(value);
      }
    }
    if (ticks.length <= MAX_LOG_TICKS) break;
  }

  if (!ticks.length) ticks = [scale.min];
  return ticks.map((value) => ({ value, y: scale.y(value) }));
}

/** Index ticks every `every` years, e.g. 2030, 2035, … */
export function yearTicks(
  years: number[],
  scale: Scale,
  every = 5,
): Array<{ index: number; year: number; x: number }> {
  return years
    .map((year, index) => ({ year, index }))
    .filter((t) => t.year % every === 0)
    .map((t) => ({ ...t, x: scale.x(t.index) }));
}

/** Maps a pointer's clientX onto the nearest series index. */
export function indexFromPointer(
  clientX: number,
  rect: DOMRect,
  scale: Scale,
): number {
  const { geo, count } = scale;
  const rel = ((clientX - rect.left) / rect.width) * geo.w;
  const span = geo.w - geo.left - geo.right;
  const i =
    scale.mode === "band"
      ? Math.floor((rel - geo.left) / scale.slot)
      : Math.round(((rel - geo.left) / span) * (count - 1));
  return Math.max(0, Math.min(count - 1, i));
}
