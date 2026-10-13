import assert from "node:assert/strict";
import test from "node:test";
import type { Profile, TaxConfig } from "../lib/api/types.ts";
import {
  EXAMPLE,
  birthDateOf,
  birthParts,
  defaultReturnProfileIds,
  examplePlan,
  freshName,
} from "../lib/view/setup.ts";

const profile = (id: number, name: string) => ({ id, name }) as Profile;

test("a default name skips the ones already taken, ignoring case", () => {
  assert.equal(freshName([]), "My plan");
  assert.equal(freshName(["my plan", "My plan 2"]), "My plan 3");
  assert.equal(freshName(["My plan"], "Example plan"), "Example plan");
});

test("a birth year alone is mid-year; a month is its first day", () => {
  assert.equal(birthDateOf(1985, null), "1985-07-01");
  assert.equal(birthDateOf(1985, 3), "1985-03-01");
  // Half-typed or impossible years are no date at all.
  assert.equal(birthDateOf(198, null), "");
  assert.equal(birthDateOf(0, 3), "");
});

test("a known exact birth date is kept while the year and month agree", () => {
  assert.equal(birthDateOf(1985, 3, "1985-03-17"), "1985-03-17");
  assert.equal(birthDateOf(1985, 4, "1985-03-17"), "1985-04-01");
  assert.equal(birthDateOf(1985, null, "1985-03-17"), "1985-07-01");
});

test("a stored birth date reads back as its year and month", () => {
  assert.deepEqual(birthParts("1985-03-17"), { year: 1985, month: 3 });
  assert.deepEqual(birthParts(""), { year: 0, month: null });
  assert.deepEqual(birthParts(null), { year: 0, month: null });
});

test("the example plan uses the seeded profiles and is ready to submit", () => {
  const returnProfiles = [
    profile(4, "US Total Market"),
    profile(6, "US Aggregate Bonds"),
    profile(9, "Cash / T-Bills"),
  ];
  const ids = defaultReturnProfileIds(returnProfiles);
  assert.deepEqual(ids, { cash: 9, stocks: 4, bonds: 6 });

  const plan = examplePlan({
    name: "Example plan",
    returnProfiles,
    inflationProfiles: [profile(2, "US Historical")],
    taxConfigs: [{ id: 7 } as TaxConfig, { id: 3 } as TaxConfig],
    today: new Date("2026-10-12T12:00:00Z"),
  });
  assert.equal(plan.birth_date, `${2026 - EXAMPLE.age}-07-01`);
  assert.equal(plan.start_date, "2026-10-12");
  assert.equal(plan.cash_profile_id, 9);
  assert.equal(plan.savings_profile_id, 9);
  assert.equal(plan.stock_profile_id, 4);
  assert.equal(plan.bond_profile_id, 6);
  assert.equal(plan.inflation_profile_id, 2);
  // The lowest id is the seeded default.
  assert.equal(plan.tax_config_id, 3);
  assert.equal(plan.high_yield_savings, EXAMPLE.savings);
  assert.equal(plan.retirement_spending_rule?.rule, "Guardrails");
  assert.ok(plan.assumptions_confirmed && plan.fund_from_investments);
});

test("the example stands in any profile for a seeded one that was deleted", () => {
  const plan = examplePlan({
    name: "Example plan",
    returnProfiles: [profile(5, "Mine")],
    inflationProfiles: [],
    taxConfigs: [],
  });
  assert.equal(plan.stock_profile_id, 5);
  assert.equal(plan.inflation_profile_id, null);
  assert.throws(() =>
    examplePlan({ name: "x", returnProfiles: [], inflationProfiles: [], taxConfigs: [] }),
  );
});
