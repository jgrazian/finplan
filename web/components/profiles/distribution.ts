/**
 * The geometry behind every distribution drawn on screen.
 *
 * Seven distribution kinds share one row, and the row carries the shape — so
 * the shape has to be drawn from the distribution itself rather than from the
 * mean and spread the table happens to summarise it by. A Student-t and a
 * normal with the same two figures are different pictures, and the difference
 * is the whole reason to pick one.
 *
 * Every return profile is drawn on one scale, `RETURN_SCALE`, so a row can be
 * read against the row above it and against the same profile in the drawer.
 */
import type { DistributionSpec } from "@/lib/api/types";
import { empiricalQuantiles, historyStats } from "@/lib/history";

/** The scale every return shape is drawn on, in percent. */
export const RETURN_SCALE = { lo: -45, hi: 55 } as const;

/** Inflation sits in a much narrower range; a return scale would flatten it. */
export const CPI_SCALE = { lo: -4, hi: 12 } as const;

export interface ShapeBox {
  w: number;
  h: number;
  lo: number;
  hi: number;
}

/**
 * Where one of the three quantiles falls on the drawn shape: the value itself,
 * and the point on the curve above it. `y` is the height of the distribution
 * at `v` — so the tick stops at the curve rather than cutting through it, and
 * the reader can see how much of the mass is out past a −22% year.
 */
export interface QuantileMark {
  /** 5, 50 or 95. */
  q: number;
  /** Annual return in percent. */
  v: number;
  x: number;
  y: number;
}

/**
 * The paths a shape is drawn from. `line2` is a second curve the kind wants
 * alongside its own — the normal a Student-t is being compared against, the
 * bear half of a regime model — and `marks` is for the kinds that are a tick
 * rather than a curve.
 *
 * `band` and `median` are the quantiles, and they are different shapes because
 * they are different facts: the band is an extent, drawn as a capped whisker
 * along the baseline, and the median is a position, drawn as a stem up to the
 * curve. Any of them may be empty.
 */
export interface ShapePaths {
  fill: string;
  line: string;
  line2: string;
  marks: string;
  /** A resampled history's observed years, bucketed — filled and stroked. */
  bars: string;
  /** 5th and 95th, from the baseline up to the curve. */
  band: string;
  /** The 50th, likewise — drawn stronger, since it is the typical year. */
  median: string;
  /** The same three as points, for a label beside the tick. */
  points: QuantileMark[];
}

const EMPTY: ShapePaths = {
  fill: "",
  line: "",
  line2: "",
  marks: "",
  bars: "",
  band: "",
  median: "",
  points: [],
};

/** Buckets across the scale; 20 puts the return scale on 5-point years. */
const BUCKETS = 20;

/** Points across the box; enough that a fat tail does not read as facets. */
const STEPS = 110;

/**
 * Percent label, one decimal — the convention across every profile figure.
 * A resampled distribution has no closed-form mean, and a dash is the honest
 * reading of that.
 */
export function pct(v: number | null): string {
  // A true minus, not a hyphen — the same one `fmtCurrency` sets, so a figure
  // reads the same whether it is a dollar amount or a rate.
  return v == null ? "—" : `${v.toFixed(1).replace("-", "\u2212")}%`;
}

// ── densities ─────────────────────────────────────────────────────────────
/**
 * The profile's density over annual return in percent, or null where the kind
 * has no curve: a constant is a tick, and a resampled history is a set of
 * observations the client was never sent.
 *
 * Normalised properly rather than to a peak of one, because a regime model
 * mixes two of these and the mixture is only right if each carries its own
 * 1/σ. The √2π every kind would share is dropped — nothing here reads an
 * absolute density.
 */
function densityOf(spec: DistributionSpec): ((v: number) => number) | null {
  switch (spec.kind) {
    case "None":
    case "Fixed":
    case "Bootstrap":
      return null;
    case "Normal": {
      const mean = spec.mean * 100;
      const sd = Math.max(Math.abs(spec.std_dev * 100), 0.05);
      return (v) => Math.exp(-0.5 * ((v - mean) / sd) ** 2) / sd;
    }
    case "LogNormal": {
      // `mean` is the median of the multiplier, so it is the log's location.
      const mu = Math.log(Math.max(1 + spec.mean, 1e-6));
      const sd = Math.max(Math.abs(spec.std_dev), 0.0005);
      return (v) => {
        const x = 1 + v / 100;
        if (x <= 1e-6) return 0;
        return Math.exp(-0.5 * ((Math.log(x) - mu) / sd) ** 2) / (x * sd);
      };
    }
    case "StudentT": {
      const mean = spec.mean * 100;
      const scale = Math.max(Math.abs(spec.scale * 100), 0.05);
      const df = Math.max(spec.df, 0.5);
      return (v) => (1 + ((v - mean) / scale) ** 2 / df) ** (-(df + 1) / 2) / scale;
    }
    case "RegimeSwitching": {
      const bull = densityOf(spec.bull);
      const bear = densityOf(spec.bear);
      if (!bull || !bear) return bull ?? bear;
      const w = bullShare(spec);
      return (v) => w * bull(v) + (1 - w) * bear(v);
    }
  }
}

/**
 * Long-run share of years spent in the bull state — the stationary
 * distribution of the two-state chain, which is what a mixture drawn over a
 * whole retirement should be weighted by rather than by 50/50.
 */
function bullShare(spec: Extract<DistributionSpec, { kind: "RegimeSwitching" }>): number {
  const out = Math.max(spec.bull_to_bear_prob, 0);
  const back = Math.max(spec.bear_to_bull_prob, 0);
  return out + back === 0 ? 0.5 : back / (out + back);
}

/** The spread the shape's height is scaled by; null where there is none. */
function spreadOf(spec: DistributionSpec): number | null {
  switch (spec.kind) {
    case "Normal":
    case "LogNormal":
      return Math.abs(spec.std_dev * 100);
    case "StudentT":
      return Math.abs(spec.scale * 100) * 1.06;
    case "RegimeSwitching":
      return spreadOf(spec.bull) ?? spreadOf(spec.bear);
    default:
      return null;
  }
}

/**
 * How much of the box a shape may fill.
 *
 * Every shape is normalised to its own peak, so without this a T-bill profile
 * would tower over a small-cap one drawn on the same scale and the two would
 * read as equally likely rather than equally normalised. Trading height back
 * against spread makes them the same kind of object — and it applies to a
 * histogram exactly as it does to a curve, so a resampled S&P sits at the same
 * height as a Normal fitted to it.
 */
function heightFactor(span: number, spread: number): number {
  return Math.max(0.34, Math.min(1, Math.sqrt(span / 12 / Math.max(spread, 0.6))));
}

// ── quantiles ─────────────────────────────────────────────────────────────
/**
 * Quantiles of the annual return, in percent, integrated off the density
 * rather than taken from a normal approximation: the two kinds where the
 * approximation is worst — a fat tail and a regime blend — are exactly the two
 * whose spread is worth reading.
 *
 * Null where the kind has no dispersion to report, or none the client can see.
 */
export function quantilesOf(
  spec: DistributionSpec,
  qs: number[],
  history?: readonly number[],
): number[] | null {
  // A resampled history has its quantiles in hand; there is nothing to
  // integrate, and integrating a fitted curve instead would report the spread
  // of a distribution the profile deliberately does not use.
  if (spec.kind === "Bootstrap") return history ? empiricalQuantiles(history, qs) : null;

  const f = densityOf(spec);
  if (!f) return null;

  const lo = -99.5;
  const hi = 800;
  const step = 0.25;
  const n = Math.round((hi - lo) / step);

  // Cumulative trapezoid, then the crossings read back off it.
  const cdf = new Float64Array(n + 1);
  let prev = f(lo);
  for (let i = 1; i <= n; i++) {
    const next = f(lo + i * step);
    cdf[i] = cdf[i - 1] + ((prev + next) / 2) * step;
    prev = next;
  }
  const total = cdf[n];
  if (!(total > 0)) return null;

  return qs.map((q) => {
    const want = q * total;
    let i = 1;
    while (i < n && cdf[i] < want) i++;
    // Linear inside the cell the crossing lands in; the grid is fine enough
    // that this is well below the one decimal the figure is shown to.
    const cell = cdf[i] - cdf[i - 1];
    const frac = cell === 0 ? 0 : (want - cdf[i - 1]) / cell;
    return lo + (i - 1 + frac) * step;
  });
}

/** The 5th and 95th alone — what the tables print as a profile's spread. */
export function bandOf(
  spec: DistributionSpec,
  history?: readonly number[],
): [number, number] | null {
  const q = quantilesOf(spec, [0.05, 0.95], history);
  return q ? [q[0], q[1]] : null;
}

/**
 * The band as the tables print it. The kinds with no band each say why rather
 * than showing a dash: "held flat" and "no dispersion" are different facts,
 * and "resampled" is a third — the shape exists, the client just has not been
 * sent the observations behind it.
 */
export function bandLabel(spec: DistributionSpec, history?: readonly number[]): string {
  switch (spec.kind) {
    case "None":
      return "held flat";
    case "Fixed":
      return "no dispersion";
    default: {
      const band = bandOf(spec, history);
      // Bootstrap with no history yet: the years are what it is made of, and
      // saying so is better than a dash that reads as "no spread".
      if (!band) return spec.kind === "Bootstrap" ? "resampled" : "—";
      return `${pct(band[0])} … ${pct(band[1])}`;
    }
  }
}

/** True where the band is a pair of figures rather than a word about the kind. */
export function bandIsFigures(
  spec: DistributionSpec,
  history?: readonly number[],
): boolean {
  if (spec.kind === "None" || spec.kind === "Fixed") return false;
  return spec.kind !== "Bootstrap" || history != null;
}

// ── paths ─────────────────────────────────────────────────────────────────
/**
 * The profile's shape, as SVG paths sized to `box` and positioned on its
 * scale. A constant and a no-growth profile are single ticks — drawing a bell
 * for either would be a picture of an assumption the profile does not make —
 * and a resampled history draws nothing at all.
 */
export function shapePaths(
  spec: DistributionSpec,
  box: ShapeBox,
  history?: readonly number[],
): ShapePaths {
  const { w, h, lo, hi } = box;
  const base = h - 2;
  const span = hi - lo;
  const x = (v: number) => ((v - lo) / span) * w;
  const tick = (v: number, top: number) =>
    `M${x(v).toFixed(1)} ${base} L${x(v).toFixed(1)} ${top}`;

  if (spec.kind === "None") return { ...EMPTY, marks: tick(0, base - 9) };
  if (spec.kind === "Fixed") return { ...EMPTY, marks: tick(spec.rate * 100, 3) };
  if (spec.kind === "Bootstrap") {
    return history ? histogram(history, box, x) : EMPTY;
  }

  const density = densityOf(spec);
  if (!density) return EMPTY;

  const height = heightFactor(span, spreadOf(spec) ?? 12);

  const samples = (f: (v: number) => number) => {
    const out: number[] = [];
    for (let i = 0; i <= STEPS; i++) out.push(Math.max(0, f(lo + (span * i) / STEPS)));
    return out;
  };
  const peak = (vals: number[]) => Math.max(...vals, Number.EPSILON);
  const pathOf = (vals: number[], max: number) => {
    let d = "";
    for (let i = 0; i <= STEPS; i++) {
      const px = ((w * i) / STEPS).toFixed(1);
      const py = (base - (vals[i] / max) * (h - 5) * height).toFixed(1);
      d += `${i ? "L" : "M"}${px} ${py} `;
    }
    return d.trim();
  };

  // A regime model is drawn as its two states rather than as their blend: the
  // blend is a two-humped curve that looks like a distribution nothing was
  // drawn from, where the states are the thing the model actually says.
  const [main, companion] = curves(spec, density, samples);

  const max = Math.max(peak(main), companion ? peak(companion) : 0);
  const line = pathOf(main, max);

  // The ticks stop at the height of the whole distribution at that return,
  // not at the solid curve: for a regime model the solid curve is one state,
  // and the quantile is a fact about the blend of both.
  const points = (quantilesOf(spec, [0.05, 0.5, 0.95]) ?? []).map((v, i) => {
    const at = Math.min(1, density(v) / max);
    return {
      q: [5, 50, 95][i],
      v,
      x: Math.min(w, Math.max(0, x(v))),
      y: base - at * (h - 5) * height,
    };
  });
  return {
    line,
    line2: companion ? pathOf(companion, max) : "",
    fill: `${line} L${w.toFixed(1)} ${base} L0 ${base} Z`,
    marks: "",
    bars: "",
    band: whisker(points, base, h, box),
    median: points
      .filter((m) => m.q === 50)
      .map((m) => `M${m.x.toFixed(1)} ${base} L${m.x.toFixed(1)} ${m.y.toFixed(1)}`)
      .join(" "),
    points,
  };
}

/**
 * The middle 90%, as a capped rail along the baseline.
 *
 * Drawn to the curve instead, a 5th-percentile tick would be three pixels tall
 * in a row — the density out there is a quarter of the peak, which is the
 * whole point of it being the 5th, and is exactly why it cannot carry the mark.
 * A rail is legible at any height and says the truer thing anyway: this is
 * where nine years in ten land.
 */
/**
 * A resampled history as the years it is made of.
 *
 * Buckets are fixed to the scale rather than to the data, so two histories are
 * comparable row to row. Years outside the scale are counted in the end bucket
 * instead of dropped — small caps have had a +101% year and emerging markets a
 * −68% one, and losing them would understate exactly the tail a bootstrap is
 * chosen for. The drawer states the true worst and best beneath.
 */
function histogram(
  history: readonly number[],
  box: ShapeBox,
  x: (v: number) => number,
): ShapePaths {
  const { w, h, lo, hi } = box;
  const base = h - 2;
  const span = hi - lo;
  const width = span / BUCKETS;

  const counts = new Array<number>(BUCKETS).fill(0);
  for (const r of history) {
    const v = Math.min(hi - 1e-6, Math.max(lo, r * 100));
    counts[Math.min(BUCKETS - 1, Math.floor((v - lo) / width))] += 1;
  }
  const tallest = Math.max(...counts);
  if (tallest === 0) return EMPTY;

  const stats = historyStats(history);
  const height = heightFactor(span, stats?.sd ?? 12);
  const topOf = (count: number) => base - (count / tallest) * (h - 5) * height;

  const bar = w / BUCKETS;
  // A hairline inset each side, so neighbouring bars read as two and not one.
  const inset = Math.min(0.6, bar * 0.08);
  let bars = "";
  counts.forEach((count, i) => {
    if (count === 0) return;
    const x0 = (i * bar + inset).toFixed(1);
    const x1 = ((i + 1) * bar - inset).toFixed(1);
    const y = topOf(count).toFixed(1);
    bars += `M${x0} ${base} L${x0} ${y} L${x1} ${y} L${x1} ${base} Z `;
  });

  const points = (empiricalQuantiles(history, [0.05, 0.5, 0.95]) ?? []).map((v, i) => {
    const clamped = Math.min(hi - 1e-6, Math.max(lo, v));
    const bucket = Math.min(BUCKETS - 1, Math.floor((clamped - lo) / width));
    return {
      q: [5, 50, 95][i],
      v,
      x: Math.min(w, Math.max(0, x(clamped))),
      // The median stem stops at the top of the bar it falls in, the way it
      // stops at the curve for every other kind.
      y: topOf(counts[bucket]),
    };
  });

  return {
    ...EMPTY,
    bars: bars.trim(),
    band: whisker(points, base, h, box),
    median: points
      .filter((m) => m.q === 50)
      .map((m) => `M${m.x.toFixed(1)} ${base} L${m.x.toFixed(1)} ${m.y.toFixed(1)}`)
      .join(" "),
    points,
  };
}

function whisker(points: QuantileMark[], base: number, h: number, box: ShapeBox): string {
  const ends = points.filter((m) => m.q !== 50);
  if (ends.length < 2) return "";
  const cap = Math.min(12, Math.max(4, h * 0.22));
  const rail = base - cap / 2;
  const [low, high] = ends;

  // An end past the scale gets a chevron rather than a cap. Emerging markets
  // put their 95th at +76% and small caps have had a +101% year; a flat cap at
  // the box edge would say the tail stops there, which is the one thing a
  // bootstrap is picked to disagree with.
  const end = (m: QuantileMark, pointing: -1 | 1) => {
    const beyond = pointing < 0 ? m.v < box.lo : m.v > box.hi;
    const x = m.x.toFixed(1);
    if (!beyond) return `M${x} ${base} L${x} ${(base - cap).toFixed(1)}`;
    const back = (m.x - pointing * Math.min(4, cap * 0.6)).toFixed(1);
    return `M${back} ${(base - cap).toFixed(1)} L${x} ${rail.toFixed(1)} L${back} ${base}`;
  };

  return (
    `${end(low, -1)} ${end(high, 1)} ` +
    `M${low.x.toFixed(1)} ${rail.toFixed(1)} L${high.x.toFixed(1)} ${rail.toFixed(1)}`
  );
}

/**
 * The solid curve and the dashed one beside it. For most kinds the density is
 * the whole picture; two kinds are worth reading against something — a
 * Student-t against the normal it is not, and a regime model as its two states.
 */
function curves(
  spec: DistributionSpec,
  density: (v: number) => number,
  samples: (f: (v: number) => number) => number[],
): [number[], number[] | null] {
  if (spec.kind === "StudentT") {
    // Matched on the middle 50% rather than on the variance, which for df 3 is
    // barely finite — so what separates the two curves is the tails alone.
    const matched = densityOf({
      kind: "Normal",
      mean: spec.mean,
      std_dev: spec.scale * 1.06,
    });
    return [samples(density), matched ? samples(matched) : null];
  }
  if (spec.kind === "RegimeSwitching") {
    const bull = densityOf(spec.bull);
    const bear = densityOf(spec.bear);
    const share = bullShare(spec);
    if (!bull || !bear) return [samples(density), null];
    return [samples((v) => share * bull(v)), samples((v) => (1 - share) * bear(v))];
  }
  return [samples(density), null];
}
