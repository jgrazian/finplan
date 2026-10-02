import assert from "node:assert/strict";
import { test } from "node:test";
import type { YearlyCashFlow } from "../lib/types.ts";
import { effectFamily } from "../lib/view/effectFamily.ts";
import {
  eventMarkers,
  stackLabels,
  successBand,
  successScalePosition,
} from "../lib/view/outcome.ts";

test("success rates land in the band whose floor they reach", () => {
  assert.equal(successBand(0.4).key, "fragile");
  assert.equal(successBand(0.7499).key, "fragile");
  assert.equal(successBand(0.75).key, "workable");
  assert.equal(successBand(0.899).key, "workable");
  assert.equal(successBand(0.9).key, "robust");
  assert.equal(successBand(1).key, "robust");
});

test("the drawn scale starts at 50% and clamps both ends", () => {
  assert.equal(successScalePosition(0.2), 0);
  assert.equal(successScalePosition(0.5), 0);
  assert.equal(successScalePosition(0.75), 0.5);
  assert.equal(successScalePosition(1), 1);
  assert.equal(successScalePosition(1.2), 1);
});

const flow = (year: number, tag?: string) =>
  ({ year, ledger: { total: 1, cash: 0, asset: 0, tax: 0, event: 0, tag } }) as YearlyCashFlow;

test("markers come from tagged years on the chart, never the plan's first year", () => {
  const markers = eventMarkers(
    [flow(2026, "Salary"), flow(2027), flow(2032, "Home Purchase"), flow(2037, "Retirement"), flow(2099, "Off chart")],
    [2026, 2027, 2028, 2029, 2030, 2031, 2032, 2033, 2034, 2035, 2036, 2037],
  );
  assert.deepEqual(markers, [
    { index: 6, year: 2032, label: "Home Purchase" },
    { index: 11, year: 2037, label: "Retirement" },
  ]);
});

test("labels that would overlap step down a row; ones with room share the top", () => {
  assert.deepEqual(
    stackLabels([
      { x: 0, width: 80 },
      { x: 50, width: 60 },
      { x: 90, width: 40 },
      { x: 120, width: 40 },
    ]),
    [0, 1, 0, 1],
  );
});

test("an event's family follows its first effect, and none makes it a marker", () => {
  assert.equal(effectFamily("Income"), "inflow");
  assert.equal(effectFamily("Expense"), "outflow");
  assert.equal(effectFamily("Sweep"), "transfer");
  assert.equal(effectFamily("TriggerEvent"), "control");
  assert.equal(effectFamily("MarketShock"), "shock");
  assert.equal(effectFamily(undefined), "marker");
});
