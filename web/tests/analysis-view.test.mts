import assert from "node:assert/strict";
import { test } from "node:test";
import type { AnalysisParameter, SolveOutcome, SweepResults } from "../lib/api/types.ts";
import {
  frontierRows,
  paramId,
  paramValue,
  sensitivityView,
  sliceAlongX,
  sliceAlongY,
  solveHeadline,
  solveRows,
  successRamp,
  sweepView,
  SUCCESS_RAMP,
} from "../lib/view/analysis.ts";

/**
 * A 3 × 4 grid: retirement age across, monthly spend up. Success falls as
 * spending rises and rises as retirement is delayed, which is the shape every
 * derivation below is checked against.
 */
const SUCCESS: number[][] = [
  // [x][y], y ascending = more spending
  [1.0, 0.98, 0.9, 0.6],
  [1.0, 1.0, 0.96, 0.8],
  [1.0, 1.0, 0.99, 0.96],
];

function sweep(): SweepResults {
  const cells = [];
  for (let x = 0; x < 3; x++) {
    for (let y = 0; y < 4; y++) {
      cells.push({
        indices: [x, y],
        success_rate: SUCCESS[x][y],
        funding_success_rate: null,
        p5: 1_000 * (x + 1),
        p50: 10_000 * (x + 1) - 1_000 * y,
        p95: 100_000,
      });
    }
  }
  return {
    axes: [
      {
        parameter_id: "event:1:age",
        label: "Retire · age",
        role: "age",
        kind: "age",
        values: [60, 62, 64],
      },
      {
        parameter_id: "event:2:amount",
        label: "Spending · amount",
        role: "amount",
        kind: "amount",
        values: [5_000, 7_000, 9_000, 11_000],
      },
    ],
    cells,
    plan: { success_rate: 0.9, funding_success_rate: null, p5: 1_000, p50: 8_000, p95: 90_000 },
    plan_indices: [1, 2],
    iterations: 250,
  };
}

test("the frontier is the last safe step in the direction success falls", () => {
  const view = sweepView(sweep(), 0.95);
  assert.equal(view.fallsWithY, true);
  // Column 0 clears at 5,000 and 7,000 but not 9,000 → index 1.
  // Column 1 clears through 9,000 → index 2. Column 2 clears everything → 3.
  assert.deepEqual(view.frontier, [1, 2, 3]);
});

test("a column with no safe cell breaks the frontier rather than guessing", () => {
  const results = sweep();
  // Sink the first column below the bar everywhere.
  for (const cell of results.cells) {
    if (cell.indices[0] === 0) cell.success_rate = 0.5;
  }
  const view = sweepView(results, 0.95);
  assert.deepEqual(view.frontier, [null, 2, 3]);
  assert.equal(frontierRows(view)[0].yValue, undefined);
});

test("moving the threshold re-reads the same cells", () => {
  const results = sweep();
  assert.deepEqual(sweepView(results, 0.99).frontier, [0, 1, 2]);
  assert.deepEqual(sweepView(results, 0.6).frontier, [3, 3, 3]);
  // Nothing about the measurements changed — only what counts as safe.
  assert.deepEqual(
    sweepView(results, 0.6).cells.map((c) => c.point.success_rate),
    sweepView(results, 0.99).cells.map((c) => c.point.success_rate),
  );
});

test("an axis where success rises with the parameter frontiers the other way", () => {
  const results = sweep();
  // Reverse each column, so more of the vertical parameter is better.
  for (const cell of results.cells) {
    cell.success_rate = SUCCESS[cell.indices[0]][3 - cell.indices[1]];
  }
  const view = sweepView(results, 0.95);
  assert.equal(view.fallsWithY, false);
  // Column 0 now clears only at the top two steps, so the safe extreme is 2.
  assert.deepEqual(view.frontier, [2, 1, 0]);
});

test("the frontier table quotes the safe value and its distance from the plan", () => {
  const view = sweepView(sweep(), 0.95);
  const rows = frontierRows(view);
  assert.deepEqual(
    rows.map((r) => [r.xValue, r.yValue, r.delta]),
    [
      [60, 7_000, -2_000],
      [62, 9_000, 0],
      [64, 11_000, 2_000],
    ],
  );
  assert.equal(rows[1].point?.success_rate, 0.96);
});

test("slices read one row and one column of the same grid", () => {
  const view = sweepView(sweep(), 0.95);
  assert.deepEqual(
    sliceAlongX(view, 3).points.map((p) => p.success),
    [0.6, 0.8, 0.96],
  );
  assert.deepEqual(
    sliceAlongY(view, 0)!.points.map((p) => p.success),
    [1.0, 0.98, 0.9, 0.6],
  );
});

test("a one-axis sweep is a single row, and has no vertical slice", () => {
  const results = sweep();
  results.axes = [results.axes[0]];
  results.cells = results.cells
    .filter((c) => c.indices[1] === 0)
    .map((c) => ({ ...c, indices: [c.indices[0]] }));
  results.plan_indices = [1];

  const view = sweepView(results, 0.95);
  assert.equal(view.rows, 1);
  assert.equal(view.columns, 3);
  assert.equal(view.yAxis, undefined);
  assert.equal(sliceAlongY(view, 0), undefined);
  assert.deepEqual(view.planCell, { x: 1, y: 0 });
});

test("a plan off the swept range leaves the grid unmarked", () => {
  const results = sweep();
  results.plan_indices = null;
  assert.equal(sweepView(results, 0.95).planCell, undefined);
});

const FIRST = SUCCESS_RAMP[0];
const LAST = SUCCESS_RAMP[SUCCESS_RAMP.length - 1];

test("the ramp spreads over the range the grid actually covers", () => {
  // Real plans live between 80% and 100%; a 0–100% ramp would paint them all
  // the same near-white.
  const ramp = successRamp([0.8, 0.9, 1.0]);
  assert.equal(ramp.low, 0.8);
  assert.equal(ramp.high, 1.0);
  assert.equal(ramp.fill(0.8), FIRST);
  assert.equal(ramp.fill(1.0), LAST);
  assert.notEqual(ramp.fill(0.9), ramp.fill(1.0));
  // Values outside the observed range clamp rather than index past the array.
  assert.equal(ramp.fill(0), FIRST);
  assert.equal(ramp.fill(2), LAST);
});

test("a flat grid is not stretched into a story it does not tell", () => {
  const ramp = successRamp([0.99, 0.995, 1.0]);
  // The widened band is a tenth wide, give or take the float arithmetic that
  // computes it from an endpoint clamped at 1.
  assert.ok(ramp.high - ramp.low > 0.099, "band should widen, not amplify");
  assert.ok(ramp.high <= 1 && ramp.low >= 0);
  // A one-point spread now occupies one step of the ramp, not all eight.
  assert.notEqual(ramp.fill(0.99), FIRST);
});

test("an empty grid still yields a usable ramp", () => {
  const ramp = successRamp([]);
  assert.equal(ramp.fill(0), FIRST);
  assert.equal(ramp.fill(1), LAST);
});

test("parameters read as monospace identifiers, and values in their own units", () => {
  const parameter: AnalysisParameter = {
    id: "event:3:amount",
    event_id: 3,
    event_name: "Retirement spending",
    role: "starts at age",
    kind: "age",
    current: 62,
    min: 52,
    max: 72,
  };
  assert.equal(paramId(parameter), "retirement-spending.starts-at-age");
  assert.equal(paramValue("age", 62.4), "62");
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
  });
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
  });
  assert.ok(high - low > 0.049, "the axis should widen rather than amplify");
  assert.ok(high <= 1 && low >= 0);
  // Half a point of movement stays a tenth of the chart, not the whole of it.
  assert.ok(rows[0].barWidth < 0.2, `bar was ${rows[0].barWidth}`);
});

function solve(best: SolveOutcome["best"]): SolveOutcome {
  const parameter: AnalysisParameter = {
    id: "event:3:amount",
    event_id: 3,
    event_name: "Retirement spending",
    role: "amount",
    kind: "amount",
    current: 9_000,
    min: 4_500,
    max: 18_000,
  };
  return {
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
    label: "retirement-spending.amount",
    mono: true,
    plan: "$9,000",
    best: "$12,000",
    delta: "+$3,000",
  });
  assert.equal(rows[1].label, "Success");
  assert.equal(rows[1].best, "95.0%");
  // A worse terminal figure at the answer is still reported, not hidden.
  assert.equal(rows[2].delta.startsWith("−"), true);
});

test("no feasible answer reads as one, rather than as a zero", () => {
  const outcome = solve(null);
  assert.equal(solveHeadline(outcome), "no answer");
  assert.deepEqual(
    solveRows(outcome).map((r) => r.best),
    ["—", "—", "—", "—"],
  );
});
