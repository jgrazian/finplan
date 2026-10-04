import assert from "node:assert/strict";
import { test } from "node:test";
import type { DrawdownBody, DrawdownChoice, DrawdownYear } from "../lib/api/types.ts";
import {
  axisLabel,
  choiceKey,
  choiceOfKey,
  comparisonLines,
  drawdownCsv,
  drawdownView,
  isCurrentChoice,
  policyFor,
  yearPanel,
} from "../lib/view/drawdown.ts";

function year(y: number, over: Partial<DrawdownYear> = {}): DrawdownYear {
  return {
    year: y,
    inflation: 1,
    spending: 100,
    income: [40],
    withdrawals: [70, 0],
    rmd: [0, 0],
    withdrawal_taxes: 10,
    cash: 0,
    surplus: 0,
    shortfall: 0,
    balances: [900, 500],
    total_tax: 12,
    ...over,
  };
}

const choice = (years: DrawdownYear[]): DrawdownChoice => ({
  choice: { kind: "Strategy", strategy: "TaxEfficientEarly" },
  overlay: false,
  summary: {
    lifetime_spending: 0,
    lifetime_tax: 0,
    ending_balance: 1000,
    ending_balance_real: 800,
    after_tax_ending_balance: 900,
    after_tax_ending_balance_real: 720,
    markers: [{ kind: "income", id: 7, year: 2041 }, { kind: "rmd", id: 2, year: 2042 }],
  },
  years,
});

const body = (years: DrawdownYear[]): DrawdownBody => ({
  seed: "1",
  retirement: { year: 2041, age: 60, source: "income" },
  birth_year: 1981,
  accounts: [
    { id: 1, name: "Brokerage", flavor: "Investment", tax_status: "Taxable" },
    { id: 2, name: "Roth", flavor: "Investment", tax_status: "TaxFree" },
  ],
  income_sources: [{ event_id: 7, name: "Social Security" }],
  fixed_sweeps: [],
  choices: [choice(years)],
});

test("sources meet the line; the tax hangs below zero", () => {
  const years = [year(2041), year(2042, { withdrawals: [70, 0], rmd: [20, 0], surplus: 5, withdrawal_taxes: 10 })];
  const b = body(years);
  const view = drawdownView(b, b.choices[0], "usd", "nominal");
  assert.deepEqual(view.series.map((s) => s.key), ["income:7", "account:1", "taxes"]);
  const first = view.columns[0];
  // 40 income + 60 net sales cover the 100 target; the 10 tax runs 0 to -10.
  assert.equal(first.stack.at(-1)?.to, 100);
  assert.deepEqual([first.taxes?.from, first.taxes?.to], [0, -10]);
  assert.equal(first.target, 100);
  assert.equal(first.cap, undefined);
  assert.equal(first.top, 100);
  // Only a real surplus rises past the line.
  const second = view.columns[1];
  assert.equal(second.surplus?.from, 100);
  assert.equal(second.top, 105);
  assert.equal(view.min, -10);
  assert.equal(first.age, 60);
});

test("share view normalises each bar and drops the target line", () => {
  const b = body([year(2041)]);
  const view = drawdownView(b, b.choices[0], "share", "nominal");
  assert.equal(view.columns[0].target, undefined);
  assert.ok(Math.abs(view.columns[0].top - 1) < 1e-9);
  assert.equal(view.max, 1);
  // The tax is not a share of spending.
  assert.equal(view.columns[0].taxes, undefined);
  assert.equal(view.min, 0);
});

test("the panel and the lifetime split report the tax beside the sources", () => {
  const b = body([year(2041), year(2042)]);
  const v = drawdownView(b, b.choices[0], "usd", "nominal");
  const panel = yearPanel(b, b.choices[0], v, 0)!;
  assert.ok(panel.rows.every((r) => r.key !== "taxes"));
  assert.equal(panel.taxes?.amount, 10);
  assert.equal(panel.taxes?.ofSpending, 0.1);
  assert.equal(panel.fundedAmount, 100);
  assert.ok(v.lifetime.every((l) => l.key !== "taxes"));
  assert.ok(Math.abs(v.lifetime.reduce((a, l) => a + l.share, 0) - 1) < 1e-9);
  assert.equal(v.lifetimeTax, 20);
  assert.equal(v.lifetimeTaxShare, 0.1);
  assert.equal(axisLabel("usd")(-20_000), axisLabel("usd")(20_000));
});

test("a shortfall is a cap up to the line; today's dollars deflate", () => {
  const b = body([year(2041, { inflation: 2, withdrawals: [20, 0], withdrawal_taxes: 0, shortfall: 40 })]);
  const real = drawdownView(b, b.choices[0], "usd", "real");
  const col = real.columns[0];
  assert.equal(col.target, 50);
  assert.equal(col.cap?.from, 30);
  assert.equal(col.cap?.to, 50);
  assert.ok(real.hasShortfall);
  assert.equal(real.endBalance, 800);
  const panel = yearPanel(b, b.choices[0], real, 0)!;
  assert.equal(panel.note?.tone, "bad");
  assert.ok(panel.funded < 1);
});

test("panel rows carry sub-lines and dim the idle ones", () => {
  const b = body([year(2041, { rmd: [10, 0] })]);
  const v = drawdownView(b, b.choices[0], "usd", "nominal");
  const panel = yearPanel(b, b.choices[0], v, 0)!;
  const brokerage = panel.rows.find((r) => r.key === "account:1")!;
  assert.match(brokerage.sub, /\$900 left · RMD \$10/);
  assert.equal(panel.rows.find((r) => r.key === "income:7")?.sub, "Income");
  assert.equal(panel.funded, 1);
});

test("markers sit on their year", () => {
  const b = body([year(2041), year(2042)]);
  const v = drawdownView(b, b.choices[0], "usd", "nominal");
  assert.deepEqual(v.markers.map((m) => [m.index, m.label]), [[0, "Social Security"], [1, "Roth RMD"]]);
});

test("choice keys round-trip and bracket ceilings are part of the key", () => {
  for (const key of ["planned", "tax-efficient", "pro-rata", "bracket-22"]) {
    const c = choiceOfKey(key)!;
    assert.equal(choiceKey(c), key);
  }
  assert.equal(choiceOfKey("nope"), undefined);
  assert.equal(choiceKey({ kind: "Strategy", strategy: "BracketFilling" }), "bracket-12");
});

test("the current chip and the policy Apply writes", () => {
  const funding = { strategy: "ProRata" as const, exclude_accounts: [3] };
  assert.ok(isCurrentChoice({ kind: "Strategy", strategy: "ProRata" }, funding));
  assert.ok(isCurrentChoice({ kind: "AsPlanned" }, null));
  assert.equal(policyFor({ kind: "AsPlanned" }, funding), undefined);
  assert.deepEqual(policyFor({ kind: "Strategy", strategy: "ProRata" }, funding)?.exclude_accounts, [3]);
});

test("the comparison picks the best of each column, unless all tie", () => {
  const row = (strategy: "ProRata" | "TaxFreeFirst", s: number, e: number, t: number) => ({
    choice: { kind: "Strategy" as const, strategy },
    overlay: false,
    success_rate: s,
    median_final_net_worth: e,
    median_after_tax_ending_balance: e,
    median_path_tax: t,
  });
  const lines = comparisonLines(
    { iterations: 200, retirement: { year: 2041, source: "start" }, rows: [row("ProRata", 0.9, 100, 50), row("TaxFreeFirst", 0.95, 100, 40)] },
    "nominal",
  );
  assert.deepEqual(lines.map((l) => l.bestSuccess), [false, true]);
  assert.deepEqual(lines.map((l) => l.bestEnding), [false, false]);
  assert.deepEqual(lines.map((l) => l.bestTax), [false, true]);
});

test("csv has a header, a row per year and quotes names", () => {
  const b = body([year(2041)]);
  b.income_sources[0].name = 'Pension, "main"';
  const csv = drawdownCsv(b, b.choices[0], "nominal").trim().split("\n");
  assert.equal(csv.length, 2);
  assert.match(csv[0], /"Pension, ""main"""/);
  assert.ok(csv[1].startsWith("2041,60,100,40,60,10"));
});

test("an RMD year marks the distribution after tax, from zero", () => {
  // 40 income and an 80 RMD taxed 10: 110 arrives against a 100 target.
  const forced = year(2042, { withdrawals: [80, 0], rmd: [80, 0], surplus: 10 });
  const b = body([year(2041), forced]);
  const v = drawdownView(b, b.choices[0], "usd", "nominal");
  assert.ok(v.hasRmd);
  assert.equal(v.columns[0].rmd, undefined);
  // 80 required, 10 of it withheld: 70, under the 100 target. Income is not in it.
  assert.equal(v.columns[1].rmd, 70);
  assert.equal(v.columns[1].top, 110);
  const panel = yearPanel(b, b.choices[0], v, 1)!;
  assert.deepEqual(panel.rmd, { amount: 80, afterTax: 70, accounts: ["Brokerage"] });
  assert.match(panel.note?.text ?? "", /required \$80 RMD exceeds the need by \$10/);
  assert.equal(drawdownView(b, b.choices[0], "share", "nominal").columns[1].rmd, undefined);
});

test("the comparison headlines the after-tax balance and keeps pre-tax for the hover", () => {
  const row = (strategy: "TaxDeferredFirst" | "TaxFreeFirst", preTax: number, afterTax: number) => ({
    choice: { kind: "Strategy" as const, strategy },
    overlay: false,
    success_rate: 0.9,
    median_final_net_worth: preTax,
    median_final_net_worth_real: preTax / 2,
    median_after_tax_ending_balance: afterTax,
    median_after_tax_ending_balance_real: afterTax / 2,
    median_path_tax: 10,
  });
  // Drawing the 401(k) first ends smaller before tax but larger after it.
  const comparison = {
    iterations: 200,
    retirement: { year: 2041, source: "start" as const },
    rows: [row("TaxDeferredFirst", 900, 850), row("TaxFreeFirst", 1000, 790)],
  };
  const lines = comparisonLines(comparison, "nominal");
  assert.deepEqual(lines.map((l) => l.afterTaxEndingBalance), [850, 790]);
  assert.deepEqual(lines.map((l) => l.endingBalance), [900, 1000]);
  assert.deepEqual(lines.map((l) => l.bestEnding), [true, false]);
  const real = comparisonLines(comparison, "real");
  assert.deepEqual(real.map((l) => l.afterTaxEndingBalance), [425, 395]);
  assert.deepEqual(real.map((l) => l.endingBalance), [450, 500]);
});

test("the side panel's balance carries the after-tax figure in the chosen basis", () => {
  const b = body([year(2041)]);
  assert.equal(drawdownView(b, b.choices[0], "usd", "nominal").endBalanceAfterTax, 900);
  assert.equal(drawdownView(b, b.choices[0], "usd", "real").endBalanceAfterTax, 720);
});
