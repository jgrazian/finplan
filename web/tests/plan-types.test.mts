import assert from "node:assert/strict";
import { test } from "node:test";
import {
  PLANS,
  applyPlanChoice,
  catchUpLabel,
  planChoiceOf,
  sameCatchUp,
} from "../lib/view/planTypes.ts";

test("a 401(k) starts at the 2026 deferral limit with both catch-ups", () => {
  const { planType, taxStatus, defaults } = applyPlanChoice("Roth401k");
  assert.equal(planType, "Roth401k");
  assert.equal(taxStatus, "TaxFree");
  assert.equal(defaults?.limit, 24_500);
  assert.equal(catchUpLabel(defaults?.catchUp ?? []), "+$8,000 at 50+ · +$11,250 at 60–63");
});

test("IRAs and HSAs carry their own catch-up ages", () => {
  assert.equal(catchUpLabel(PLANS.TraditionalIra.catchUp), "+$1,100 at 50+");
  assert.equal(PLANS.TraditionalIra.taxStatus, "TaxDeferred");
  assert.equal(catchUpLabel(PLANS.Hsa.catchUp), "+$1,000 at 55+");
});

test("an unnamed plan sets only the tax treatment and keeps the limits", () => {
  assert.deepEqual(applyPlanChoice("OtherTaxFree"), { planType: null, taxStatus: "TaxFree" });
  assert.equal(planChoiceOf(null, "TaxDeferred"), "OtherTaxDeferred");
  assert.equal(planChoiceOf("RothIra", "TaxFree"), "RothIra");
});

test("picking a plan hands out a copy, so editing it leaves the defaults alone", () => {
  const { defaults } = applyPlanChoice("Traditional401k");
  defaults!.catchUp[0].amount = 1;
  assert.equal(PLANS.Traditional401k.catchUp[0].amount, 8_000);
  assert.ok(!sameCatchUp(defaults!.catchUp, PLANS.Traditional401k.catchUp));
});
