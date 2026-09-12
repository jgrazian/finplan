/**
 * Analysis payloads → what the Sweep and Solve screens draw.
 *
 * The server measures and sends nothing derived. How a sweep is laid out over
 * graphs lives in `./sweep.ts`; what is here is the vocabulary the whole tab
 * shares — how a parameter is named and how its values read — plus the two
 * analyses that have one shape each, the ranking and the goal seek.
 */
import type {
  AnalysisParameter,
  SensitivityResults,
  SolveOutcome,
  SolveStep,
} from "@/lib/api/types";
import { fmtCompact, fmtCurrency, fmtPercent } from "../format.ts";

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

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
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
