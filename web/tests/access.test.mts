import test from "node:test";
import assert from "node:assert/strict";
import { accessPresentation } from "../lib/view/access.ts";

test("beta feature access never presents a paid subscription or checkout", () => {
  const beta = accessPresentation({ access_mode: "beta", pro: true });
  assert.equal(beta.label, "Feedback beta");
  assert.equal(beta.showBilling, false);
  assert.match(beta.description, /Free access during the beta/);
  assert.match(beta.description, /Future plans and pricing may change/);
});

test("self-hosted access stays distinct from beta and subscription tiers", () => {
  const local = accessPresentation({ access_mode: "self_hosted", pro: true });
  assert.equal(local.label, "Self-hosted access");
  assert.equal(local.showBilling, false);
  for (const pro of [true, false]) {
    const subscription = accessPresentation({ access_mode: "subscription", pro });
    assert.equal(subscription.showBilling, true);
    assert.equal(subscription.label, pro ? "Pro" : "Free");
  }
});
