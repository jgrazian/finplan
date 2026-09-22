import assert from "node:assert/strict";
import { test } from "node:test";
import { scenarioDestination, toHref } from "../lib/nav/url.ts";

test("new scenario opens its own Portfolio with no previous row or section", () => {
  const destination = scenarioDestination(42, "portfolio");
  assert.deepEqual(destination, { scenario: 42, tab: "portfolio", section: "accounts" });
  assert.equal(toHref(destination), "/portfolio?scenario=42");
});
