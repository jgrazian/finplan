import assert from "node:assert/strict";
import { test } from "node:test";
import { draftOfEffect, emptyEffect, toEffectSpec } from "../components/plan/effectDraft.ts";

function sweep() {
  const effect = toEffectSpec({ ...emptyEffect(1, 2), form: "Sweep" });
  assert.equal(effect.kind, "Sweep");
  if (effect.kind !== "Sweep") throw new Error("Expected sweep");
  return effect;
}

test("new sweeps use a 12% bracket ceiling and edited percentages save as rates", () => {
  const effect = sweep();
  assert.deepEqual(effect.sources, {
    mode: "Strategy", strategy: "BracketFilling", exclude_accounts: [], bracket_ceiling: 0.12,
  });
  const edited = { ...draftOfEffect(effect, 1, 2), bracketCeiling: 22.5 };
  const saved = toEffectSpec(edited);
  assert.equal(saved.kind, "Sweep");
  if (saved.kind !== "Sweep" || saved.sources?.mode !== "Strategy") throw new Error("Expected strategy");
  assert.equal(saved.sources.bracket_ceiling, 0.225);
  assert.equal(draftOfEffect(saved, 1, 2).bracketCeiling, 22.5);
  const switched = toEffectSpec({ ...edited, strategy: "PenaltyAware" });
  if (switched.kind !== "Sweep" || switched.sources?.mode !== "Strategy") throw new Error("Expected strategy");
  assert.equal(switched.sources.bracket_ceiling, undefined);
});

test("saved sweeps retain omitted defaults, zero ceilings and excluded accounts", () => {
  const effect = sweep();
  assert.equal(draftOfEffect({ ...effect, sources: null }, 1, 2).strategy, "TaxEfficientEarly");
  const sources = { mode: "Strategy" as const, strategy: "BracketFilling" as const, exclude_accounts: [] };
  assert.equal(draftOfEffect({ ...effect, sources }, 1, 2).bracketCeiling, 12);
  const zero = draftOfEffect({ ...effect, sources: { ...sources, bracket_ceiling: 0 } }, 1, 2);
  assert.equal(zero.bracketCeiling, 0);
  assert.equal(zero.rawSources, undefined);
  const excluded = { ...effect, sources: { ...sources, exclude_accounts: [3], bracket_ceiling: 0.24 } };
  const saved = toEffectSpec(draftOfEffect(excluded, 1, 2));
  if (saved.kind !== "Sweep") throw new Error("Expected sweep");
  assert.deepEqual(saved.sources, excluded.sources);
});
