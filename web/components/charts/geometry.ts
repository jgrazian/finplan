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

export interface Scale {
  geo: ChartGeometry;
  /** Index → x. */
  x: (i: number) => number;
  /** Value → y. */
  y: (v: number) => number;
  /** Top of the value domain. */
  max: number;
  /** y of the value-zero baseline. */
  baseline: number;
  count: number;
}

export function makeScale(
  count: number,
  max: number,
  geo: ChartGeometry = DEFAULT_GEOMETRY,
): Scale {
  const span = geo.w - geo.left - geo.right;
  const plotHeight = geo.h - geo.bottom - geo.top;
  return {
    geo,
    max,
    count,
    baseline: geo.h - geo.bottom,
    x: (i) => geo.left + (count > 1 ? (i * span) / (count - 1) : span / 2),
    y: (v) => geo.h - geo.bottom - (max > 0 ? (v / max) * plotHeight : 0),
  };
}

/** Domain top with the design's 5% headroom above the highest series. */
export function domainMax(...series: number[][]): number {
  const peak = Math.max(...series.flatMap((s) => (s.length ? [Math.max(...s)] : [0])));
  return peak * 1.05;
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

export interface Bar {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function barLayout(values: number[], scale: Scale): Bar[] {
  const { geo } = scale;
  const slot = (geo.w - geo.left - geo.right) / values.length;
  return values.map((v, i) => ({
    x: geo.left + i * slot + 1.5,
    w: Math.max(slot - 3, 2),
    y: scale.y(v),
    h: Math.max(scale.baseline - scale.y(v), 0.5),
  }));
}

/** Evenly spaced value ticks, inclusive of both ends. */
export function valueTicks(scale: Scale, steps = 4): Array<{ value: number; y: number }> {
  const step = scale.max / steps;
  return Array.from({ length: steps + 1 }, (_, k) => ({
    value: step * k,
    y: scale.y(step * k),
  }));
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
  const i = Math.round(((rel - geo.left) / span) * (count - 1));
  return Math.max(0, Math.min(count - 1, i));
}
