import assert from "node:assert/strict";
import { test } from "node:test";
import { clampEffort, effortStops } from "../lib/view/effort.ts";
import {
  guestNotice,
  intervalLabel,
  SIGN_UP_FOR_ITERATIONS,
  successIntervalPoints,
} from "../lib/view/guest.ts";

test("the interval is 1.96·√(p(1−p)/n) in percentage points", () => {
  const points = successIntervalPoints(0.8, 100);
  assert.ok(points != null);
  assert.ok(Math.abs(points - 7.84) < 1e-9);
  assert.equal(intervalLabel(points), "8");
});

test("more iterations narrow the interval, and the extremes have none", () => {
  const small = successIntervalPoints(0.8, 100)!;
  const large = successIntervalPoints(0.8, 1_000)!;
  assert.ok(large < small);
  assert.equal(intervalLabel(large), "2");
  assert.equal(successIntervalPoints(1, 100), 0);
  assert.equal(successIntervalPoints(0, 100), 0);
});

test("a tight interval keeps a decimal and nothing is claimed without a sample", () => {
  assert.equal(intervalLabel(0.62), "0.6");
  assert.equal(successIntervalPoints(0.8, 0), undefined);
  assert.equal(successIntervalPoints(Number.NaN, 100), undefined);
});

test("the guest banner names the retention window and the way out", () => {
  const notice = guestNotice(30);
  assert.equal(
    notice.lead + notice.action + notice.tail,
    "Guest plan — deleted after 30 days without a visit. Create a free account to keep it.",
  );
  assert.match(guestNotice(1).lead, /1 day without/);
  assert.match(guestNotice(null).lead, /^Guest plan — deleted/);
  assert.equal(SIGN_UP_FOR_ITERATIONS, "Sign up free for 1,000 iterations");
});

test("a guest's cap of 100 offers only 100 and hides converge", () => {
  assert.deepEqual(effortStops(100), [{ iterations: 100, converge: false }]);
});

test("stops above the cap are dropped and converge needs its floor of 500", () => {
  assert.deepEqual(
    effortStops(1_000).map((s) => (s.converge ? "converge" : s.iterations)),
    [100, 250, 1_000, "converge"],
  );
  assert.deepEqual(
    effortStops(499).map((s) => (s.converge ? "converge" : s.iterations)),
    [100, 250],
  );
  assert.equal(effortStops(undefined).length, 6);
  // A cap under the lowest stop is still a run the account can start.
  assert.deepEqual(effortStops(50), [{ iterations: 50, converge: false }]);
});

test("an effort above the cap comes down to the highest stop allowed", () => {
  assert.deepEqual(clampEffort({ iterations: 5_000, converge: false }, 1_000), {
    iterations: 1_000,
    converge: false,
  });
  assert.deepEqual(clampEffort({ iterations: 1_000, converge: false }, 100), {
    iterations: 100,
    converge: false,
  });
  assert.deepEqual(clampEffort({ iterations: 500, converge: true }, 100), {
    iterations: 100,
    converge: false,
  });
  const kept = { iterations: 250, converge: false };
  assert.equal(clampEffort(kept, 5_000), kept);
  assert.equal(clampEffort(kept), kept);
});
