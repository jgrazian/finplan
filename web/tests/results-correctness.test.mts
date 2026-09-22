import assert from "node:assert/strict";
import { test } from "node:test";
import { fmtCompact, fmtCompactOrExact, fmtCurrency, fmtAxis } from "../lib/format.ts";
import { linearDomain, logDomain, makeScale, valueTicks, linePath, barColumn } from "../components/charts/geometry.ts";
import { stackSeries, resolveScaleKind } from "../components/charts/stack.ts";

for (const format of [fmtCompact, fmtCompactOrExact]) {
  test(`${format.name} preserves negative and small outcomes`, () => {
    assert.equal(format(-51_859.73), "−$52k");
    assert.equal(format(-2_500_000), "−$2.50M");
    assert.equal(format(750), "$750");
    assert.equal(format(-750), "−$750");
    assert.equal(format(0), "$0");
    assert.equal(format(-0), "$0");
    assert.equal(format(0.25), "$0.25");
    assert.equal(format(-0.25), "−$0.25");
    assert.equal(format(Number.NaN), "—");
    assert.equal(format(Infinity), "—");
  });
}

test("currency and axes never round sub-dollar balances to zero", () => {
  for (const format of [fmtCompact, fmtCurrency, fmtAxis]) {
    assert.equal(format(-0.25), "−$0.25");
    assert.equal(format(0.01), "$0.01");
    assert.equal(format(0.001), "<$0.01");
    assert.equal(format(-0.001), "−<$0.01");
  }
});

test("signed linear scales place debt below zero with visible negative ticks", () => {
  const domain = linearDomain([-52_000, 0, 100_000]);
  assert.ok(domain.min < -52_000);
  assert.ok(domain.max > 100_000);
  const scale = makeScale(3, domain);
  assert.ok(scale.y(-52_000) > scale.y(0));
  assert.ok(scale.y(0) > scale.y(100_000));
  assert.ok(scale.y(-52_000) < scale.baseline);
  assert.ok(valueTicks(scale).some(t => t.value < 0));
  assert.equal(fmtAxis(-50_000), "−$50k");
  assert.doesNotMatch(linePath([-52_000, 0, 100_000], scale), /NaN|Infinity/);
});

test("all-debt, zero and empty charts retain a valid domain", () => {
  assert.deepEqual(linearDomain([-100, -20]), {min: -105, max: 0});
  for (const values of [[], [0, 0]]) {
    const domain = linearDomain(values);
    assert.ok(domain.max > domain.min);
    assert.ok(Number.isFinite(makeScale(values.length, domain).y(0)));
  }
});

const account = (id: string, values: number[]) => ({ accountId: id, label: id, color: "#123456", values });

test("debt never subtracts from the visible positive stack or vanishes from bars", () => {
  const stack = stackSeries([account("cash", [100, 100]), account("loan", [-40, -150])], 2);
  assert.deepEqual(stack.positive, [100, 100]);
  assert.deepEqual(stack.negative, [-40, -150]);
  assert.deepEqual(stack.total, [60, -50]);
  const scale = makeScale(2, linearDomain(stack.positive, stack.negative), {mode: "band"});
  for (const band of stack.bands) {
    for (let i = 0; i < 2; i++) {
      assert.ok(band.top[i] >= band.bottom[i]);
      assert.ok(scale.y(band.bottom[i]) - scale.y(band.top[i]) > 0);
      assert.ok(barColumn(i, scale).w > 0);
    }
  }
});

test("sign-changing accounts, multiple debts and missing points reconcile", () => {
  const series = [account("cash", [50, -20, 10]), account("loan", [-30, -40, -50]), account("other", [100])];
  const stack = stackSeries(series, 3);
  assert.deepEqual(stack.positive, [150, 0, 10]);
  assert.deepEqual(stack.negative, [-30, -60, -50]);
  for (let i = 0; i < 3; i++) {
    assert.equal(stack.total[i], series.reduce((sum, s) => sum + (s.values[i] ?? 0), 0));
    assert.ok(stack.bands.every(b => b.top[i] >= b.bottom[i]));
  }
});

test("log is symmetric about zero and never distorts account composition", () => {
  // Zero, debt and unavailable values all plot on a symmetric log axis.
  for (const values of [[0, 100], [-10, 100], [NaN, 100]]) {
    const resolved = resolveScaleKind("log", "fan");
    assert.equal(resolved.kind, "log");
    assert.equal(resolved.reason, undefined);
    assert.doesNotMatch(linePath(values, makeScale(2, logDomain(values), { kind: "log" })), /NaN|Infinity/);
  }
  for (const view of ["stack", "bar"] as const) {
    assert.equal(resolveScaleKind("log", view).kind, "linear");
    assert.ok(resolveScaleKind("log", view).reason);
  }
  // A tiny positive outcome is still inside the domain, not clipped to a prettier floor.
  const positive = logDomain([0.01, 100_000_000]);
  assert.ok(positive.min <= 0.01);
  assert.ok(positive.max >= 100_000_000);

  // Debt runs below zero on the log axis, in order, with labelled gridlines.
  const domain = logDomain([-52_000, 0, 2_000_000]);
  assert.ok(domain.min <= -52_000);
  assert.ok(domain.max >= 2_000_000);
  const scale = makeScale(3, domain, { kind: "log" });
  assert.ok(scale.y(-52_000) > scale.y(-1_000));
  assert.ok(scale.y(-1_000) > scale.y(0));
  assert.ok(scale.y(0) > scale.y(1_000));
  assert.ok(scale.y(1_000) > scale.y(2_000_000));
  assert.ok(scale.y(-52_000) <= scale.baseline);
  const ticks = valueTicks(scale);
  assert.ok(ticks.length <= 7);
  assert.ok(ticks.some((t) => t.value < 0));
  assert.ok(ticks.some((t) => t.value === 0));
  assert.ok(ticks.some((t) => t.value > 0));
  assert.deepEqual(ticks.map((t) => t.value), [...ticks.map((t) => t.value)].sort((a, b) => a - b));

  // An all-positive plan still reads as a one-sided log axis: no wasted frame.
  const grow = makeScale(2, logDomain([100_000, 5_000_000]), { kind: "log" });
  assert.equal(grow.y(100_000), grow.baseline);
  assert.ok(valueTicks(grow).every((t) => t.value > 0));
});
