/**
 * `api::runs::Results` → the Results screen's `ResultsData`.
 *
 * Three shape changes happen here. The API returns one band per stored
 * percentile; the chart wants three named series, so the nearest stored path to
 * 5 / 50 / 95 is chosen. The engine snapshots wealth at the plan start and
 * again at every year end, which puts two points in the opening year — the
 * series is collapsed to one point per year so the chart's year ticks and the
 * yearly cash-flow table line up.
 *
 * The per-account series, the cash flows and the ledger are not a fan at all:
 * they describe the one path the request named, which `series` says here so
 * that everything drawn from them is attributed to it.
 *
 * And every dollar is deflated. The engine works in nominal dollars, so a
 * balance at the end of a 35-year plan is quoted in dollars worth roughly half
 * what today's are — which makes a rising net-worth line unreadable as a
 * statement about whether the plan works. This module is the single place that
 * divides by the path's own realised inflation, so everything downstream of it
 * is in the plan's first-year dollars and can be compared with everything else.
 */
import type { Band, Results, Scenario } from "@/lib/api/types";
import type {
  AccountSeries,
  LedgerSummary,
  MonteCarloStats,
  NetWorthBands,
  Percentile,
  ResultsData,
  SimulationWarning,
  YearlyCashFlow,
} from "@/lib/types";
import type { PlanAxis } from "./axis";
import { yearOf } from "./format";

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

/** The percentile each named path stands for, for picking it out of the fan. */
const TARGET: Record<Percentile, number> = { p5: 0.05, p50: 0.5, p95: 0.95 };

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
  /** The path the request asked for, and so the one the payload describes. */
  series: Percentile,
): ResultsData {
  const paths = results.bands.filter((b) => b.percentile != null);
  const reference = nearest(paths, 0.5) ?? results.bands[0];
  const factorFor = inflationIndex(results);

  if (!reference || reference.dates.length === 0) {
    return emptyResults(results, factorFor);
  }

  // One index per calendar year, keeping that year's last snapshot.
  const keep = lastIndexPerYear(reference.dates);
  const dates = keep.map((i) => reference.dates[i]);

  // Each path is deflated by its own realised inflation rather than by a shared
  // index: the fan is then the spread of real outcomes, which is the question
  // being asked of it, and not the spread of nominal ones.
  const at = (band: Band | undefined) =>
    band
      ? keep.map((i) => deflate(band.net_worth[i] ?? 0, band.inflation[i]))
      : keep.map(() => 0);

  const bands: NetWorthBands = {
    years: dates.map(yearOf),
    ages: dates.map(axis.at),
    p5: at(nearest(paths, 0.05)),
    p50: at(reference),
    p95: at(nearest(paths, 0.95)),
  };

  // The path everything outside the fan describes — the same band the API
  // filled the per-account series and cash flows from.
  const seriesBand = nearest(paths, TARGET[series]) ?? reference;
  const cashFlows = toCashFlows(results, axis, factorFor, bands[series], bands.years);
  const baseYear = bands.years[0] ?? yearOf(scenario.start_date);

  return {
    stats: toStats(results, cashFlows, finalFactor(reference)),
    bands,
    accountSeries: toAccountSeries(results, keep, seriesBand),
    cashFlows,
    warnings: toWarnings(results),
    horizonLabel: axis.label(bands.ages[bands.ages.length - 1]),
    baseYear,
    totalInflation: finalFactor(reference),
  };
}

/** The stored path closest to `target`, or undefined if none were stored. */
function nearest(paths: Band[], target: number): Band | undefined {
  let best: Band | undefined;
  let bestDistance = Infinity;
  for (const band of paths) {
    const distance = Math.abs((band.percentile ?? 0) - target);
    if (distance < bestDistance) {
      best = band;
      bestDistance = distance;
    }
  }
  return best;
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

/**
 * Aggregates over final net worth are deflated by the median path's total
 * inflation: they describe wealth at one moment — the end of the plan — and
 * belong to no single path, so no other factor is theirs to use.
 *
 * Lifetime taxes are the exception. A sum over 35 years cannot be restated by
 * dividing the total, so it is re-summed from the per-year figures the cash
 * flows already carry, each deflated at the year it was paid.
 */
function toStats(
  results: Results,
  cashFlows: YearlyCashFlow[],
  final: number,
): MonteCarloStats {
  const stats = results.stats;
  const lifetimeTaxes = cashFlows.length
    ? cashFlows.reduce((sum, flow) => sum + flow.taxes, 0)
    : deflate(stats.lifetime_taxes, final);

  return {
    numIterations: stats.num_iterations,
    successRate: stats.success_rate,
    fundingSuccessRate: stats.funding_success_rate ?? undefined,
    meanFinalNetWorth: deflate(stats.mean_final_net_worth, final),
    stdDevFinalNetWorth: deflate(stats.std_dev_final_net_worth, final),
    minFinalNetWorth: deflate(stats.min_final_net_worth, final),
    maxFinalNetWorth: deflate(stats.max_final_net_worth, final),
    percentileValues: stats.percentile_values.map((p) => [
      p.percentile,
      deflate(p.final_net_worth, final),
    ]),
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
): AccountSeries[] {
  return results.account_series.map((series, index) => ({
    accountId: String(series.account_id),
    label: series.label,
    color: SERIES_COLORS[index % SERIES_COLORS.length],
    values: keep.map((i) => deflate(series.values[i] ?? 0, path.inflation[i])),
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

/** A run that stored no wealth snapshots still has stats worth showing. */
function emptyResults(
  results: Results,
  factorFor: (year: number) => number,
): ResultsData {
  const bands: NetWorthBands = { years: [], ages: [], p5: [], p50: [], p95: [] };
  const final = results.inflation[results.inflation.length - 1]?.factor ?? 1;
  const cashFlows = toCashFlows(results, YEAR_AXIS, factorFor, bands.p50, bands.years);
  return {
    stats: toStats(results, cashFlows, final),
    bands,
    accountSeries: [],
    cashFlows,
    warnings: toWarnings(results),
    horizonLabel: "—",
    baseYear: results.inflation[0]?.year ?? new Date().getFullYear(),
    totalInflation: final,
  };
}

/** Stand-in for a run with no snapshots to derive a real axis from. */
const YEAR_AXIS: PlanAxis = {
  unit: "year",
  range: [0, 0],
  at: (isoDate) => yearOf(isoDate),
  label: (position) => String(position),
};
