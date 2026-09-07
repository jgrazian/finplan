/** Independent pointwise real quantiles plus ONE nominal-terminal-ranked path.
 * Only path details are deflated here; the engine aggregates real observations
 * before computing the envelope/terminal stats. Never divide nominal aggregates
 * by a representative (or averaged) inflation path.
 */
import type { Band, Results, Scenario } from "@/lib/api/types";
import type {
  AccountSeries,
  LedgerSummary,
  MonteCarloStats,
  NetWorthBands,
  ResultsData,
  SimulationWarning,
  YearlyCashFlow,
} from "@/lib/types";
import type { PlanAxis } from "./axis";
import { yearOf } from "./format.ts";

/** Stack colours, darkest first, matching the Results artboard. */
const SERIES_COLORS = [
  "#1d2d3d",
  "#41617f",
  "#749dc4",
  "#b5d9fd",
  "#5980a6",
  "#8fb4d6",
  "#2f4a63",
  "#cfe4f7",
];

const WARNING_TITLES: Record<string, string> = {
  EffectSkipped: "Effect skipped",
  EvaluationFailed: "Evaluation failed",
  IterationLimitHit: "Iteration limit hit",
  CashShortfall: "Unfunded cash account",
};

/** Nominal → real. A missing or nonsensical factor leaves the figure alone. */
export function deflate(nominal: number, factor: number | undefined): number {
  return factor && factor > 0 ? nominal / factor : nominal;
}

/**
 * Cumulative inflation by calendar year, for the path the cash flows describe.
 * Years before the table starts are the base year; years past its end hold at
 * the last factor recorded.
 */
export function inflationIndex(results: Results): (year: number) => number {
  const byYear = new Map(results.inflation.map((p) => [p.year, p.factor]));
  const years = results.inflation.map((p) => p.year);
  const first = years[0];
  const last = years[years.length - 1];

  return (year) => {
    if (first == null) return 1;
    if (year <= first) return 1;
    return byYear.get(year) ?? byYear.get(last) ?? 1;
  };
}

export function toResultsData(
  results: Results,
  scenario: Scenario,
  axis: PlanAxis,
): ResultsData {
  // Trust the response's actual resolved ID, never the current UI request.
  const path = results.bands.find((b) => b.path_id === results.series_id);
  const isReal = path?.percentile != null && results.inflation.length > 0;
  const factorFor = isReal ? inflationIndex(results) : () => 1;
  const keep = lastIndexPerYear(path?.dates ?? []);
  const dates = keep.map((i) => path!.dates[i]);
  const pathValues = keep.map((i) => deflate(path!.net_worth[i], isReal ? path!.inflation[i] : 1));
  const real = results.real_net_worth;
  const byDate = new Map(real?.points.map((point) => [point.date, point]));
  const hasEnvelope = isReal && dates.length > 0 && dates.every((date) => byDate.has(date));
  const at = (key: "p5" | "p50" | "p95") =>
    hasEnvelope ? dates.map((date) => byDate.get(date)![key]) : [];
  const bands: NetWorthBands = {
    years: dates.map(yearOf),
    ages: dates.map(axis.at),
    p5: at("p5"),
    p50: at("p50"),
    p95: at("p95"),
  };
  const cashFlows = toCashFlows(results, axis, factorFor, pathValues, bands.years);
  const baseDate = real?.terminal.base_date ?? path?.dates[0] ?? scenario.start_date;
  const pathLabel = path == null ? "Path unavailable" : path.percentile == null
    ? "Synthetic nominal mean (not a path)"
    : `P${Number((path.percentile * 100).toFixed(2))} nominal-terminal-ranked path`;
  return {
    runId: results.run_id,
    pathId: results.series_id,
    pathLabel,
    pathValues,
    hasEnvelope,
    stats: toStats(results, cashFlows),
    bands,
    accountSeries: path ? toAccountSeries(results, keep, path, isReal) : [],
    cashFlows,
    warnings: toWarnings(results),
    horizonLabel: dates.length ? axis.label(bands.ages[bands.ages.length - 1]) : "—",
    baseYear: yearOf(baseDate),
    baseDate,
    dollarLabel: isReal ? `${baseDate} dollars (annual inflation)` : "nominal dollars",
    totalInflation: isReal ? finalFactor(path) : Number.NaN,
  };
}

/** Cumulative inflation over the whole horizon of one path. */
function finalFactor(band: Band | undefined): number {
  const last = band?.inflation[band.inflation.length - 1];
  return last && last > 0 ? last : 1;
}

function lastIndexPerYear(dates: string[]): number[] {
  const lastByYear = new Map<number, number>();
  dates.forEach((date, index) => lastByYear.set(yearOf(date), index));
  return [...lastByYear.values()].sort((a, b) => a - b);
}

/** Already-real aggregates; legacy data is unavailable, not an approximation.
 * Lifetime taxes belong only to the selected path, summed at each year's factor.
 */
function toStats(
  results: Results,
  cashFlows: YearlyCashFlow[],
): MonteCarloStats {
  const stats = results.stats;
  const real = results.real_net_worth;
  const final = real?.points.at(-1);
  const lifetimeTaxes = cashFlows.length
    ? cashFlows.reduce((sum, flow) => sum + flow.taxes, 0)
    : Number.NaN;

  return {
    numIterations: stats.num_iterations,
    successRate: stats.success_rate,
    fundingSuccessRate: stats.funding_success_rate ?? undefined,
    meanFinalNetWorth: real?.terminal.mean ?? Number.NaN,
    stdDevFinalNetWorth: real?.terminal.std_dev ?? Number.NaN,
    minFinalNetWorth: real?.terminal.min ?? Number.NaN,
    maxFinalNetWorth: real?.terminal.max ?? Number.NaN,
    percentileValues: final ? [[0.05, final.p5], [0.5, final.p50], [0.95, final.p95]] : [],
    converged: stats.converged ?? undefined,
    convergenceMetric: stats.convergence_metric ?? undefined,
    convergenceValue: stats.convergence_value ?? undefined,
    lifetimeTaxes,
  };
}

/**
 * Account values share the snapshots of the path they were read from, so they
 * share its inflation too — which keeps the stacked bands summing to that
 * path's net-worth line rather than to a median the request never asked for.
 */
function toAccountSeries(
  results: Results,
  keep: number[],
  path: Band,
  isReal: boolean,
): AccountSeries[] {
  return results.account_series.map((series, index) => ({
    accountId: String(series.account_id),
    label: series.label,
    color: SERIES_COLORS[index % SERIES_COLORS.length],
    values: keep.map((i) => deflate(series.values[i] ?? 0, isReal ? path.inflation[i] : 1)),
  }));
}

/** One account's standing in a single year, as the rail's breakdown reads it. */
export interface AccountStanding {
  accountId: AccountSeries["accountId"];
  label: string;
  color: string;
  value: number;
  /**
   * The row's bar: |value| as a share of the year's largest position. Measured
   * against the largest rather than against the total because a plan with a
   * mortgage on it has positions on both sides of zero, and shares of a net
   * total are meaningless once the parts do not all point the same way.
   */
  share: number;
  /** Change from the prior year; undefined in the plan's first year. */
  delta: number | undefined;
}

export interface AccountBreakdown {
  standings: AccountStanding[];
  /** What the standings add up to — the year's net worth, on this path. */
  total: number;
}

/**
 * One year of `accountSeries`, cut across the accounts instead of along time.
 *
 * The chart draws each account as a band through the whole horizon; this is the
 * same numbers read at a single year, which is the question a reader has while
 * pointing at one — what is the net worth on screen actually made of, and what
 * moved since last year.
 */
export function accountBreakdown(
  series: AccountSeries[],
  index: number,
): AccountBreakdown {
  const values = series.map((s) => s.values[index] ?? 0);
  const largest = Math.max(...values.map(Math.abs), 0) || 1;

  return {
    standings: series.map((s, i) => ({
      accountId: s.accountId,
      label: s.label,
      color: s.color,
      value: values[i],
      share: Math.abs(values[i]) / largest,
      delta: index > 0 ? values[i] - (s.values[index - 1] ?? 0) : undefined,
    })),
    total: values.reduce((sum, v) => sum + v, 0),
  };
}

const NO_LEDGER: LedgerSummary = { total: 0, cash: 0, asset: 0, tax: 0, event: 0 };

function toCashFlows(
  results: Results,
  axis: PlanAxis,
  factorFor: (year: number) => number,
  /** Wealth along the same path the flows describe. */
  path: number[],
  years: number[],
): YearlyCashFlow[] {
  const ledgers = new Map(results.ledger_years.map((y) => [y.year, y]));
  const netWorth = new Map(years.map((year, i) => [year, path[i]]));

  return results.cash_flows.map((flow) => {
    const factor = factorFor(flow.year);
    const ledger = ledgers.get(flow.year);
    return {
      year: flow.year,
      age: axis.at(`${flow.year}-12-31`),
      income: deflate(flow.income, factor),
      expenses: deflate(flow.expenses, factor),
      contributions: deflate(flow.contributions, factor),
      withdrawals: deflate(flow.withdrawals, factor),
      appreciation: deflate(flow.appreciation, factor),
      netCashFlow: deflate(flow.net_cash_flow, factor),
      taxes: deflate(flow.taxes, factor),
      netWorth: netWorth.get(flow.year) ?? 0,
      inflationFactor: factor,
      ledger: ledger
        ? {
            total: ledger.total,
            cash: ledger.cash,
            asset: ledger.asset,
            tax: ledger.tax,
            event: ledger.event,
            tag: ledger.tag ?? undefined,
          }
        : NO_LEDGER,
    };
  });
}

function toWarnings(results: Results): SimulationWarning[] {
  return results.warnings.map((warning, index) => ({
    id: `${warning.kind}-${index}`,
    kind: warning.kind,
    title: WARNING_TITLES[warning.kind] ?? warning.kind,
    detail: warning.date ? `${warning.date} — ${warning.message}` : warning.message,
  }));
}
