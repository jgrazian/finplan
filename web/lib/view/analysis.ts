/**
 * Analysis payloads → what the Sweep screen draws.
 *
 * The server measures and sends nothing derived. How a sweep is laid out over
 * graphs lives in `./sweep.ts`; what is here is the vocabulary the whole tab
 * shares — how a parameter is named and how its values read — plus the
 * sensitivity ranking, which has one shape.
 */
import type { SensitivityResults } from "@/lib/api/types";
import { fmtCompact, fmtCurrency } from "../format.ts";

/** Stable named input, shared with the Plan parameters editor. */
export function paramId(parameter: { name: string }): string {
  return parameter.name;
}

/** The API represents calendar dates as integral UTC epoch days. */
export function parameterDate(value: number): string {
  return new Date(Math.round(value) * 86_400_000).toISOString().slice(0, 10);
}

export function parameterDay(date: string): number {
  return Date.parse(`${date}T00:00:00Z`) / 86_400_000;
}

export function paramValue(kind: string, value: number): string {
  if (kind === "amount") return fmtCurrency(value);
  if (kind === "rate") return `${Number((value * 100).toFixed(6))}%`;
  if (kind === "date") return parameterDate(value);
  if (kind === "age") {
    const months = Math.round(value * 12);
    return `${Math.floor(months / 12)} yr${months % 12 ? ` ${months % 12} mo` : ""}`;
  }
  return String(Math.round(value));
}

export function paramTick(kind: string, value: number): string {
  return kind === "amount" ? fmtCompact(value) : paramValue(kind, value);
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
export function sensitivityView(results: SensitivityResults, metric: "funding" | "success" = "funding"): SensitivityChart {
  const read = (point: { success_rate: number; funding_success_rate: number | null }) => metric === "funding" ? point.funding_success_rate : point.success_rate;
  const measured = results.rows.filter((row) => read(row.low) != null && read(row.high) != null);
  const ends = measured.flatMap((row) => [
    clamp01(read(row.low)!),
    clamp01(read(row.high)!),
  ]);
  // The plan's own rate is marked on every bar, so the axis has to reach it.
  if (read(results.plan) != null) ends.push(clamp01(read(results.plan)!));
  if (ends.length === 0) ends.push(0, 1);

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

  const rows = measured.map((row) => {
    const lowSuccess = clamp01(read(row.low)!);
    const highSuccess = clamp01(read(row.high)!);
    return {
      parameterId: row.parameter_id,
      label: row.label,
      kind: row.kind,
      lowValue: row.low_value,
      highValue: row.high_value,
      lowSuccess,
      highSuccess,
      span: Math.abs(highSuccess - lowSuccess) * 100,
      barStart: Math.min(at(lowSuccess), at(highSuccess)),
      barWidth: Math.abs(at(highSuccess) - at(lowSuccess)),
    };
  });

  rows.sort((a, b) => b.span - a.span);
  return { rows, low, high };
}
