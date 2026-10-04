import assert from "node:assert/strict";
import { test } from "node:test";
import {
  SERVER_SPEEDUP,
  decideRun,
  formatDuration,
  offloadAvailability,
  serverSeconds,
} from "../lib/local/estimate.ts";

test("under 10 seconds just runs", () => {
  assert.deepEqual(decideRun({ seconds: 0.4, constrained: false }), { kind: "run" });
  assert.deepEqual(decideRun({ seconds: 9.99, constrained: false }), { kind: "run" });
});

test("10 to 60 seconds runs with the estimate showing", () => {
  assert.deepEqual(decideRun({ seconds: 10, constrained: false }), {
    kind: "run-with-estimate",
    seconds: 10,
  });
  assert.deepEqual(decideRun({ seconds: 60, constrained: false }), {
    kind: "run-with-estimate",
    seconds: 60,
  });
});

test("over 60 seconds offers the servers, with their own estimate", () => {
  const choice = decideRun({ seconds: 120, constrained: false });
  assert.equal(choice.kind, "offer-offload");
  if (choice.kind !== "offer-offload") return;
  assert.equal(choice.reason, "slow");
  assert.equal(choice.seconds, 120);
  assert.equal(choice.serverSeconds, 120 / SERVER_SPEEDUP);
});

test("a constrained device is offered the servers once the run is not trivial", () => {
  const choice = decideRun({ seconds: 15, constrained: true });
  assert.equal(choice.kind, "offer-offload");
  if (choice.kind === "offer-offload") assert.equal(choice.reason, "constrained");
  // A quick run is not worth a question, even there.
  assert.deepEqual(decideRun({ seconds: 2, constrained: true }), { kind: "run" });
});

test("a nonsense estimate is a quick run, not a refusal", () => {
  assert.deepEqual(decideRun({ seconds: Number.NaN, constrained: false }), { kind: "run" });
  assert.deepEqual(decideRun({ seconds: -5, constrained: false }), { kind: "run" });
});

test("the server's estimate never reads as instant", () => {
  assert.equal(serverSeconds(61), 8);
  assert.equal(serverSeconds(1), 2);
});

test("durations read as people say them", () => {
  assert.equal(formatDuration(0.2), "1 s");
  assert.equal(formatDuration(45), "45 s");
  assert.equal(formatDuration(150), "3 min");
});

test("offload is unavailable without an account, and says to sign in", () => {
  const answer = offloadAvailability(false, undefined);
  assert.equal(answer.available, false);
  if (!answer.available) assert.equal(answer.signIn, true);
});

test("offload is available with budget, and explains a spent one with the reset date", () => {
  assert.deepEqual(
    offloadAvailability(true, {
      available: true,
      unavailable_reason: null,
      resets_at: "2026-11-01T00:00:00Z",
    }),
    { available: true },
  );
  const spent = offloadAvailability(true, {
    available: false,
    unavailable_reason: "budget_spent",
    resets_at: "2026-11-01T00:00:00Z",
  });
  assert.equal(spent.available, false);
  if (!spent.available) {
    assert.match(spent.reason, /spent/);
    // The server resets at 00:00 UTC, so the day is November 1 whatever the reader's zone.
    assert.match(spent.reason, /November 1/);
    assert.equal(spent.signIn, undefined);
  }
});

test("a budget that could not be read does not claim the servers are available", () => {
  const answer = offloadAvailability(true, undefined);
  assert.equal(answer.available, false);
});
