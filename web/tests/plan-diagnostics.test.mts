import assert from "node:assert/strict";
import { test } from "node:test";
import { fundingDiagnostics } from "../components/results/diagnostics.ts";

test("diagnostics use stored warning kinds, including processing failures, without parsing prose", () => {
  const shortfall = { id: "1", kind: "CashShortfall", title: "", detail: "Any localized message" };
  const processing = { id: "2", kind: "EffectSkipped", title: "", detail: "Effect unavailable" };
  const evaluation = { id: "3", kind: "EvaluationFailed", title: "", detail: "Evaluation unavailable" };
  const unrelated = { id: "4", kind: "Unknown", title: "", detail: "CashShortfall $1,000 in 2027" };
  assert.deepEqual(fundingDiagnostics([shortfall, processing, evaluation, unrelated]), {
    shortfalls: [shortfall], processing: [processing, evaluation],
  });
});

test("switching to a path without warnings does not retain another path's diagnostics", () => {
  const first = [{ id: "1", kind: "CashShortfall", title: "", detail: "Shortfall" }];
  assert.equal(fundingDiagnostics(first).shortfalls.length, 1);
  assert.deepEqual(fundingDiagnostics([]), { shortfalls: [], processing: [] });
});
