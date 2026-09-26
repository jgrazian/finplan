import assert from "node:assert/strict";
import { test } from "node:test";
import { parseNav, resolveScenario, scenarioDestination, toHref } from "../lib/nav/url.ts";

test("new scenario opens its own Portfolio using its slug", () => {
  const destination = scenarioDestination("s8a4f21b7c903", "portfolio");
  assert.deepEqual(destination, { scenario: "s8a4f21b7c903", tab: "portfolio", section: "accounts" });
  assert.equal(toHref(destination), "/portfolio?scenario=s8a4f21b7c903");
});

test("slug links preserve the tab, section and selection across reloads", () => {
  const state = { scenario: "s8a4f21b7c903", tab: "analysis" as const, section: "sweep", selection: "Retire at 65" };
  const url = new URL(toHref(state), "https://example.com");
  assert.deepEqual(parseNav(url.pathname, url.search), state);
});

test("invalid scenario references are ignored", () => {
  for (const reference of ["", "-1", "1.5", "a/b", "a b"]) {
    assert.equal(parseNav("/plan", new URLSearchParams({ scenario: reference }).toString()).scenario, undefined);
  }
});

test("scenario resolution handles slugs, legacy bookmarks and stale links", () => {
  const first = { id: 1, slug: "s111111111111" };
  const second = { id: 42, slug: "s222222222222" };
  const scenarios = [first, second];
  assert.equal(resolveScenario(scenarios, second.slug), second);
  const legacy = parseNav("/plan", "?scenario=42&sel=Retire");
  const resolved = resolveScenario(scenarios, legacy.scenario);
  assert.equal(resolved, second);
  assert.equal(toHref({ ...legacy, scenario: resolved?.slug }), "/plan?scenario=s222222222222&sel=Retire");
  assert.equal(resolveScenario(scenarios, "missing"), first);
  assert.equal(resolveScenario(scenarios, undefined), first);
  assert.equal(resolveScenario([], second.slug), undefined);
});
