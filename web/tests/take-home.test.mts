import assert from "node:assert/strict";
import test from "node:test";
import type { TaxConfig } from "../lib/api/types.ts";
import { federalTax, takeHome } from "../lib/view/takeHome.ts";

const config: TaxConfig = {
  id: 1,
  name: "Test brackets",
  description: null,
  state_rate: 0.05,
  capital_gains_rate: 0.15,
  early_withdrawal_penalty_rate: 0.1,
  standard_deduction: 15_000,
  age_65_extra_deduction: 0,
  federal_brackets: [
    { threshold: 11_925, rate: 0.12 },
    { threshold: 0, rate: 0.1 },
  ],
};

test("federal tax folds the standard deduction in as a 0% band", () => {
  assert.equal(federalTax(15_000, config), 0);
  assert.equal(federalTax(26_925, config), 1_192.5);
  assert.equal(federalTax(50_000, config), 1_192.5 + 23_075 * 0.12);
});

test("a salary below spending shows the shortfall after income tax", () => {
  const year = takeHome(50_000, 0, 54_000, config);
  assert.equal(year.federal, 3_961.5);
  assert.equal(year.state, 2_500);
  assert.equal(year.net, 43_538.5);
  assert.equal(year.surplus, 43_538.5 - 54_000);
});

test("a 401(k) deferral is pre-tax: out of gross before tax is figured", () => {
  const year = takeHome(50_000, 5_000, 0, config);
  assert.equal(year.federal, federalTax(45_000, config));
  assert.equal(year.state, 2_250);
  assert.equal(year.net, 45_000 - year.federal - 2_250);
});

test("no tax config means no tax", () => {
  const year = takeHome(50_000, 0, 40_000, undefined);
  assert.equal(year.net, 50_000);
  assert.equal(year.surplus, 10_000);
});
