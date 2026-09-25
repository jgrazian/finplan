import assert from "node:assert/strict";
import { test } from "node:test";
import { draftOfTrigger, firesOnceMatters, toTriggerSpec, triggerProblem } from "../components/plan/triggerDraft.ts";

test("date and age parameters round trip through leaf and schedule controls", () => {
  const date = { kind: "DateParameter", parameter_id: 12 } as const;
  const age = { kind: "AgeParameter", parameter_id: 13 } as const;
  assert.deepEqual(toTriggerSpec(draftOfTrigger(date, 0, 0)), date);
  assert.deepEqual(toTriggerSpec(draftOfTrigger(age, 0, 0)), age);

  const repeating = {
    kind: "Repeating", interval: "Monthly", start_condition: date,
    end_condition: age, max_occurrences: null,
  } as const;
  const draft = draftOfTrigger(repeating, 0, 0);
  assert.equal(draft.raw, undefined);
  assert.equal(triggerProblem(draft), null);
  assert.deepEqual(toTriggerSpec(draft), repeating);
});

test("unselected typed parameters stop an event from saving", () => {
  const draft = draftOfTrigger({ kind: "DateParameter", parameter_id: 3 }, 0, 0);
  draft.condition.dateParameterId = 0;
  assert.match(triggerProblem(draft) ?? "", /needs a date parameter/);
});

test("fires once is offered only where a trigger can hold across steps", () => {
  const date = { kind: "Date", on_date: "2030-01-01" } as const;
  const netWorth = { kind: "NetWorth", comparison: "GreaterThanOrEqual", threshold: 1 } as const;
  const offered = (spec: Parameters<typeof draftOfTrigger>[0]) =>
    firesOnceMatters(draftOfTrigger(spec, 0, 0));

  assert.equal(offered(date), false);
  assert.equal(offered({ kind: "AgeParameter", parameter_id: 1 }), false);
  assert.equal(offered({ kind: "RelativeToEvent", event_id: 1, unit: "Months", value: 6 }), false);
  assert.equal(offered({
    kind: "Repeating", interval: "Monthly", start_condition: netWorth,
    end_condition: null, max_occurrences: null,
  }), false);

  assert.equal(offered(netWorth), true);
  assert.equal(offered({ kind: "AccountBalance", account_id: 1, comparison: "LessThanOrEqual", threshold: 0 }), true);
  assert.equal(offered({ kind: "And", children: [date, netWorth] }), true);
  assert.equal(offered({ kind: "Manual" }), true);
});
