import assert from "node:assert/strict";
import { test } from "node:test";
import type { TaxConfig } from "../lib/api/types.ts";
import { defaultTaxConfig } from "../lib/view/assumptions.ts";
import {
  bracketProblem,
  draftProblem,
  fromTaxDraft,
  scheduleChart,
  toTaxDraft,
  yearTax,
} from "../lib/view/taxSchedule.ts";

const single: TaxConfig = {
  id: 1,
  name: "Single",
  description: null,
  state_rate: 0.05,
  early_withdrawal_penalty_rate: 0.1,
  standard_deduction: 14_600,
  age_65_extra_deduction: 1_950,
  niit_threshold: 200_000,
  federal_brackets: [
    { threshold: 0, rate: 0.1 },
    { threshold: 11_600, rate: 0.12 },
    { threshold: 47_150, rate: 0.22 },
    { threshold: 100_525, rate: 0.24 },
    { threshold: 191_950, rate: 0.32 },
    { threshold: 243_725, rate: 0.35 },
    { threshold: 609_350, rate: 0.37 },
  ],
  capital_gains_brackets: [
    { threshold: 0, rate: 0 },
    { threshold: 47_025, rate: 0.15 },
    { threshold: 518_900, rate: 0.2 },
  ],
};

test("a config round-trips through the editor's percents unchanged", () => {
  const draft = toTaxDraft(single);
  assert.deepEqual(
    draft.federal.map((b) => b.rate),
    [10, 12, 22, 24, 32, 35, 37],
  );
  assert.equal(draft.stateRate, 5);
  const { name, state_rate, federal_brackets, capital_gains_brackets, niit_threshold } = fromTaxDraft(draft);
  assert.deepEqual(
    { name, state_rate, federal_brackets, capital_gains_brackets, niit_threshold },
    {
      name: single.name,
      state_rate: single.state_rate,
      federal_brackets: single.federal_brackets,
      capital_gains_brackets: single.capital_gains_brackets,
      niit_threshold: single.niit_threshold,
    },
  );
});

test("bracket tables the engine could not walk are named before saving", () => {
  assert.equal(bracketProblem([{ threshold: 0, rate: 10 }]), undefined);
  assert.match(bracketProblem([{ threshold: 5, rate: 10 }]) ?? "", /start at \$0/);
  assert.match(
    bracketProblem([
      { threshold: 0, rate: 10 },
      { threshold: 0, rate: 12 },
    ]) ?? "",
    /rise/,
  );
  assert.match(bracketProblem([{ threshold: 0, rate: 120 }]) ?? "", /0–100/);
  assert.match(draftProblem({ ...toTaxDraft(single), name: "  " }) ?? "", /name/);
});

test("gains stack on ordinary income past the deduction", () => {
  const draft = toTaxDraft(single);
  // $75,400 taxable: 10% of 11,600 + 12% of 35,550 + 22% of 28,250.
  const year = yearTax(draft, 90_000, 60_000);
  assert.equal(Math.round(year.ordinaryTax), 1_160 + 4_266 + 6_215);
  assert.equal(year.marginal, 22);
  // All $60k lands in the 15% band, which runs from $61,625 gross; under NIIT's $200k.
  assert.deepEqual(
    year.pieces.map((p) => [p.rate, p.amount]),
    [[15, 60_000]],
  );
  assert.equal(year.gainsTax, 9_000);

  // Low income: the first gains fill the deduction, then the 0% band.
  const low = yearTax(draft, 10_000, 60_000);
  assert.deepEqual(
    low.pieces.map((p) => [p.rate, p.amount]),
    [
      [null, 4_600],
      [0, 47_025],
      [15, 8_375],
    ],
  );

  // NIIT on the gains or the income over the threshold, whichever is smaller.
  const high = yearTax(draft, 180_000, 60_000);
  assert.equal(high.pieces.at(-1)?.amount, 40_000);
  assert.equal(Math.round(high.pieces.at(-1)?.tax ?? 0), 1_520);
});

test("the chart's axis runs past the top bracket of a joint table", () => {
  const joint = toTaxDraft({
    ...single,
    standard_deduction: 29_200,
    federal_brackets: [...single.federal_brackets.slice(0, 6), { threshold: 731_200, rate: 0.37 }],
  });
  const chart = scheduleChart(joint, { ordinary: 90_000, gains: 60_000 });
  const top = chart.ordinary.at(-1);
  assert.equal(top?.rate, 37);
  assert.ok((top?.width ?? 0) > 0, "the 37% band is on the chart");
  assert.equal(chart.ticks.at(-1)?.label, "$1M+");
  assert.equal(chart.ordinary[0].rate, null, "the deduction is the first band");
});

test("a new plan starts on the oldest tax config, not the first by name", () => {
  const joint = { ...single, id: 2, name: "Joint Household" };
  assert.equal(defaultTaxConfig([joint, single])?.name, "Single");
  assert.equal(defaultTaxConfig([]), undefined);
});
