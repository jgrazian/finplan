import assert from "node:assert/strict";
import { test } from "node:test";
import type {
  AnalysisParameter,
  WhatIfEntry,
  WhatIfOutcome,
  WhatIfStep,
} from "../lib/api/types.ts";
import {
  addOptions,
  amountStep,
  changedSummary,
  fanView,
  impactText,
  layerImpacts,
  layerView,
  newLayer,
  niceStep,
  runKey,
  runnableEntries,
  stepLayer,
  waterfallView,
  whatIfStats,
  type WhatIfContext,
} from "../lib/view/whatIf.ts";

const spend: AnalysisParameter = {
  id: "parameter:1",
  parameter_id: 1,
  name: "Spending",
  kind: "amount",
  current: 48_000,
  min: 30_000,
  max: 70_000,
};
const retire: AnalysisParameter = {
  id: "parameter:2",
  parameter_id: 2,
  name: "Retire at",
  kind: "age",
  current: 65,
  min: 55,
  max: 75,
};
const growth: AnalysisParameter = {
  id: "parameter:3",
  parameter_id: 3,
  name: "Growth",
  kind: "rate",
  current: 0.05,
  min: 0,
  max: 0.1,
};

const ctx: WhatIfContext = { parameters: [spend, retire, growth], ages: { start: 40, end: 95 } };

function step(success: number, median: number, dry: number | null): WhatIfStep {
  return {
    point: { success_rate: success, funding_success_rate: null, p5: 0, p50: 0, p95: 0 },
    median_end_real: median,
    p10_dry_at: dry,
  };
}

function outcome(steps: WhatIfStep[], ages: number[] | null = [40, 50, 60, 70, 80, 90, 95]): WhatIfOutcome {
  const n = (ages ?? [2026, 2036]).length;
  const fan = { p25: Array(n).fill(5e5), p50: Array(n).fill(1e6), p75: Array(n).fill(1.5e6) };
  return {
    steps,
    ages,
    years: ages ? ages.map((a) => 1986 + a) : [2026, 2036],
    plan_fan: fan,
    what_if_fan: fan,
    plan_retirement_age: 65,
    what_if_retirement_age: 67,
  };
}

test("steps are round and stable", () => {
  assert.equal(niceStep(2_400), 2_500);
  assert.equal(niceStep(6_000), 5_000);
  assert.equal(amountStep(48_000), 2_500);
  assert.equal(amountStep(0), 1_000);
});

test("stepping each kind of layer moves it by its own unit, within bounds", () => {
  const amount = stepLayer({ kind: "parameter", parameter_id: 1, value: 48_000 }, "value", 1, ctx);
  assert.deepEqual(amount, { kind: "parameter", parameter_id: 1, value: 50_500 });
  const rate = stepLayer({ kind: "parameter", parameter_id: 3, value: 0.05 }, "value", -1, ctx);
  assert.equal(rate.kind === "parameter" && rate.value, 0.0475);
  const age = stepLayer({ kind: "parameter", parameter_id: 2, value: 65 }, "value", 1, ctx);
  assert.equal(age.kind === "parameter" && age.value, 66);

  const shock = { kind: "market-shock" as const, age: 95, drop: 0.9 };
  assert.deepEqual(stepLayer(shock, "drop", 1, ctx), shock);
  assert.deepEqual(stepLayer(shock, "age", 1, ctx), shock);
  assert.equal((stepLayer(shock, "drop", -1, ctx) as typeof shock).drop, 0.85);

  const cost = { kind: "one-off" as const, age: 58, amount: -5_000, account_id: null };
  assert.equal((stepLayer(cost, "amount", -1, ctx) as typeof cost).amount, -5_000);
  assert.equal((stepLayer(cost, "amount", 1, ctx) as typeof cost).amount, -10_000);
});

test("layers read as sentences", () => {
  const shock = layerView(
    { id: "a", enabled: true, layer: { kind: "market-shock", age: 67, drop: 0.3 } },
    ctx,
  );
  assert.equal(shock.kind, "market shock");
  assert.deepEqual(
    shock.parts.map((p) => (p.type === "text" ? p.text : `[${p.value}]`)),
    ["Markets fall", "[30%]", "at age", "[67]"],
  );
  const cost = layerView(
    { id: "b", enabled: true, layer: { kind: "one-off", age: 58, amount: -40_000, account_id: null } },
    ctx,
  );
  assert.equal(cost.kind, "one-off cost");
  assert.equal(cost.parts[0].type === "text" && cost.parts[0].text, "One-off cost of");
  const stale = layerView(
    { id: "c", enabled: true, layer: { kind: "parameter", parameter_id: 99, value: 1 } },
    ctx,
  );
  assert.ok(stale.problem);
});

test("age layers cannot run without a birth date", () => {
  const noAge: WhatIfContext = { ...ctx, ages: null };
  const entries: WhatIfEntry[] = [
    { id: "a", enabled: true, layer: { kind: "market-shock", age: 67, drop: 0.3 } },
    { id: "b", enabled: true, layer: { kind: "parameter", parameter_id: 1, value: 50_000 } },
  ];
  assert.deepEqual(runnableEntries(entries, noAge).map((e) => e.id), ["b"]);
  const options = addOptions(entries, noAge);
  assert.ok(options.find((o) => o.key === "market-shock")?.disabled);
  // Spending is already overridden, so it is not offered again.
  assert.equal(options.some((o) => o.key === "parameter:1"), false);
});

test("impacts are the difference between consecutive steps", () => {
  const result = outcome([step(0.9, 1e6, null), step(0.85, 8e5, 88), step(0.87, 9e5, null)]);
  const impacts = layerImpacts(result, ["a", "b"]);
  assert.ok(Math.abs(impacts.get("a")! + 0.05) < 1e-9);
  const entry: WhatIfEntry = { id: "a", enabled: true, layer: { kind: "market-shock", age: 67, drop: 0.3 } };
  assert.equal(impactText(entry, impacts, null), "−5.0 pts");
  assert.equal(impactText({ ...entry, enabled: false }, impacts, null), "off");
});

test("stats compare the last step against the plan", () => {
  const stats = whatIfStats(outcome([step(0.9, 1_000_000, 71), step(0.873, 880_000, null)]));
  assert.equal(stats.success, "87.3%");
  assert.equal(stats.successDelta, "−2.7 pts vs plan");
  assert.equal(stats.endLabel, "median at 95");
  assert.equal(stats.end, "$880k");
  assert.equal(stats.endDelta, "−$120k vs plan");
  assert.equal(stats.dry, "never");
  assert.equal(stats.dryDelta, "plan: age 71");
  assert.equal(whatIfStats(outcome([step(0.9, 1e6, null)])).successDelta, "same as plan");
});

test("the waterfall runs plan → layers → total, carrying each level", () => {
  const view = waterfallView(outcome([step(0.9, 0, null), step(0.85, 0, null), step(0.87, 0, null)]), [
    "Crash",
    "Spending",
  ]);
  assert.deepEqual(view.bars.map((b) => b.tone), ["plan", "loss", "gain", "total"]);
  assert.deepEqual(view.bars.map((b) => b.label), ["90.0%", "−5.0", "+2.0", "87.0%"]);
  // The loss bar hangs from the plan's level down to the new one.
  assert.equal(view.bars[0].connector?.y, view.bars[1].y);
  assert.equal(view.bars[3].connector, null);
});

test("the fan draws retirement rules only when plotted by age", () => {
  const byAge = fanView(outcome([step(0.9, 0, null)]))!;
  assert.ok(byAge.planRetire != null && byAge.whatIfRetire != null);
  assert.ok(byAge.planBand.endsWith("Z"));
  const byYear = fanView(outcome([step(0.9, 0, null)], null))!;
  assert.equal(byYear.planRetire, null);
});

test("the fan's band is P25–P75 around the P50 centreline", () => {
  const view = fanView(outcome([step(0.9, 0, null)]))!;
  const firstY = (d: string) => Number(d.split(" ")[1]);
  // P75 leads the band path; it sits above (smaller y than) the centreline.
  assert.ok(firstY(view.whatIfBand) < firstY(view.whatIfMedian));
});

test("new layers start where they change nothing, or at a round figure", () => {
  assert.deepEqual(newLayer({ kind: "parameter", parameterId: 1 }, ctx), {
    kind: "parameter",
    parameter_id: 1,
    value: 48_000,
  });
  assert.deepEqual(newLayer({ kind: "market-shock" }, ctx, 66), { kind: "market-shock", age: 66, drop: 0.3 });
  const cost = newLayer({ kind: "cost" }, ctx);
  assert.equal(cost.kind === "one-off" && cost.amount, -40_000);
});

test("the run key ignores which entries are off", () => {
  const a: WhatIfEntry = { id: "a", enabled: true, layer: { kind: "market-shock", age: 67, drop: 0.3 } };
  const b: WhatIfEntry = { id: "b", enabled: false, layer: { kind: "one-off", age: 58, amount: -1, account_id: null } };
  assert.equal(runKey(runnableEntries([a, b], ctx)), runKey(runnableEntries([a], ctx)));
  assert.equal(changedSummary([], ctx), "No overrides — this is the plan as saved.");
  assert.match(changedSummary([a, b], ctx), /^1 override on, 1 off/);
});
