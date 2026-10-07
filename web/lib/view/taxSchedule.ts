/**
 * A tax config as the editor holds it, and what its schedule does to a year.
 *
 * The API keeps rates as fractions and thresholds over taxable income; the
 * editor shows percents, and its chart and worked example put the thresholds
 * on a gross-income axis by adding the standard deduction back, which is how
 * the engine applies them (`TaxConfig::brackets_with_deduction`).
 */
import type { CreateTaxConfig, TaxConfig } from "@/lib/api/types";

/** The engine's `TaxConfig::NIIT_RATE`. */
export const NIIT_RATE = 0.038;

/** Where NIIT starts when it is switched on: the single filer's threshold. */
export const NIIT_DEFAULT_THRESHOLD = 200_000;

/** One row of a bracket table: from `threshold` of taxable income, `rate` percent. */
export interface BracketDraft {
  threshold: number;
  rate: number;
}

export interface TaxDraft {
  name: string;
  description: string | null;
  /** Percent. */
  stateRate: number;
  /** Percent. */
  penaltyRate: number;
  standardDeduction: number;
  age65Extra: number;
  /** Null: no net investment income tax. */
  niitThreshold: number | null;
  federal: BracketDraft[];
  gains: BracketDraft[];
}

/** `0.1 * 100` is `10.000000000000002`; a percent is shown to four places at most. */
const toPercent = (fraction: number) => Math.round(fraction * 1e6) / 1e4;
const toFraction = (percent: number) => Math.round(percent * 1e4) / 1e6;

export function toTaxDraft(config: TaxConfig): TaxDraft {
  const rows = (brackets: TaxConfig["federal_brackets"]) =>
    brackets.map((b) => ({ threshold: b.threshold, rate: toPercent(b.rate) }));
  return {
    name: config.name,
    description: config.description,
    stateRate: toPercent(config.state_rate),
    penaltyRate: toPercent(config.early_withdrawal_penalty_rate),
    standardDeduction: config.standard_deduction,
    age65Extra: config.age_65_extra_deduction,
    niitThreshold: config.niit_threshold,
    federal: rows(config.federal_brackets),
    gains: rows(config.capital_gains_brackets),
  };
}

/**
 * The whole draft as a create body. An update sends the same fields, where
 * `niit_threshold: null` removes the tax — which is what a draft with it off
 * means.
 */
export function fromTaxDraft(draft: TaxDraft): CreateTaxConfig {
  const rows = (brackets: BracketDraft[]) =>
    brackets.map((b) => ({ threshold: b.threshold, rate: toFraction(b.rate) }));
  return {
    name: draft.name.trim(),
    description: draft.description,
    state_rate: toFraction(draft.stateRate),
    early_withdrawal_penalty_rate: toFraction(draft.penaltyRate),
    standard_deduction: draft.standardDeduction,
    age_65_extra_deduction: draft.age65Extra,
    niit_threshold: draft.niitThreshold,
    federal_brackets: rows(draft.federal),
    capital_gains_brackets: rows(draft.gains),
  };
}

/** Bracket tables the engine could not walk, said the way the server would. */
export function bracketProblem(rows: BracketDraft[]): string | undefined {
  if (rows.length === 0) return "Add at least one bracket.";
  if (rows[0].threshold !== 0) return "The first bracket must start at $0.";
  for (let i = 1; i < rows.length; i++) {
    if (rows[i].threshold <= rows[i - 1].threshold) return "Thresholds must rise row by row.";
  }
  if (rows.some((b) => b.rate < 0 || b.rate > 100)) return "Rates are 0–100%.";
  return undefined;
}

/** What stops the draft from saving, if anything. */
export function draftProblem(draft: TaxDraft): string | undefined {
  if (!draft.name.trim()) return "Enter a name.";
  const federal = bracketProblem(draft.federal);
  if (federal) return `Ordinary brackets: ${federal}`;
  const gains = bracketProblem(draft.gains);
  if (gains) return `Gains brackets: ${gains}`;
  if (draft.stateRate > 100 || draft.penaltyRate > 100) return "Rates are 0–100%.";
  return undefined;
}

export function sameDraft(a: TaxDraft, b: TaxDraft): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

// ── The chart ────────────────────────────────────────────────────────────

/** A band of the chart, its position in percent of the axis. */
export interface ScheduleSegment {
  left: number;
  width: number;
  /** Percent; null for the standard deduction's hatched band. */
  rate: number | null;
  title: string;
}

export interface ScheduleTick {
  left: number;
  label: string;
  align: "start" | "middle" | "end";
}

export interface ScheduleChart {
  ordinary: ScheduleSegment[];
  gains: ScheduleSegment[];
  niit: ScheduleSegment[];
  ticks: ScheduleTick[];
  /** Gross income → percent of the axis. */
  at: (income: number) => number;
}

const NICE_MAXIMA = [700_000, 1_000_000, 1_500_000, 2_000_000, 3_000_000, 5_000_000, 10_000_000];
const TICKS = [0, 25_000, 50_000, 100_000, 200_000, 400_000];

const money = (n: number) => `$${Math.round(n).toLocaleString("en-US")}`;
const short = (n: number) =>
  n === 0 ? "$0" : n >= 1_000_000 ? `$${Number((n / 1e6).toFixed(1))}M` : `$${Math.round(n / 1000)}k`;

/**
 * The schedule on a gross-income axis, square-root scaled so the low brackets
 * — where most years fall — get the room. The axis runs past the highest
 * threshold (or the example year) so the top band always shows.
 */
export function scheduleChart(draft: TaxDraft, example: { ordinary: number; gains: number }): ScheduleChart {
  const sd = Math.max(0, draft.standardDeduction);
  const tops = [
    ...draft.federal.map((b) => sd + b.threshold),
    ...draft.gains.map((b) => sd + b.threshold),
    draft.niitThreshold ?? 0,
    example.ordinary + example.gains,
  ];
  const needed = Math.max(...tops) * 1.15;
  const max = NICE_MAXIMA.find((m) => m >= needed) ?? needed;
  const at = (x: number) => Math.sqrt(Math.max(0, Math.min(x, max)) / max) * 100;

  const segment = (from: number, to: number, rate: number | null): ScheduleSegment => ({
    left: at(from),
    width: at(to) - at(from),
    rate,
    title:
      rate == null
        ? `Standard deduction: the first ${money(to)} is untaxed`
        : `${money(from)}${to >= max ? "+" : `–${money(to)}`} at ${rate}%`,
  });
  const bands = (rows: BracketDraft[]) => {
    const sorted = [...rows].sort((a, b) => a.threshold - b.threshold);
    const out = sd > 0 ? [segment(0, sd, null)] : [];
    sorted.forEach((b, i) => {
      const from = sd + b.threshold;
      const to = i + 1 < sorted.length ? sd + sorted[i + 1].threshold : max;
      if (from < max && to > from) out.push(segment(from, Math.min(to, max), b.rate));
    });
    return out;
  };

  const values = [...TICKS.filter((v) => at(v) < at(max) - 9), max];
  return {
    ordinary: bands(draft.federal),
    gains: bands(draft.gains),
    niit:
      draft.niitThreshold != null && draft.niitThreshold < max
        ? [segment(draft.niitThreshold, max, NIIT_RATE * 100)]
        : [],
    ticks: values.map((v, i) => ({
      left: at(v),
      label: short(v) + (v === max ? "+" : ""),
      align: i === 0 ? "start" : i === values.length - 1 ? "end" : "middle",
    })),
    at,
  };
}

// ── A worked year ────────────────────────────────────────────────────────

export interface GainsPiece {
  label: string;
  /** Percent; null inside the standard deduction. */
  rate: number | null;
  amount: number;
  tax: number;
}

export interface YearTax {
  /** Federal tax on the ordinary income alone. */
  ordinaryTax: number;
  /** The ordinary bracket the last dollar of ordinary income lands in, percent. */
  marginal: number;
  /** Where the gains land, band by band, and NIIT on top. */
  pieces: GainsPiece[];
  gainsTax: number;
}

/**
 * One year's federal tax: the ordinary income through the brackets past the
 * deduction, then the long-term gains stacked on top of it, each slice at the
 * rate of the band it lands in, and NIIT on the gains or on the income over
 * its threshold, whichever is smaller. Gross income stands in for MAGI.
 */
export function yearTax(draft: TaxDraft, ordinary: number, gains: number): YearTax {
  const sd = Math.max(0, draft.standardDeduction);
  const federal = [...draft.federal].sort((a, b) => a.threshold - b.threshold);
  const taxable = Math.max(0, ordinary - sd);
  let ordinaryTax = 0;
  let marginal = 0;
  federal.forEach((b, i) => {
    const end = i + 1 < federal.length ? federal[i + 1].threshold : Infinity;
    ordinaryTax += (Math.max(0, Math.min(taxable, end) - b.threshold) * b.rate) / 100;
    if (taxable > b.threshold) marginal = b.rate;
  });

  const schedule = [...draft.gains].sort((a, b) => a.threshold - b.threshold);
  const bands = [
    { from: 0, to: sd, rate: null as number | null, label: "0% · inside the standard deduction" },
    ...schedule.map((b, i) => ({
      from: sd + b.threshold,
      to: i + 1 < schedule.length ? sd + schedule[i + 1].threshold : Infinity,
      rate: b.rate as number | null,
      label: `${b.rate}%`,
    })),
  ];
  const pieces: GainsPiece[] = [];
  for (const band of bands) {
    const amount = Math.max(0, Math.min(ordinary + gains, band.to) - Math.max(ordinary, band.from));
    if (amount > 0.5) {
      pieces.push({ label: band.label, rate: band.rate, amount, tax: (amount * (band.rate ?? 0)) / 100 });
    }
  }
  if (draft.niitThreshold != null) {
    const amount = Math.min(gains, Math.max(0, ordinary + gains - draft.niitThreshold));
    if (amount > 0.5) {
      pieces.push({
        label: `NIIT 3.8% · over ${money(draft.niitThreshold)}`,
        rate: NIIT_RATE * 100,
        amount,
        tax: amount * NIIT_RATE,
      });
    }
  }
  return {
    ordinaryTax,
    marginal,
    pieces,
    gainsTax: pieces.reduce((sum, p) => sum + p.tax, 0),
  };
}
