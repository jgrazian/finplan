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

/** Linear reads absolute dollars; log reads growth rate, through zero into debt. */
export type ScaleKind = "linear" | "log";

/** Signed position on the log axis; finite for every finite dollar amount. */
function symlog(v: number, unit: number): number {
  return Math.sign(v) * Math.log10(1 + Math.abs(v) / unit);
}

/**
 * The dollars either side of zero that a log axis spends on its linear segment.
 * A plain log axis has no zero and no debt to stand on, so "log" here is the
 * bi-symmetric log sign(v)·log10(1 + |v|/unit): decades above the unit, their
 * mirror below −unit, and a real zero in between.
 *
 * A plan that crosses zero takes a thousandth of its own decade, which leaves
 * three or four decades on each side: enough to read an early five-figure
 * balance against a seven-figure peak, without spending half the frame on the
 * dollars either side of zero. A plan that never reaches zero takes a unit far
 * below its own floor, so it still reads as the one-sided log axis it was.
 */
function logUnit({ min, max }: Domain): number {
  const reach = Math.max(Math.abs(min), Math.abs(max));
  if (!(reach > 0)) return 1;
  if (min > 0 || max < 0) return Math.min(Math.abs(min), Math.abs(max)) / 1e6;
  return Math.max(1, decadeBelow(reach) / 1000);
}

/** The stretch of value plotted, including debt below zero on linear axes. */
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
  /** Bottom of the value domain; negative when the plan carries net debt. */
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
  const unit = kind === "log" ? logUnit(domain) : 1;
  const logFloor = kind === "log" ? symlog(min, unit) : 0;
  const logSpan = kind === "log" ? symlog(max, unit) - logFloor : 0;

  const fraction = (v: number) =>
    kind === "log"
      ? logSpan > 0
        ? (symlog(v, unit) - logFloor) / logSpan
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

/** Include zero and both signed extremes, with 5% headroom at either end. */
export function linearDomain(...series: number[][]): Domain {
  const min = Math.min(0, ...series.flatMap((s) => s.filter(Number.isFinite)));
  const max = peak(series);
  return min === 0 && max === 0 ? { min: 0, max: 1 } : { min: min * 1.05, max: max * 1.05 };
}

/**
 * The stretch of the symmetric log axis the plan occupies. An all-positive plan
 * keeps the one-sided behaviour: the floor is the decade at or below the
 * smallest outcome, so small outcomes are never clipped for aesthetics. A plan
 * that reaches zero or runs into debt keeps its sign — zero is a position on
 * this axis, so both extremes simply take headroom and the axis crosses
 * between them.
 */
export function logDomain(...series: number[][]): Domain {
  const values = series.flatMap((s) => s.filter(Number.isFinite));
  if (!values.length) return { min: 0, max: 1 };
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  if (lo > 0) return { min: decadeBelow(lo), max: hi * 1.05 };
  if (hi < 0) return { min: lo * 1.05, max: -decadeBelow(-hi) };
  return lo === 0 && hi === 0 ? { min: 0, max: 1 } : { min: lo * 1.05, max: hi * 1.05 };
}

/** The domain a kind of axis wants over the same numbers. */
export function domainFor(kind: ScaleKind, series: number[][]): Domain {
  return kind === "log" ? logDomain(...series) : linearDomain(...series);
}

function decadeBelow(v: number): number {
  return Math.pow(10, Math.floor(Math.log10(v)));
}

function peak(series: number[][]): number {
  return Math.max(0, ...series.flatMap((s) => s.filter(Number.isFinite)));
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

/**
 * Tick steps, finest first — the coarsest that fits is the one drawn. A domain
 * that crosses zero spends its decades twice, once per sign, so the steps carry
 * on past the mantissas into strides of whole decades.
 */
const LOG_TICK_STEPS: Array<{ mantissas: number[]; stride: number }> = [
  { mantissas: [1, 1.5, 2, 3, 5, 7], stride: 1 },
  { mantissas: [1, 2, 5], stride: 1 },
  { mantissas: [1], stride: 1 },
  { mantissas: [1], stride: 2 },
  { mantissas: [1], stride: 3 },
];
// Two signs and a zero need more gridlines than a one-sided axis to stay round.
const MAX_LOG_TICKS = 8;

/**
 * Round numbers inside a log domain. A plan spanning several decades wants only
 * the powers of ten; one that never leaves a decade and a half would be left
 * with a single gridline — so the finest set that still fits the frame wins.
 */
function logTicks(scale: Scale): Array<{ value: number; y: number }> {
  // Zero is a gridline of its own on this axis, and the decades run outwards
  // from it in whichever directions the plan actually goes.
  const zero = scale.min <= 0 && scale.max >= 0 ? [0] : [];

  let ticks: number[] = [];
  for (const step of LOG_TICK_STEPS) {
    ticks = [...zero, ...decadeTicks(scale, step.mantissas, step.stride)].sort((a, b) => a - b);
    if (ticks.length <= MAX_LOG_TICKS) break;
  }

  if (!ticks.length) ticks = [scale.min];
  return ticks.map((value) => ({ value, y: scale.y(value) }));
}

/** Every ±mantissa×10ⁿ inside the domain, exponents taken `stride` at a time. */
function decadeTicks(scale: Scale, mantissas: number[], stride: number): number[] {
  const reach = Math.max(Math.abs(scale.min), Math.abs(scale.max));
  if (!(reach > 0)) return [];
  // Relative slack: a domain floor is an exact decade, and a domain spanning
  // ten of them must not let an absolute epsilon swallow the smallest of them.
  const inside = (v: number) =>
    v >= scale.min - Math.abs(scale.min) * 1e-9 && v <= scale.max + Math.abs(scale.max) * 1e-9;

  // A one-sided domain starts at its own floor; one that crosses zero starts at
  // the unit, below which the axis is linear and its gridlines pile up on zero.
  const floor = scale.min > 0 ? scale.min : scale.max < 0 ? -scale.max : logUnit(scale);
  const first = Math.floor(Math.log10(floor) + 1e-9);
  const values: number[] = [];
  for (let e = first; e <= Math.floor(Math.log10(reach) + 1e-9); e += stride) {
    for (const m of mantissas) {
      const magnitude = m * Math.pow(10, e);
      if (inside(magnitude)) values.push(magnitude);
      if (inside(-magnitude)) values.push(-magnitude);
    }
  }
  return values;
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
