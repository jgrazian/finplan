import assert from "node:assert/strict";
import { test } from "node:test";
import {
  draftOfEffect,
  effectProblem,
  emptyEffect,
  spendingRuleOf,
  toEffectSpec,
} from "../components/plan/effectDraft.ts";
import {
  describeRule,
  guardrails,
  portfolioValue,
  startingAmount,
  switchRule,
  toSpendingView,
} from "../lib/view/spending.ts";
import type { Account, Asset, EffectSpec, SpendingResults } from "../lib/api/types.ts";

const spending: EffectSpec = {
  kind: "Spending",
  from_account_id: 3,
  rule: { ...guardrails(80_000), floor: 50_000 },
  portfolio: { kind: "Accounts", account_ids: [1, 2] },
  pass_through: false,
};

test("a spending effect round-trips through the editor's draft", () => {
  const draft = draftOfEffect(spending, 3, 1);
  assert.equal(draft.form, "Spending");
  assert.deepEqual(draft.portfolioIds, [1, 2]);
  assert.equal(draft.passThrough, false);
  // Turned into an Expense, it would start at the rule's own amount.
  assert.equal(draft.amount, 80_000);
  assert.deepEqual(toEffectSpec(draft), spending);
  assert.equal(effectProblem(draft, 0), null);
});

test("an expense turned into a rule keeps its yearly figure", () => {
  const monthly = { ...emptyEffect(3, 1), form: "Spending" as const, amount: 5_000 };
  assert.deepEqual(spendingRuleOf(monthly, 12), guardrails(60_000));
  const liquid = toEffectSpec(monthly);
  assert.equal(liquid.kind, "Spending");
  if (liquid.kind === "Spending") {
    assert.deepEqual(liquid.portfolio, { kind: "Liquid" });
    assert.equal(liquid.pass_through, true);
  }
});

test("a rule out of range is a problem before it is sent", () => {
  const draft = draftOfEffect(spending, 3, 1);
  draft.spendingRule = { rule: "PercentOfPortfolio", rate: 0.3, floor: null, ceiling: null };
  assert.match(effectProblem(draft, 0) ?? "", /at most 20%/);
  draft.spendingRule = { rule: "PercentOfPortfolio", rate: 0.04, floor: 60_000, ceiling: 50_000 };
  assert.match(effectProblem(draft, 0) ?? "", /floor above its ceiling/);
});

test("switching rules keeps the starting amount", () => {
  const start = guardrails(40_000);
  const percent = switchRule(start, "PercentOfPortfolio", 1_000_000);
  assert.deepEqual(percent, { rule: "PercentOfPortfolio", rate: 0.04, floor: null, ceiling: null });
  assert.equal(startingAmount(percent, 1_000_000), 40_000);
  const back = switchRule(percent, "Guardrails", 1_000_000);
  assert.equal(back.rule === "Guardrails" && back.initial.kind === "Amount" && back.initial.amount, 40_000);
  const capped = switchRule(start, "CappedChange", 1_000_000);
  assert.equal(capped.rule === "CappedChange" && capped.rate, 0.04);
  // Without a portfolio to measure against, a rate rule takes its usual rate.
  assert.equal(switchRule(start, "PercentOfPortfolio", 0).rule === "PercentOfPortfolio" &&
    (switchRule(start, "PercentOfPortfolio", 0) as { rate: number }).rate, 0.04);
  assert.match(describeRule(start), /guardrails from \$40,000 a year, ±20%, steps of 10%/);
});

test("the portfolio's value counts banks and investments at opening prices", () => {
  const accounts = [
    { id: 1, flavor: "Bank", cash_value: 10_000, positions: [] },
    { id: 2, flavor: "Investment", cash_value: 500, positions: [{ asset_id: 7, units: 100 }] },
    { id: 3, flavor: "Property", value: 400_000, positions: [] },
  ] as unknown as Account[];
  const assets = [{ id: 7, initial_price: 50 }] as unknown as Asset[];
  assert.equal(portfolioValue(accounts, assets), 15_500);
  assert.equal(portfolioValue(accounts, assets, { kind: "Accounts", account_ids: [2] }), 5_500);
});

test("the results view keeps full calendar years only", () => {
  const point = (date: string, p50: number) => ({
    date, p5: p50 - 2, p10: p50 - 1, p25: p50 - 0.5, p50, p75: p50 + 0.5, p90: p50 + 1, p95: p50 + 2,
  });
  const results: SpendingResults = {
    num_iterations: 100,
    median_lifetime_spending: 1_500_000,
    p10_lowest_draw: 41_000,
    cut_rate: 0.22,
    median_years_below_first: 2,
    median_first_draw: 60_000,
    rules: ["Capped change"],
    cut_cap: 0.025,
    points: [point("2026-12-31", 20), point("2027-12-31", 60), point("2028-12-31", 62), point("2029-09-03", 40)],
  };
  const view = toSpendingView(results, "2026-09-03", (date) => Number(date.slice(0, 4)) - 1960)!;
  assert.deepEqual(view.years, [2027, 2028]);
  assert.deepEqual(view.ages, [67, 68]);
  assert.deepEqual(view.p50, [60, 62]);
  assert.deepEqual(view.low, [59, 61]);
  assert.equal(view.cutRate, 0.22);
  assert.deepEqual(view.rules, ["Capped change"]);
  // Capped at 2.5% a year, no cut can go deeper than 10%.
  assert.equal(view.cutsCanExceed10, false);
  assert.equal(toSpendingView({ ...results, cut_cap: null }, "2026-01-01", () => 0)!.cutsCanExceed10, true);
  assert.equal(toSpendingView(undefined, "2026-01-01", () => 0), null);
  // A plan that starts on January 1 keeps its first year.
  assert.deepEqual(toSpendingView(results, "2026-01-01", () => 0)!.years, [2026, 2027, 2028]);
});
