import assert from "node:assert/strict";
import { test } from "node:test";
import { clampEffort, effortStops, parseStoredEffort } from "../lib/view/effort.ts";
import { guestNotice, SIGN_UP_FOR_ITERATIONS } from "../lib/view/guest.ts";

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

test("a guest's cap of 100 offers only 100", () => {
  assert.deepEqual(effortStops(100), [{ iterations: 100 }]);
});

test("stops above the cap are dropped", () => {
  assert.deepEqual(
    effortStops(1_000).map((s) => s.iterations),
    [100, 250, 1_000],
  );
  assert.deepEqual(
    effortStops(undefined).map((s) => s.iterations),
    [100, 250, 1_000, 2_000, 5_000, 10_000],
  );
  // A cap under the lowest stop is still a run the account can start.
  assert.deepEqual(effortStops(50), [{ iterations: 50 }]);
});

test("an effort above the cap comes down to the highest stop allowed", () => {
  assert.deepEqual(clampEffort({ iterations: 10_000 }, 5_000), { iterations: 5_000 });
  assert.deepEqual(clampEffort({ iterations: 1_000 }, 100), { iterations: 100 });
  const kept = { iterations: 250 };
  assert.equal(clampEffort(kept, 5_000), kept);
  assert.equal(clampEffort(kept), kept);
});

test("a stored stop is kept only while it is still a stop", () => {
  assert.deepEqual(parseStoredEffort("10000"), { iterations: 10_000 });
  assert.deepEqual(parseStoredEffort("250"), { iterations: 250 });
  assert.equal(parseStoredEffort(null), undefined);
  assert.equal(parseStoredEffort(""), undefined);
  assert.equal(parseStoredEffort("500"), undefined);
  assert.equal(parseStoredEffort("converge"), undefined);
});
