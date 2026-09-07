import assert from "node:assert/strict";
import { test } from "node:test";
import type { Results, Scenario } from "../lib/api/types.ts";
import { toResultsData } from "../lib/view/results.ts";

const dates = ["2026-06-01", "2026-12-31", "2027-06-01"];
const scenario = { start_date: "2040-01-01" } as Scenario; // Edited live dates must not relabel stored dollars.
const axis = { unit: "year" as const, range: [2026, 2027] as [number, number], at: (date: string) => Number(date.slice(0, 4)), label: String };
function fixture(series = "0.5"): Results {
  const bands = [
    { path_id: "0.1", percentile: 0.1, dates, net_worth: [100, 100, 100], inflation: [1, 1, 1] },
    { path_id: "0.5", percentile: 0.5, dates, net_worth: [100, 300, 200], inflation: [1, 1, 4] },
    { path_id: "0.95", percentile: 0.95, dates, net_worth: [100, -50, 300], inflation: [1, 1, 2] },
    { path_id: "mean", percentile: null, dates, net_worth: [100, 116.67, 200], inflation: [1, 1, 2.33] },
  ];
  const selected = bands.find(b => b.path_id === series)!;
  return {
    run_id: 7, scenario_id: 2, series_id: series, series_percentile: selected.percentile,
    stats: { num_iterations: 3, success_rate: 1, funding_success_rate: 2/3,
      mean_final_net_worth: 200, std_dev_final_net_worth: 81.65, min_final_net_worth: 100, max_final_net_worth: 300,
      lifetime_taxes: 999, converged: null, convergence_metric: null, convergence_value: null,
      percentile_values: [{percentile: 0.5, final_net_worth: 200}],
    },
    bands,
    real_net_worth: {
      terminal: {base_date: dates[0], num_iterations: 3, mean: 100, std_dev: 40.82, min: 50, max: 150},
      points: [
        {date: dates[0], p5: 100, p50: 100, p95: 100},
        {date: dates[1], p5: -35, p50: 100, p95: 280},
        {date: dates[2], p5: 55, p50: 100, p95: 145},
      ],
    },
    account_series: [{account_id: 1, label: "Bank", values: selected.net_worth}],
    cash_flows: [{year: 2027, income: 80, expenses: 20, contributions: 0, withdrawals: 0, appreciation: 0, net_cash_flow: 60, taxes: 8}],
    warnings: [], inflation: [{year: 2026, factor: 1}, {year: 2027, factor: selected.inflation[2]}],
    ledger_years: [{year: 2027, cash: 1, asset: 0, tax: 0, event: 0, total: 1, tag: null}],
  };
}

test("pointwise envelope and real aggregates are never inferred from nominal-ranked paths", () => {
  const data = toResultsData(fixture(), scenario, axis);
  assert.deepEqual(data.bands.p5, [-35, 55]);
  assert.deepEqual(data.bands.p50, [100, 100]);
  assert.deepEqual(data.bands.p95, [280, 145]);
  assert.deepEqual(data.pathValues, [300, 50]);
  assert.equal(data.stats.meanFinalNetWorth, 100); // NOT nominal mean 200 / selected inflation 4.
  assert.deepEqual(data.stats.percentileValues, [[0.05, 55], [0.5, 100], [0.95, 145]]);
  assert.equal(data.baseDate, "2026-06-01");
  assert.equal(data.dollarLabel, "2026-06-01 dollars (annual inflation)");
  assert.equal(data.pathLabel, "P50 nominal-terminal-ranked path");
});

test("a loaded payload changes path identity, wealth, accounts, flows and ledger factors together, never the envelope", () => {
  const before = toResultsData(fixture(), scenario, axis);
  // While a request for P5 is pending, mapping the retained P50 payload keeps
  // P50 labels and details. There is deliberately no requested-percentile input.
  const pending = toResultsData(fixture(), scenario, axis);
  assert.deepEqual(pending, before);
  const after = toResultsData(fixture("0.1"), scenario, axis); // P5 resolves to stored P10.
  assert.deepEqual(after.bands, before.bands);
  assert.deepEqual(after.stats.percentileValues, before.stats.percentileValues);
  assert.equal(after.pathId, "0.1");
  assert.equal(after.pathLabel, "P10 nominal-terminal-ranked path");
  for (const data of [before, after]) {
    assert.equal(data.accountSeries[0].values[1], data.pathValues[1]);
    assert.equal(data.cashFlows[0].netWorth, data.pathValues[1]);
    assert.equal(data.cashFlows[0].income, 80 / data.cashFlows[0].inflationFactor);
    assert.equal(data.stats.lifetimeTaxes, 8 / data.cashFlows[0].inflationFactor);
    assert.equal(data.runId, 7);
  }
  assert.notEqual(before.cashFlows[0].inflationFactor, after.cashFlows[0].inflationFactor);
});

test("historical runs show unavailable real statistics and no fabricated envelope", () => {
  const old = fixture();
  old.real_net_worth = null;
  const data = toResultsData(old, scenario, axis);
  assert.equal(data.hasEnvelope, false);
  assert.deepEqual(data.bands.p5, []);
  assert.deepEqual(data.bands.p50, []);
  assert.deepEqual(data.bands.p95, []);
  assert.deepEqual(data.pathValues, [300, 50]);
  assert.ok(Number.isNaN(data.stats.meanFinalNetWorth));
  assert.deepEqual(data.stats.percentileValues, []);
  old.inflation = [];
  const nominal = toResultsData(old, scenario, axis);
  assert.equal(nominal.dollarLabel, "nominal dollars");
  assert.deepEqual(nominal.pathValues, [300, 200]);
});

test("a synthetic mean is never deflated by its averaged inflation", () => {
  const data = toResultsData(fixture("mean"), scenario, axis);
  assert.equal(data.pathLabel, "Synthetic nominal mean (not a path)");
  assert.equal(data.dollarLabel, "nominal dollars");
  assert.deepEqual(data.pathValues, [116.67, 200]);
  assert.equal(data.hasEnvelope, false); // Cannot overlay real and nominal units.
  assert.equal(data.cashFlows[0].income, 80);
});

test("empty detail does not borrow another path or fabricate wealth", () => {
  const raw = fixture();
  raw.bands = [];
  raw.account_series = [];
  raw.cash_flows = [];
  const data = toResultsData(raw, scenario, axis);
  assert.deepEqual(data.pathValues, []);
  assert.deepEqual(data.bands.p50, []);
  assert.equal(data.stats.meanFinalNetWorth, 100);
  assert.ok(Number.isNaN(data.stats.lifetimeTaxes));
});
