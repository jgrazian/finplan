/**
 * A finished sweep as a workspace: N swept variables, and graphs drawn over it.
 *
 * The server evaluates every combination of every swept variable once. Nothing
 * here asks it for more — a graph picks one or two of those variables for its
 * own axes, holds the rest at a chosen step, and reads the metric it wants out
 * of the cells that already exist. That is what lets a screen carry four graphs
 * of the same sweep and still show one run in the footer.
 */
import type { AnalysisPoint, SweepAxis, SweepResults } from "@/lib/api/types";
import { fmtCompact, fmtCurrency, fmtPercent } from "../format.ts";
import { paramTick } from "./analysis.ts";

// ───────────────────────────── metrics ─────────────────────────────

export type MetricId = "success" | "funding" | "p50" | "p5" | "p95";

/** One dependent variable a graph can be drawn against. */
export interface Metric {
  id: MetricId;
  /** In the inspector's menu. */
  label: string;
  /** In a sentence or a legend, where the heading already says what it is. */
  short: string;
  /** Read it off a cell. `undefined` where the run did not measure it. */
  of: (point: AnalysisPoint) => number | undefined;
  format: (value: number) => string;
  /** The same, short enough for an axis tick. */
  tick: (value: number) => string;
  /**
   * The range to draw these values over.
   *
   * Never the metric's theoretical range: real plans cluster in the last few
   * points of the success scale, and a 0–100% axis renders every grid as the
   * same near-white rectangle. The legend quotes both ends, so the reader is
   * told what the scale is rather than left to assume it.
   */
  span: (values: number[]) => Span;
}

export interface Span {
  low: number;
  high: number;
}

/** The narrowest band a rate will be spread over: a flat grid should look flat. */
const MIN_RATE_SPAN = 0.1;

/** The same for money, as a fraction of the larger end rather than an absolute. */
const MIN_MONEY_SPAN = 0.1;

const rate = (
  id: MetricId,
  label: string,
  short: string,
  of: (point: AnalysisPoint) => number | null,
): Metric => ({
  id,
  label,
  short,
  of: (point) => of(point) ?? undefined,
  format: (value) => fmtPercent(value),
  tick: (value) => `${Math.round(value * 100)}`,
  span: (values) => widen(bounds(values, 0, 1), MIN_RATE_SPAN, 0, 1),
});

const money = (id: MetricId, label: string, short: string, of: (p: AnalysisPoint) => number): Metric => ({
  id,
  label,
  short,
  of,
  format: fmtCurrency,
  tick: fmtCompact,
  span: (values) => {
    const seen = bounds(values, 0, 1);
    return widen(seen, Math.abs(seen.high || 1) * MIN_MONEY_SPAN);
  },
});

/** Everything a sweep measures, in the order the menu offers it. */
export const METRICS: readonly Metric[] = [
  rate("success", "Success rate", "success", (p) => p.success_rate),
  rate("funding", "Funding success", "funding success", (p) => p.funding_success_rate),
  money("p50", "P50 terminal net worth", "P50 terminal", (p) => p.p50),
  money("p5", "P5 terminal net worth", "P5 terminal", (p) => p.p5),
  money("p95", "P95 terminal net worth", "P95 terminal", (p) => p.p95),
];

export function metric(id: MetricId): Metric {
  return METRICS.find((m) => m.id === id) ?? METRICS[0];
}

function bounds(values: number[], fallbackLow: number, fallbackHigh: number): Span {
  const seen = values.filter((v) => Number.isFinite(v));
  return seen.length === 0
    ? { low: fallbackLow, high: fallbackHigh }
    : { low: Math.min(...seen), high: Math.max(...seen) };
}

/** Open a band out to at least `minSpan`, staying inside the clamps if given. */
function widen(span: Span, minSpan: number, clampLow?: number, clampHigh?: number): Span {
  const short = minSpan - (span.high - span.low);
  if (short <= 0) return span;
  let low = span.low - short / 2;
  let high = span.high + short / 2;
  if (clampLow != null && low < clampLow) {
    low = clampLow;
    high = clampHigh == null ? low + minSpan : Math.min(clampHigh, low + minSpan);
  }
  if (clampHigh != null && high > clampHigh) {
    high = clampHigh;
    low = clampLow == null ? high - minSpan : Math.max(clampLow, high - minSpan);
  }
  return { low, high };
}

// ──────────────────────────── the ramp ────────────────────────────

/**
 * The heatmap and surface ramp: one hue, the design system's accent scale from
 * its `-900` step to its `-100`. The heaviest step reads as low, which puts the
 * weight of the image on the corner of the grid where the plan fails.
 *
 * Steps rather than colours, so the ramp follows the account's palette. On a
 * dark ground the scale reverses with it, and `-900` becomes the lightest tint
 * — still the heaviest mark against that ground, so the reading is unchanged.
 */
export const RAMP = [
  "var(--color-accent-900)",
  "var(--color-accent-800)",
  "var(--color-accent-700)",
  "var(--color-accent-600)",
  "var(--color-accent-500)",
  "var(--color-accent-400)",
  "var(--color-accent-300)",
  "var(--color-accent-100)",
] as const;

/** Where a value lands on the ramp, over a span already decided. */
export function shade(span: Span, value: number): string {
  const width = span.high - span.low || 1;
  const step = Math.round(clamp01((value - span.low) / width) * (RAMP.length - 1));
  return RAMP[step];
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

// ──────────────────────────── the space ────────────────────────────

/**
 * A finished sweep, indexed.
 *
 * The server sends cells row-major over the axes, but a graph asks for a point
 * by its position on every variable at once, so the flat array is addressed
 * through strides rather than searched.
 */
export interface SweepSpace {
  axes: SweepAxis[];
  /** Steps along each axis, in the axes' own order. */
  shape: number[];
  /** The cell at a full index vector, or `undefined` where none was measured. */
  at: (indices: readonly number[]) => AnalysisPoint | undefined;
  /** The plan as it stands, so every figure has something to be a change from. */
  plan: AnalysisPoint;
  /** Where the plan sits on each axis, or `undefined` when it is off the grid. */
  planIndices: number[] | undefined;
  iterations: number;
  /** Combinations evaluated. */
  points: number;
  /** Position of a parameter among the axes, or `-1`. */
  slotOf: (parameterId: string) => number;
}

export function sweepSpace(results: SweepResults): SweepSpace {
  const axes = results.axes;
  const shape = axes.map((axis) => axis.values.length);
  const strides = stridesFor(shape);
  const total = shape.reduce((a, b) => a * b, 1);

  const cells = new Array<AnalysisPoint | undefined>(total).fill(undefined);
  for (const raw of results.cells) {
    const flat = flatten(raw.indices, shape, strides);
    if (flat == null) continue;
    cells[flat] = {
      success_rate: raw.success_rate,
      funding_success_rate: raw.funding_success_rate,
      p5: raw.p5,
      p50: raw.p50,
      p95: raw.p95,
    };
  }

  const planIndices = results.plan_indices ?? undefined;
  return {
    axes,
    shape,
    at: (indices) => {
      const flat = flatten(indices, shape, strides);
      return flat == null ? undefined : cells[flat];
    },
    plan: results.plan,
    planIndices:
      planIndices && planIndices.length === shape.length ? [...planIndices] : undefined,
    iterations: results.iterations,
    points: total,
    slotOf: (parameterId) => axes.findIndex((axis) => axis.parameter_id === parameterId),
  };
}

/** Row-major strides: the last axis varies fastest, as the server writes them. */
function stridesFor(shape: number[]): number[] {
  const strides = new Array<number>(shape.length).fill(1);
  for (let i = shape.length - 2; i >= 0; i--) strides[i] = strides[i + 1] * shape[i + 1];
  return strides;
}

function flatten(
  indices: readonly number[],
  shape: number[],
  strides: number[],
): number | undefined {
  if (indices.length !== shape.length) return undefined;
  let flat = 0;
  for (let i = 0; i < shape.length; i++) {
    const at = indices[i];
    if (!Number.isInteger(at) || at < 0 || at >= shape[i]) return undefined;
    flat += at * strides[i];
  }
  return flat;
}

/**
 * A short name for each swept variable, unique within the sweep.
 *
 * The server sends both a role ("amount") and a full label ("Travel budget ·
 * amount"). The role is what a chart axis wants, but a sweep can carry three
 * amounts, and three axes all labelled "amount" is worse than three long ones:
 * the role is used only where it identifies exactly one variable, and the whole
 * label where it does not.
 */
export function axisNames(axes: SweepAxis[]): Map<string, string> {
  const seen = new Map<string, number>();
  for (const axis of axes) seen.set(axis.role, (seen.get(axis.role) ?? 0) + 1);
  return new Map(
    axes.map((axis) => [
      axis.parameter_id,
      (seen.get(axis.role) ?? 0) > 1 ? axis.label : axis.role,
    ]),
  );
}

// ──────────────────────────── one graph ────────────────────────────

export type GraphKind = "line" | "heatmap" | "surface";

/**
 * Where a surface is looked at from, to start.
 *
 * Azimuth turns about the vertical, elevation lifts the camera above the
 * horizon. Both are per graph: an axonometric surface always hides one corner
 * behind its own ridge, so two surfaces of one sweep facing different ways is a
 * thing to want, not a thing to reconcile.
 */
export const DEFAULT_AZIMUTH = 45;
export const DEFAULT_ELEVATION = 20;

/**
 * Bounds on the elevation.
 *
 * Flat on the horizon collapses the grid to a line, and straight down collapses
 * the height to nothing — at either end the picture stops being a surface, and
 * the fit would scale the remains up to fill the frame.
 */
export const MIN_ELEVATION = 8;
export const MAX_ELEVATION = 80;

/** Whether a kind draws a second variable, and therefore needs a Y axis. */
export function needsY(kind: GraphKind): boolean {
  return kind !== "line";
}

/**
 * One graph, as the workspace holds it.
 *
 * Everything here is a choice about drawing, not about simulating: changing any
 * of it re-reads the finished sweep and none of it costs a run.
 */
export interface GraphSpec {
  id: string;
  kind: GraphKind;
  metric: MetricId;
  /**
   * Surface only: the metric its colour carries, when that is not the one its
   * height carries.
   *
   * A surface's height already says where the metric is high, so colouring it
   * by the same number spends a whole channel saying it twice. Pointed at a
   * second measure it answers a question neither a heatmap nor a plain surface
   * can — where a plan is both safe and rich. Absent means colour follows the
   * height, which is the honest default for one measure.
   */
  colour?: MetricId;
  /** Surface only: where the camera stands, in degrees. */
  azimuth?: number;
  elevation?: number;
  /** Parameter id on the horizontal axis. */
  x: string;
  /** Parameter id on the vertical axis; ignored by a line. */
  y: string | undefined;
  /**
   * Where each variable that is not on an axis is sliced, by parameter id.
   * Anything absent sits wherever the plan sits, which is the default a graph
   * should be read at.
   */
  held: Record<string, number>;
  /** Spans both columns of the layout. */
  wide: boolean;
}

/** One variable this graph is not drawing, and the value it is frozen at. */
export interface HeldVariable {
  axis: SweepAxis;
  /** Unique within this sweep — see `axisNames`. */
  name: string;
  index: number;
  value: number;
  /** Sitting where the plan sits — what "held at plan" means on the card. */
  atPlan: boolean;
}

/** A graph resolved against a particular sweep, ready to draw. */
export interface GraphView {
  spec: GraphSpec;
  metric: Metric;
  xAxis: SweepAxis;
  /** `undefined` for a line, and for a 2-D kind with nothing left to put on Y. */
  yAxis: SweepAxis | undefined;
  /** What to call each axis on this graph — unique within the sweep. */
  xName: string;
  yName: string | undefined;
  held: HeldVariable[];
  /** The metric at a position on this graph's own axes. */
  sample: (x: number, y?: number) => number | undefined;
  /** The band the drawn values are spread over. */
  span: Span;
  /**
   * What colour carries, and how to read it. The same as the metric above for
   * every kind but a surface told to colour itself by something else, so a
   * chart can shade from this trio without asking which case it is in.
   */
  colour: Metric;
  colourSpan: Span;
  sampleColour: (x: number, y?: number) => number | undefined;
  /** True where colour is saying something the height is not. */
  twoMeasures: boolean;
  /** Where the camera stands, in degrees. Only a surface reads these. */
  azimuth: number;
  elevation: number;
  /** Where the plan sits on this graph's axes, when it sits on them at all. */
  planX: number | undefined;
  planY: number | undefined;
}

/**
 * Resolve a graph against a finished sweep.
 *
 * `undefined` when the graph names a variable this sweep does not carry, which
 * happens when the swept set changed and the layout has not been reconciled.
 */
export function graphView(space: SweepSpace, spec: GraphSpec): GraphView | undefined {
  const xSlot = space.slotOf(spec.x);
  if (xSlot < 0) return undefined;
  const ySlot = needsY(spec.kind) && spec.y != null ? space.slotOf(spec.y) : -1;
  if (needsY(spec.kind) && spec.y != null && ySlot < 0) return undefined;

  const chosen = metric(spec.metric);
  const base = space.axes.map((axis, slot) => {
    if (slot === xSlot || slot === ySlot) return 0;
    const pinned = spec.held[axis.parameter_id];
    const planned = space.planIndices?.[slot];
    // The plan's own step is the default, and the middle of the axis is the
    // fallback when the plan falls outside the swept range: an unstated hold
    // should be the least surprising slice, never step zero by accident.
    const fallback = planned ?? Math.floor((axis.values.length - 1) / 2);
    return clampIndex(pinned ?? fallback, axis.values.length);
  });

  const sample = (x: number, y?: number): number | undefined => {
    const indices = [...base];
    indices[xSlot] = x;
    if (ySlot >= 0) indices[ySlot] = y ?? 0;
    const point = space.at(indices);
    return point ? chosen.of(point) : undefined;
  };

  const xAxis = space.axes[xSlot];
  const yAxis = ySlot >= 0 ? space.axes[ySlot] : undefined;

  const drawn = eachCell(xAxis, yAxis, sample);

  const names = axisNames(space.axes);
  const held: HeldVariable[] = space.axes.flatMap((axis, slot) => {
    if (slot === xSlot || slot === ySlot) return [];
    const index = base[slot];
    return [
      {
        axis,
        name: names.get(axis.parameter_id) ?? axis.role,
        index,
        value: axis.values[index],
        atPlan: space.planIndices?.[slot] === index,
      },
    ];
  });

  // Colour is its own measure only where a surface was told to point it
  // somewhere else; everything else shades by what it draws.
  const colour =
    spec.kind === "surface" && spec.colour != null && spec.colour !== spec.metric
      ? metric(spec.colour)
      : chosen;
  const sameMeasure = colour.id === chosen.id;
  const sampleColour = sameMeasure
    ? sample
    : (x: number, y?: number): number | undefined => {
        const indices = [...base];
        indices[xSlot] = x;
        if (ySlot >= 0) indices[ySlot] = y ?? 0;
        const point = space.at(indices);
        return point ? colour.of(point) : undefined;
      };

  const shaded = sameMeasure
    ? drawn
    : eachCell(xAxis, yAxis, sampleColour);

  return {
    spec,
    metric: chosen,
    xAxis,
    yAxis,
    xName: names.get(xAxis.parameter_id) ?? xAxis.role,
    yName: yAxis ? (names.get(yAxis.parameter_id) ?? yAxis.role) : undefined,
    held,
    sample,
    span: chosen.span(drawn),
    colour,
    colourSpan: colour.span(shaded),
    sampleColour,
    twoMeasures: !sameMeasure,
    azimuth: wrapAzimuth(spec.azimuth ?? DEFAULT_AZIMUTH),
    elevation: clampElevation(spec.elevation ?? DEFAULT_ELEVATION),
    planX: space.planIndices?.[xSlot],
    planY: ySlot >= 0 ? space.planIndices?.[ySlot] : undefined,
  };
}

/** Bring a turn back into 0–360, so a drag past the end keeps going. */
export function wrapAzimuth(azimuth: number): number {
  if (!Number.isFinite(azimuth)) return DEFAULT_AZIMUTH;
  return ((azimuth % 360) + 360) % 360;
}

/** Keep a camera above the horizon and below straight down. */
export function clampElevation(elevation: number): number {
  if (!Number.isFinite(elevation)) return DEFAULT_ELEVATION;
  return Math.min(MAX_ELEVATION, Math.max(MIN_ELEVATION, elevation));
}

/** Every measured value a graph draws, for deciding the band to draw it over. */
function eachCell(
  xAxis: SweepAxis,
  yAxis: SweepAxis | undefined,
  read: (x: number, y?: number) => number | undefined,
): number[] {
  const values: number[] = [];
  for (let x = 0; x < xAxis.values.length; x++) {
    if (yAxis) {
      for (let y = 0; y < yAxis.values.length; y++) {
        const value = read(x, y);
        if (value != null) values.push(value);
      }
    } else {
      const value = read(x);
      if (value != null) values.push(value);
    }
  }
  return values;
}

function clampIndex(index: number, length: number): number {
  if (!Number.isInteger(index)) return 0;
  return Math.min(length - 1, Math.max(0, index));
}

/** What a graph is called on its card, and what its heading qualifies it with. */
export function graphTitle(view: GraphView): { title: string; sub: string } {
  return {
    title: view.metric.label,
    sub: view.yName ? `${view.yName} × ${view.xName}` : `by ${view.xName}`,
  };
}

/** What a card says it is holding, in one line. */
export function heldSummary(view: GraphView): string {
  if (view.held.length === 0) return "every swept variable is on an axis";
  return view.held
    .map((h) => `${h.name} ${paramTick(h.axis.kind, h.value)}${h.atPlan ? "" : " (pinned)"}`)
    .join(" · ");
}

// ─────────────────────────── the layout ───────────────────────────

/**
 * The graphs a fresh sweep opens on.
 *
 * One heatmap of the first two variables, because the pair is the thing a sweep
 * was run to see, and a line through the first, because a line is what anyone
 * reads a number off. A one-variable sweep has no grid to draw, so it gets the
 * line alone rather than a heatmap of one column.
 */
export function defaultGraphs(axes: SweepAxis[]): GraphSpec[] {
  if (axes.length === 0) return [];
  const [first, second] = axes;
  const line: GraphSpec = {
    id: freshId(),
    kind: "line",
    metric: "success",
    x: first.parameter_id,
    y: undefined,
    held: {},
    wide: axes.length < 2,
  };
  if (!second) return [line];
  return [
    {
      id: freshId(),
      kind: "heatmap",
      metric: "success",
      x: first.parameter_id,
      y: second.parameter_id,
      held: {},
      wide: false,
    },
    line,
  ];
}

/** A graph added by hand: the first two variables, drawn a way nothing else is. */
export function newGraph(axes: SweepAxis[], existing: GraphSpec[]): GraphSpec | undefined {
  if (axes.length === 0) return undefined;
  const taken = new Set(existing.map((g) => `${g.kind}:${g.metric}:${g.x}:${g.y ?? ""}`));
  const kinds: GraphKind[] = axes.length > 1 ? ["heatmap", "line", "surface"] : ["line"];
  for (const kind of kinds) {
    for (const chosen of METRICS) {
      const spec: GraphSpec = {
        id: freshId(),
        kind,
        metric: chosen.id,
        x: axes[0].parameter_id,
        y: needsY(kind) ? axes[1]?.parameter_id : undefined,
        held: {},
        wide: false,
      };
      if (!taken.has(`${spec.kind}:${spec.metric}:${spec.x}:${spec.y ?? ""}`)) return spec;
    }
  }
  // Every combination is already on screen; a duplicate is a fine thing to add.
  return {
    id: freshId(),
    kind: kinds[0],
    metric: "success",
    x: axes[0].parameter_id,
    y: needsY(kinds[0]) ? axes[1]?.parameter_id : undefined,
    held: {},
    wide: false,
  };
}

/**
 * Point an existing layout at a new sweep.
 *
 * Re-running over a different set of variables must not empty the screen: a
 * graph whose axis is gone is moved onto one that is there, keeping its kind
 * and its metric, and holds that no longer name a swept variable are dropped.
 * Only a sweep with nothing in common falls back to the defaults.
 */
export function reconcile(specs: GraphSpec[], axes: SweepAxis[]): GraphSpec[] {
  if (axes.length === 0) return [];
  const ids = new Set(axes.map((axis) => axis.parameter_id));
  if (specs.length === 0) return defaultGraphs(axes);

  const next = specs.map((spec) => {
    const x = ids.has(spec.x) ? spec.x : axes[0].parameter_id;
    let y = spec.y != null && ids.has(spec.y) ? spec.y : undefined;
    if (needsY(spec.kind) && (y == null || y === x)) {
      y = axes.find((axis) => axis.parameter_id !== x)?.parameter_id;
    }
    // A 2-D kind with only one variable left has nothing to draw; it becomes
    // the line through the variable it kept rather than an empty frame.
    const kind: GraphKind = needsY(spec.kind) && y == null ? "line" : spec.kind;
    // A hold only means anything for a variable that is still swept and is
    // not itself on an axis: one that has been promoted to an axis would sit
    // in the record as a value nothing reads and would spring back if the
    // graph were ever pointed somewhere else.
    const held = Object.fromEntries(
      Object.entries(spec.held).filter(
        ([parameterId]) => ids.has(parameterId) && parameterId !== x && parameterId !== y,
      ),
    );
    return { ...spec, kind, x, y: needsY(kind) ? y : undefined, held };
  });

  return next;
}

let counter = 0;

function freshId(): string {
  counter += 1;
  return `g${counter}`;
}

/**
 * A stored layout, checked before anything is drawn from it.
 *
 * The server keeps the workspace as the JSON the client sent and never reads
 * into it, so this is the boundary where it becomes graphs again: anything
 * whose kind, metric or X axis is not one this build knows is dropped rather
 * than drawn, and a layout with nothing left in it reads as no layout, which
 * opens on the defaults. Graphs pointing at variables that are no longer swept
 * are kept — `reconcile` moves those, and losing a card because the sweep
 * changed is exactly what it exists to prevent.
 *
 * Ids come back with the layout, so the generator is advanced past them: a
 * restored `g2` and a freshly added `g2` would be one card as far as React and
 * the selection are concerned.
 */
export function parseLayout(value: unknown): GraphSpec[] | undefined {
  if (!Array.isArray(value)) return undefined;

  const specs: GraphSpec[] = [];
  const seen = new Set<string>();
  for (const raw of value) {
    const spec = parseGraph(raw);
    if (!spec || seen.has(spec.id)) continue;
    seen.add(spec.id);
    specs.push(spec);
  }
  if (specs.length === 0) return undefined;

  for (const id of seen) {
    const suffix = /^g(\d+)$/.exec(id);
    if (suffix) counter = Math.max(counter, Number(suffix[1]));
  }
  return specs;
}

const KINDS: readonly GraphKind[] = ["line", "heatmap", "surface"];

function parseGraph(raw: unknown): GraphSpec | undefined {
  if (typeof raw !== "object" || raw === null) return undefined;
  const graph = raw as Record<string, unknown>;

  const id = typeof graph.id === "string" && graph.id.length > 0 ? graph.id : undefined;
  const kind = KINDS.find((k) => k === graph.kind);
  const chosen = METRICS.find((m) => m.id === graph.metric)?.id;
  const x = typeof graph.x === "string" ? graph.x : undefined;
  if (!id || !kind || !chosen || !x) return undefined;

  const held: Record<string, number> = {};
  if (typeof graph.held === "object" && graph.held !== null) {
    for (const [parameterId, at] of Object.entries(graph.held)) {
      if (typeof at === "number" && Number.isFinite(at)) held[parameterId] = Math.trunc(at);
    }
  }

  const y = typeof graph.y === "string" ? graph.y : undefined;
  return {
    id,
    kind,
    metric: chosen,
    colour: METRICS.find((m) => m.id === graph.colour)?.id,
    azimuth: angle(graph.azimuth),
    elevation: angle(graph.elevation),
    x,
    y: needsY(kind) ? y : undefined,
    held,
    wide: graph.wide === true,
  };
}

/** A camera angle, or nothing — a stored `null` means "wherever the default is". */
function angle(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

// ──────────────────────────── the export ────────────────────────────

/** Every evaluated combination as CSV: the variables, then what they produced. */
export function sweepCsv(space: SweepSpace): string {
  const header = [
    ...space.axes.map((axis) => axis.label),
    "success_rate",
    "funding_success_rate",
    "p5",
    "p50",
    "p95",
  ];

  const rows: string[] = [header.map(quote).join(",")];
  const walk = (slot: number, indices: number[]) => {
    if (slot === space.shape.length) {
      const point = space.at(indices);
      if (!point) return;
      rows.push(
        [
          ...indices.map((at, i) => String(space.axes[i].values[at])),
          point.success_rate,
          point.funding_success_rate ?? "",
          point.p5,
          point.p50,
          point.p95,
        ].join(","),
      );
      return;
    }
    for (let at = 0; at < space.shape[slot]; at++) walk(slot + 1, [...indices, at]);
  };
  walk(0, []);
  return rows.join("\n");
}

function quote(field: string): string {
  return /[",\n]/.test(field) ? `"${field.replace(/"/g, '""')}"` : field;
}
