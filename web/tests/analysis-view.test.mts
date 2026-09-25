import assert from "node:assert/strict";
import { test } from "node:test";
import type { AnalysisParameter, SolveOutcome } from "../lib/api/types.ts";
import {
  paramId,
  paramValue,
  sensitivityView,
  solveHeadline,
  solveRows,
} from "../lib/view/analysis.ts";

test("parameters read as shared input names, and values in their own units", () => {
  const parameter: AnalysisParameter = {
    id: "event:3:amount",
    parameter_id: 3,
    name: "Retirement spending",
    kind: "age",
    current: 62,
    min: 52,
    max: 72,
  };
  assert.equal(paramId(parameter), "Retirement spending");
  assert.equal(paramValue("age", 62.5), "62 yr 6 mo");
  assert.equal(paramValue("amount", 7_000), "$7,000");
});

test("the sensitivity bars are drawn against the range the parameters cover", () => {
  const { rows, low, high } = sensitivityView({
    fraction: 0.2,
    iterations: 200,
    plan: { success_rate: 0.9, funding_success_rate: null, p5: 0, p50: 0, p95: 0 },
    rows: [
      {
        parameter_id: "a",
        label: "A",
        kind: "amount",
        low_value: 1,
        high_value: 2,
        low: { success_rate: 0.6, funding_success_rate: null, p5: 0, p50: 0, p95: 0 },
        high: { success_rate: 0.95, funding_success_rate: null, p5: 0, p50: 0, p95: 0 },
        span: 35,
      },
      {
        parameter_id: "b",
        label: "B",
        kind: "age",
        low_value: 60,
        high_value: 70,
        low: { success_rate: 0.99, funding_success_rate: null, p5: 0, p50: 0, p95: 0 },
        high: { success_rate: 0.89, funding_success_rate: null, p5: 0, p50: 0, p95: 0 },
        span: 10,
      },
    ],
  }, "success");
  const [rising, falling] = rows;
  // The axis covers 0.6 to 0.99 — the range the rows and the plan occupy —
  // rather than 0 to 1, where every band would be a sliver at the right edge.
  assert.equal(low, 0.6);
  assert.equal(high, 0.99);
  assert.equal(rising.barStart, 0);
  assert.ok(Math.abs(rising.barWidth - 0.35 / 0.39) < 1e-9);
  // A band whose high end is the worse one still draws left to right.
  assert.ok(falling.barStart > 0.7);
  assert.ok(Math.abs(falling.barWidth - 0.1 / 0.39) < 1e-9);
});

test("a ranking where nothing moves still draws readable bars", () => {
  const point = (rate: number) => ({
    success_rate: rate,
    funding_success_rate: null,
    p5: 0,
    p50: 0,
    p95: 0,
  });
  const { rows, low, high } = sensitivityView({
    fraction: 0.2,
    iterations: 200,
    plan: point(1),
    rows: [
      {
        parameter_id: "a",
        label: "A",
        kind: "amount",
        low_value: 1,
        high_value: 2,
        low: point(0.995),
        high: point(1),
        span: 0.5,
      },
    ],
  }, "success");
  assert.ok(high - low > 0.049, "the axis should widen rather than amplify");
  assert.ok(high <= 1 && low >= 0);
  // Half a point of movement stays a tenth of the chart, not the whole of it.
  assert.ok(rows[0].barWidth < 0.2, `bar was ${rows[0].barWidth}`);
});

function solve(best: SolveOutcome["best"]): SolveOutcome {
  const parameter: AnalysisParameter = {
    id: "event:3:amount",
    parameter_id: 3,
    name: "Retirement spending",
    kind: "amount",
    current: 9_000,
    min: 4_500,
    max: 18_000,
  };
  return {
    constraint: "success-rate",
    method: "bisection",
    parameters: [parameter],
    plan: { success_rate: 0.87, funding_success_rate: null, p5: 100, p50: 1_000, p95: 5_000 },
    steps: [],
    best,
    std_error: 0.014,
    iterations: 250,
  };
}

test("the solve comparison quotes the parameter, then what moved with it", () => {
  const rows = solveRows(
    solve({
      values: [12_000],
      feasible: true,
      bracket_low: null,
      bracket_high: null,
      success_rate: 0.95,
      funding_success_rate: null,
      p5: 50,
      p50: 900,
      p95: 4_000,
    }),
  );
  assert.deepEqual(rows[0], {
    label: "Retirement spending",
    mono: true,
    plan: "$9,000",
    best: "$12,000",
    delta: "+$3,000",
  });
  assert.equal(rows[1].best, "Not measured — rerun");
  assert.equal(rows[2].label, "Positive ending net worth");
  assert.equal(rows[2].best, "95.0%");
  // A worse terminal figure at the answer is still reported, not hidden.
  assert.equal(rows[3].delta.startsWith("−"), true);
});

test("no feasible answer reads as one, rather than as a zero", () => {
  const outcome = solve(null);
  assert.equal(solveHeadline(outcome), "no answer");
  assert.deepEqual(
    solveRows(outcome).map((r) => r.best),
    ["—", "—", "—", "—", "—"],
  );
});

test("funding ranking uses funding differences and omits unmeasured rows", () => {
  const point = (success_rate: number, funding_success_rate: number | null) => ({ success_rate, funding_success_rate, p5: 0, p50: 0, p95: 0 });
  const row = (id: string, low: ReturnType<typeof point>, high: ReturnType<typeof point>) => ({ parameter_id: id, label: id, kind: "amount", low_value: 1, high_value: 2, low, high, span: 99 });
  const results = { fraction: 0.2, iterations: 100, plan: point(1, 0), rows: [
    row("wealth", point(0, 0), point(1, 0)),
    row("funding", point(1, 0), point(1, 0.8)),
    row("legacy", point(0, null), point(1, null)),
  ] };
  assert.deepEqual(sensitivityView(results).rows.map(r => r.parameterId), ["funding", "wealth"]);
  assert.equal(sensitivityView(results).rows[0].span, 80);
  assert.equal(sensitivityView(results, "success").rows[0].parameterId, "wealth");
});

test("rate and calendar coordinates display without losing their units", () => {
  assert.equal(paramValue("rate", 0.0456789), "4.56789%");
  assert.equal(paramValue("rate", 1.2), "120%");
  assert.equal(paramValue("date", Date.UTC(2035, 0, 2) / 86_400_000), "2035-01-02");
  assert.equal(paramValue("age", 40 + 1/12), "40 yr 1 mo");
});
