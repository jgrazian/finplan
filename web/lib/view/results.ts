/**
 * `api::runs::Results` → the Results screen's `ResultsData`.
 *
 * Two shape changes happen here. The API returns one band per stored
 * percentile; the chart wants three named series, so the nearest stored path to
 * 5 / 50 / 95 is chosen. And the engine snapshots wealth at the plan start and
 * again at every year end, which puts two points in the opening year — the
 * series is collapsed to one point per year so the chart's year ticks and the
 * yearly cash-flow table line up.
 */
import type { Band, Results, Scenario } from "@/lib/api/types";
import type {
  AccountSeries,
  MonteCarloStats,
  NetWorthBands,
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

const WARNING_TITLES: Record<string, string> = {
  EffectSkipped: "Effect skipped",
  EvaluationFailed: "Evaluation failed",
  IterationLimitHit: "Iteration limit hit",
};

export function toResultsData(
  results: Results,
  scenario: Scenario,
  axis: PlanAxis,
): ResultsData {
  const paths = results.bands.filter((b) => b.percentile != null);
  const reference = nearest(paths, 0.5) ?? results.bands[0];

  if (!reference || reference.dates.length === 0) {
    return emptyResults(results);
  }

  // One index per calendar year, keeping that year's last snapshot.
  const keep = lastIndexPerYear(reference.dates);
  const dates = keep.map((i) => reference.dates[i]);
  const at = (band: Band | undefined) =>
    band ? keep.map((i) => band.net_worth[i] ?? 0) : keep.map(() => 0);

  const p50 = at(reference);
  const bands: NetWorthBands = {
    years: dates.map(yearOf),
    ages: dates.map(axis.at),
    p5: at(nearest(paths, 0.05)),
    p50,
    p95: at(nearest(paths, 0.95)),
  };

  return {
    stats: toStats(results),
    bands,
    accountSeries: toAccountSeries(results, keep),
    cashFlows: toCashFlows(results),
    warnings: toWarnings(results),
    horizonLabel: axis.label(bands.ages[bands.ages.length - 1]),
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

function lastIndexPerYear(dates: string[]): number[] {
  const lastByYear = new Map<number, number>();
  dates.forEach((date, index) => lastByYear.set(yearOf(date), index));
  return [...lastByYear.values()].sort((a, b) => a - b);
}

function toStats(results: Results): MonteCarloStats {
  const stats = results.stats;
  return {
    numIterations: stats.num_iterations,
    successRate: stats.success_rate,
    meanFinalNetWorth: stats.mean_final_net_worth,
    stdDevFinalNetWorth: stats.std_dev_final_net_worth,
    minFinalNetWorth: stats.min_final_net_worth,
    maxFinalNetWorth: stats.max_final_net_worth,
    percentileValues: stats.percentile_values.map((p) => [p.percentile, p.final_net_worth]),
    converged: stats.converged ?? undefined,
    convergenceMetric: stats.convergence_metric ?? undefined,
    convergenceValue: stats.convergence_value ?? undefined,
    lifetimeTaxes: stats.lifetime_taxes,
  };
}

function toAccountSeries(results: Results, keep: number[]): AccountSeries[] {
  return results.account_series.map((series, index) => ({
    accountId: String(series.account_id),
    label: series.label,
    color: SERIES_COLORS[index % SERIES_COLORS.length],
    values: keep.map((i) => series.values[i] ?? 0),
  }));
}

function toCashFlows(results: Results): YearlyCashFlow[] {
  return results.cash_flows.map((flow) => ({
    year: flow.year,
    income: flow.income,
    expenses: flow.expenses,
    taxes: flow.taxes,
  }));
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
function emptyResults(results: Results): ResultsData {
  return {
    stats: toStats(results),
    bands: { years: [], ages: [], p5: [], p50: [], p95: [] },
    accountSeries: [],
    cashFlows: toCashFlows(results),
    warnings: toWarnings(results),
    horizonLabel: "—",
  };
}
