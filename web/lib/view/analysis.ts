/**
 * Analysis payloads → what the Sweep and Solve screens draw.
 *
 * The server sends one grid of measured cells and nothing derived from it: the
 * safety threshold, the frontier it implies, the slices through a cell and the
 * frontier table are all computed here. That is what lets the threshold field
 * re-read a finished sweep instead of asking for another one.
 */
import type {
  AnalysisParameter,
  AnalysisPoint,
  SensitivityResults,
  SolveOutcome,
  SolveStep,
  SweepAxis,
  SweepResults,
} from "@/lib/api/types";
import { fmtCompact, fmtCurrency, fmtPercent } from "../format.ts";

/**
 * The heatmap ramp: one hue, the design system's accent scale from its darkest
 * step to its lightest. Dark reads as low, which puts the weight of the image
 * on the corner of the grid where the plan fails.
 */
export const SUCCESS_RAMP = [
  "#1d2d3d",
  "#2c455d",
  "#41617f",
  "#597ea3",
  "#749dc4",
  "#94bce3",
  "#b5d9fd",
  "#eef6ff",
] as const;

/**
 * The narrowest band the ramp will spread itself over, in fractions of success.
 *
 * A grid that only ever moves two points is a grid with no story in it, and
 * stretching the full ramp across those two points invents one. Below this the
 * band is widened instead, so a flat grid looks flat.
 */
const MIN_RAMP_SPAN = 0.1;

/** The success range a grid's colours are spread over. */
export interface Ramp {
  low: number;
  high: number;
  /** Where a rate lands on the ramp. */
  fill: (rate: number) => string;
}

/**
 * Spread the ramp over the range the grid actually covers.
 *
 * Fixing it to 0–100% would be honest and useless: real plans live between 80%
 * and 100%, and every cell would come out the same near-white. The legend
 * quotes the two ends, so the reader is told what the colours mean rather than
 * left to assume the full scale.
 */
export function successRamp(rates: number[]): Ramp {
  const observed = rates.filter((r) => Number.isFinite(r));
  let low = observed.length > 0 ? Math.min(...observed) : 0;
  let high = observed.length > 0 ? Math.max(...observed) : 1;
  const short = MIN_RAMP_SPAN - (high - low);
  if (short > 0) {
    low = Math.max(0, low - short / 2);
    high = Math.min(1, low + MIN_RAMP_SPAN);
    low = Math.max(0, high - MIN_RAMP_SPAN);
  }
  const span = high - low || 1;
  return {
    low,
    high,
    fill: (rate) => {
      const step = Math.round(clamp01((rate - low) / span) * (SUCCESS_RAMP.length - 1));
      return SUCCESS_RAMP[step];
    },
  };
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

/** A parameter as a monospace identifier: `retirement-spending.amount`. */
export function paramId(parameter: { event_name: string; role: string }): string {
  return `${slug(parameter.event_name)}.${slug(parameter.role)}`;
}

function slug(text: string): string {
  return (
    text
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "") || "unnamed"
  );
}

/** A value in the parameter's own units — an age is a number, money is money. */
export function paramValue(kind: string, value: number): string {
  return kind === "amount" ? fmtCurrency(value) : String(Math.round(value));
}

/** The same, abbreviated for an axis tick. */
export function paramTick(kind: string, value: number): string {
  return kind === "amount" ? fmtCompact(value) : String(Math.round(value));
}

// ───────────────────────────── sweep ─────────────────────────────

/** One measured combination, with its place on the two axes resolved. */
export interface Cell {
  /** Column, along the first requested axis. */
  x: number;
  /** Row, along the second axis; always 0 for a one-axis sweep. */
  y: number;
  point: AnalysisPoint;
  /** Clears the safety threshold. */
  safe: boolean;
}

export interface SweepView {
  /** The horizontal axis: the first parameter the request named. */
  xAxis: SweepAxis;
  /** The vertical axis, or `undefined` for a one-parameter sweep. */
  yAxis: SweepAxis | undefined;
  columns: number;
  rows: number;
  cells: Cell[];
  /** `cells` indexed by `[x][y]`, for the readouts and slices. */
  at: (x: number, y: number) => Cell | undefined;
  /** The plan as it stands, for every "vs plan" figure. */
  plan: AnalysisPoint;
  /** Where the plan sits on the grid, when it sits on it at all. */
  planCell: { x: number; y: number } | undefined;
  /**
   * The last safe row in each column, walking the direction success falls, or
   * `null` where the column has no safe cell at all.
   */
  frontier: Array<number | null>;
  /** True when success falls as the vertical axis rises — spending, say. */
  fallsWithY: boolean;
  /** The colour scale, spread over the range these cells actually cover. */
  ramp: Ramp;
  threshold: number;
  iterations: number;
}

/**
 * Resolve a finished sweep against a safety threshold.
 *
 * `threshold` is a fraction, and is the only input that is not from the server:
 * moving it re-derives the frontier without touching the measurements.
 */
export function sweepView(results: SweepResults, threshold: number): SweepView {
  const [xAxis, yAxis] = results.axes;
  const columns = xAxis?.values.length ?? 0;
  const rows = yAxis?.values.length ?? 1;

  const grid: Array<Array<Cell | undefined>> = Array.from({ length: columns }, () =>
    Array.from({ length: rows }, () => undefined),
  );
  const cells: Cell[] = [];
  for (const raw of results.cells) {
    // The server sends indices in the order the axes were requested, so the
    // first is the column whether or not there is a second.
    const x = raw.indices[0] ?? 0;
    const y = raw.indices[1] ?? 0;
    if (x >= columns || y >= rows) continue;
    const cell: Cell = {
      x,
      y,
      point: {
        success_rate: raw.success_rate,
        funding_success_rate: raw.funding_success_rate,
        p5: raw.p5,
        p50: raw.p50,
        p95: raw.p95,
      },
      safe: raw.success_rate >= threshold,
    };
    grid[x][y] = cell;
    cells.push(cell);
  }

  const at = (x: number, y: number) => grid[x]?.[y];

  // Which way the vertical axis makes things worse. Read off the data rather
  // than assumed from the parameter: an axis of contributions and an axis of
  // withdrawals move success in opposite directions.
  const fallsWithY = rows > 1 && meanSuccessAtRow(grid, 0) >= meanSuccessAtRow(grid, rows - 1);

  const frontier = Array.from({ length: columns }, (_, x) => {
    const order = fallsWithY
      ? Array.from({ length: rows }, (_, i) => rows - 1 - i)
      : Array.from({ length: rows }, (_, i) => i);
    for (const y of order) {
      if (at(x, y)?.safe) return y;
    }
    return null;
  });

  const planIndices = results.plan_indices;
  return {
    xAxis,
    yAxis,
    columns,
    rows,
    cells,
    at,
    plan: results.plan,
    planCell:
      planIndices == null ? undefined : { x: planIndices[0] ?? 0, y: planIndices[1] ?? 0 },
    frontier,
    fallsWithY,
    ramp: successRamp(cells.map((c) => c.point.success_rate)),
    threshold,
    iterations: results.iterations,
  };
}

function meanSuccessAtRow(grid: Array<Array<Cell | undefined>>, y: number): number {
  const values = grid.map((column) => column[y]?.point.success_rate).filter(isNumber);
  return values.length === 0 ? 0 : values.reduce((a, b) => a + b, 0) / values.length;
}

function isNumber(v: number | undefined): v is number {
  return v != null;
}

/** One row of the "highest safe value by column" table. */
export interface FrontierRow {
  x: number;
  xValue: number;
  /** The safe extreme on the vertical axis, or `undefined` where none is. */
  yValue: number | undefined;
  point: AnalysisPoint | undefined;
  /** Change from the plan's own vertical value, when the plan is on the grid. */
  delta: number | undefined;
}

/** The frontier as a table you can read without hovering the grid. */
export function frontierRows(view: SweepView): FrontierRow[] {
  const planY =
    view.planCell && view.yAxis ? view.yAxis.values[view.planCell.y] : undefined;

  return view.xAxis.values.map((xValue, x) => {
    const y = view.frontier[x];
    const cell = y == null ? undefined : view.at(x, y);
    const yValue = y == null ? undefined : view.yAxis?.values[y];
    return {
      x,
      xValue,
      yValue,
      point: cell?.point,
      delta: yValue != null && planY != null ? yValue - planY : undefined,
    };
  });
}

/** A line of success rates through one row or column of the grid. */
export interface Slice {
  axis: SweepAxis;
  points: Array<{ value: number; success: number }>;
}

/** Success against the horizontal axis, at a fixed row. */
export function sliceAlongX(view: SweepView, y: number): Slice {
  return {
    axis: view.xAxis,
    points: view.xAxis.values.map((value, x) => ({
      value,
      success: view.at(x, y)?.point.success_rate ?? 0,
    })),
  };
}

/** Success against the vertical axis, at a fixed column. */
export function sliceAlongY(view: SweepView, x: number): Slice | undefined {
  if (!view.yAxis) return undefined;
  const axis = view.yAxis;
  return {
    axis,
    points: axis.values.map((value, y) => ({
      value,
      success: view.at(x, y)?.point.success_rate ?? 0,
    })),
  };
}

// ─────────────────────────── sensitivity ───────────────────────────

/** The ranking, and the success axis its bars are drawn against. */
export interface SensitivityChart {
  rows: SensitivityView[];
  /** The success axis the bars span, as fractions. */
  low: number;
  high: number;
}

/** One ranked parameter, with the bar geometry its row draws. */
export interface SensitivityView {
  parameterId: string;
  label: string;
  kind: string;
  lowValue: number;
  highValue: number;
  lowSuccess: number;
  highSuccess: number;
  /** Points of success between the two ends. */
  span: number;
  /** Bar extent as fractions of the chart's own success axis. */
  barStart: number;
  barWidth: number;
}

/**
 * The narrowest success axis the ranking will draw over.
 *
 * Wider than the heatmap's, because a bar has to be visible as a bar: five
 * points of range across the chart still leaves a one-point mover as a stub.
 */
const MIN_SENSITIVITY_SPAN = 0.05;

/**
 * The ±band ranking, ready to draw.
 *
 * Drawn against the range the parameters actually cover rather than 0–100%.
 * Every real plan clusters in the last few points of the scale, and a full-width
 * axis renders the whole ranking as identical slivers at the right edge —
 * destroying exactly the comparison the screen exists to make.
 */
export function sensitivityView(results: SensitivityResults): SensitivityChart {
  const ends = results.rows.flatMap((row) => [
    clamp01(row.low.success_rate),
    clamp01(row.high.success_rate),
  ]);
  // The plan's own rate is marked on every bar, so the axis has to reach it.
  ends.push(clamp01(results.plan.success_rate));

  let low = Math.min(...ends);
  let high = Math.max(...ends);
  const short = MIN_SENSITIVITY_SPAN - (high - low);
  if (short > 0) {
    high = Math.min(1, high + short / 2);
    low = Math.max(0, high - MIN_SENSITIVITY_SPAN);
    high = Math.min(1, low + MIN_SENSITIVITY_SPAN);
  }
  const span = high - low || 1;
  const at = (rate: number) => clamp01((clamp01(rate) - low) / span);

  const rows = results.rows.map((row) => {
    const lowSuccess = clamp01(row.low.success_rate);
    const highSuccess = clamp01(row.high.success_rate);
    return {
      parameterId: row.parameter_id,
      label: row.label,
      kind: row.kind,
      lowValue: row.low_value,
      highValue: row.high_value,
      lowSuccess,
      highSuccess,
      span: row.span,
      barStart: Math.min(at(lowSuccess), at(highSuccess)),
      barWidth: Math.abs(at(highSuccess) - at(lowSuccess)),
    };
  });

  return { rows, low, high };
}

// ───────────────────────────── solve ─────────────────────────────

/** One line of the plan-versus-optimal comparison. */
export interface SolveRow {
  label: string;
  mono?: boolean;
  plan: string;
  best: string;
  delta: string;
}

/**
 * The comparison table: each varied parameter, then the outcome measures that
 * moved with it.
 */
export function solveRows(outcome: SolveOutcome): SolveRow[] {
  const best = outcome.best;
  const rows: SolveRow[] = outcome.parameters.map((parameter, i) => {
    const from = parameter.current;
    const to = best?.values[i];
    return {
      label: paramId(parameter),
      mono: true,
      plan: paramValue(parameter.kind, from),
      best: to == null ? "—" : paramValue(parameter.kind, to),
      delta: to == null ? "—" : signed(parameter.kind, to - from),
    };
  });

  rows.push({
    label: "Success",
    plan: fmtPercent(outcome.plan.success_rate),
    best: best ? fmtPercent(best.success_rate) : "—",
    delta: best ? "meets the constraint" : "—",
  });
  rows.push({
    label: "P50 terminal",
    plan: fmtCompact(outcome.plan.p50),
    best: best ? fmtCompact(best.p50) : "—",
    delta: best ? deltaMoney(best.p50 - outcome.plan.p50) : "—",
  });
  rows.push({
    label: "P5 terminal",
    plan: fmtCompact(outcome.plan.p5),
    best: best ? fmtCompact(best.p5) : "—",
    delta: best ? deltaMoney(best.p5 - outcome.plan.p5) : "—",
  });
  return rows;
}

function signed(kind: string, delta: number): string {
  if (Math.abs(delta) < 1e-9) return "unchanged";
  const sign = delta > 0 ? "+" : "−";
  return sign + paramValue(kind, Math.abs(delta));
}

function deltaMoney(delta: number): string {
  if (Math.abs(delta) < 1) return "unchanged";
  return (delta > 0 ? "+" : "−") + fmtCompact(Math.abs(delta)).replace("−", "");
}

/** The headline figure: the objective's value at the answer. */
export function solveHeadline(outcome: SolveOutcome): string {
  const best = outcome.best;
  if (!best) return "no answer";
  const parameter = outcome.parameters[0];
  return parameter ? paramValue(parameter.kind, best.values[0]) : fmtPercent(best.success_rate);
}

/** Every probe, in the order taken, for the convergence plot. */
export function convergenceSteps(outcome: SolveOutcome): SolveStep[] {
  return outcome.steps;
}

/** The parameter a solve's answer is quoted in, when it has exactly one. */
export function solvedParameter(outcome: SolveOutcome): AnalysisParameter | undefined {
  return outcome.parameters.length === 1 ? outcome.parameters[0] : undefined;
}
