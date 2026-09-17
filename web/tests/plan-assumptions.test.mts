import assert from "node:assert/strict";
import test from "node:test";
import type { TaxConfig } from "../lib/api/types.ts";
import type { InflationProfile } from "../lib/types.ts";
import { toInflationChoices, toTaxChoices } from "../lib/view/assumptions.ts";

function profile(over: Partial<InflationProfile>): InflationProfile {
  return {
    id: "Moderate",
    serverId: 1,
    kind: "Normal",
    mean: 2.5,
    sd: 1,
    note: "",
    distribution: { kind: "Normal", mean: 0.025, std_dev: 0.01 },
    ...over,
  };
}

function config(over: Partial<TaxConfig>): TaxConfig {
  return {
    id: 7,
    name: "US Federal 2024 (single)",
    description: null,
    state_rate: 0.05,
    capital_gains_rate: 0.15,
    early_withdrawal_penalty_rate: 0.1,
    federal_brackets: [
      { threshold: 0, rate: 0.1 },
      { threshold: 100_000, rate: 0.32 },
      { threshold: 609_350, rate: 0.37 },
    ],
    ...over,
  };
}

test("an inflation choice carries the rate its name does not", () => {
  const [choice] = toInflationChoices([profile({})]);
  assert.equal(choice.id, 1);
  assert.equal(choice.name, "Moderate");
  assert.equal(choice.detail, "2.5%/yr");
});

test("a profile with no closed-form mean says where its years come from", () => {
  const sampled = profile({
    kind: "Bootstrap",
    mean: null,
    sd: null,
    distribution: { kind: "Bootstrap", preset: "us-cpi", block_size: null },
  });
  assert.equal(toInflationChoices([sampled])[0].detail, "sampled history");

  const regimes = profile({ kind: "RegimeSwitching", mean: null, sd: null });
  assert.equal(toInflationChoices([regimes])[0].detail, "varies by year");
});

test("a tax choice reports the top bracket, not the first or the last listed", () => {
  const unordered = config({
    federal_brackets: [
      { threshold: 609_350, rate: 0.37 },
      { threshold: 0, rate: 0.1 },
    ],
  });
  assert.equal(toTaxChoices([unordered])[0].detail, "top 37% · state 5%");
});

test("a tax configuration with no description says what it charges instead", () => {
  const [described] = toTaxChoices([config({ description: "2024 brackets, 5% state" })]);
  assert.equal(described.note, "2024 brackets, 5% state");

  const [bare] = toTaxChoices([config({})]);
  assert.equal(bare.note, "15% long-term gains, 10% early-withdrawal penalty.");
});
