import assert from "node:assert/strict";
import { test } from "node:test";
import { mergePlanLists } from "../lib/nav/merge.ts";
import {
  homeOf,
  localPlanId,
  parseNav,
  planRef,
  resolveScenario,
  scenarioDestination,
  toHref,
} from "../lib/nav/url.ts";

/** A row as either home lists it: local plans carry `slug = "l<id>"`. */
const row = (id: number, slug: string, updated_at = "2026-10-01 10:00:00") => ({
  id,
  slug,
  updated_at,
});

test("a ref's first character names its home", () => {
  assert.equal(homeOf("l12"), "local");
  assert.equal(homeOf("s8a4f21b7c903"), "cloud");
  // Before slugs a bookmark was the number, and every plan was the cloud's.
  assert.equal(homeOf("7"), "cloud");
  assert.equal(homeOf(undefined), "cloud");
  // `l` alone, or `l` and a name, is not a local ref.
  for (const ref of ["l", "lx", "l1a", "L12", "l-1", "l1.5"]) assert.equal(homeOf(ref), "cloud", ref);
});

test("a local ref carries the local row id", () => {
  assert.equal(localPlanId("l12"), 12);
  assert.equal(localPlanId("l0"), 0);
  assert.equal(localPlanId("s8a4f21b7c903"), undefined);
  assert.equal(localPlanId("12"), undefined);
  assert.equal(localPlanId(undefined), undefined);
});

test("planRef builds a ref once and passes a finished one through", () => {
  assert.equal(planRef("local", 12), "l12");
  assert.equal(planRef("local", "12"), "l12");
  assert.equal(planRef("local", "l12"), "l12");
  assert.equal(planRef("cloud", "s8a4f21b7c903"), "s8a4f21b7c903");
  assert.equal(planRef("cloud", 7), "7");
  assert.equal(homeOf(planRef("local", 3)), "local");
  assert.equal(localPlanId(planRef("local", 3)), 3);
});

test("parseNav accepts local refs and still rejects what it always did", () => {
  const state = parseNav("/plan", "?scenario=l12");
  assert.equal(state.scenario, "l12");
  assert.equal(state.tab, "plan");
  // Cloud slugs and legacy numbers are unchanged.
  assert.equal(parseNav("/plan", "?scenario=s8a4f21b7c903").scenario, "s8a4f21b7c903");
  assert.equal(parseNav("/plan", "?scenario=12").scenario, "12");
  for (const reference of ["", "l-1", "l 1", "l/1", "l1.5"]) {
    assert.equal(
      parseNav("/plan", new URLSearchParams({ scenario: reference }).toString()).scenario,
      undefined,
      reference,
    );
  }
});

test("a local plan's link round-trips with its tab, section and row", () => {
  const destination = scenarioDestination(planRef("local", 4), "portfolio");
  assert.equal(toHref(destination), "/portfolio?scenario=l4");
  const state = { scenario: "l4", tab: "analysis" as const, section: "sweep", selection: "Retire at 65" };
  const url = new URL(toHref(state), "https://example.com");
  assert.deepEqual(parseNav(url.pathname, url.search), state);
});

test("resolveScenario finds a plan by slug in a list holding both homes", () => {
  const plans = [row(3, "l3"), row(3, "s1111"), row(9, "l9"), row(5, "s2222")];
  assert.equal(resolveScenario(plans, "l3"), plans[0]);
  assert.equal(resolveScenario(plans, "s1111"), plans[1]);
  assert.equal(resolveScenario(plans, "l9"), plans[2]);
  assert.equal(resolveScenario(plans, "s2222"), plans[3]);
});

test("a bare number is an old cloud bookmark, never a local plan that shares it", () => {
  const plans = [row(3, "l3"), row(3, "s1111"), row(5, "l5")];
  assert.equal(resolveScenario(plans, "3"), plans[1]);
  // 5 exists only locally, so the number names nothing and the list's first wins.
  assert.equal(resolveScenario(plans, "5"), plans[0]);
});

test("a local ref finds its row by id when the slug differs, and falls back otherwise", () => {
  const plans = [row(1, "s1111"), row(8, "l8")];
  assert.equal(resolveScenario(plans, "l8"), plans[1]);
  // The same number in the cloud is a different plan.
  assert.equal(resolveScenario([row(8, "s8888")], "l8")?.slug, "s8888");
  assert.equal(resolveScenario(plans, "l77"), plans[0]);
  assert.equal(resolveScenario(plans, undefined), plans[0]);
  assert.equal(resolveScenario([], "l1"), undefined);
});

test("a merged list is newest first across homes, and one home alone keeps its own order", () => {
  const local = [row(1, "l1", "2026-10-02 09:00:00"), row(2, "l2", "2026-09-01 09:00:00")];
  const cloud = [row(1, "s1", "2026-10-03T08:00:00Z"), row(2, "s2", "2026-08-01 09:00:00")];
  const merged = mergePlanLists({ local: { rows: local }, cloud: { rows: cloud } });
  assert.deepEqual(
    merged.rows.map((r) => r.slug),
    ["s1", "l1", "l2", "s2"],
  );
  assert.deepEqual(merged.failed, []);
  assert.equal(merged.error, undefined);

  // Not re-sorted when there is nothing to merge: the server's order stands.
  const odd = [row(1, "s1", "2026-01-01 00:00:00"), row(2, "s2", "2026-06-01 00:00:00")];
  assert.deepEqual(mergePlanLists({ cloud: { rows: odd } }).rows, odd);
});

test("one home failing leaves the other's plans listed, and says which failed", () => {
  const down = new Error("store worker crashed");
  const cloud = [row(1, "s1")];
  const merged = mergePlanLists({ local: { error: down }, cloud: { rows: cloud } });
  assert.deepEqual(merged.rows, cloud);
  assert.deepEqual(merged.failed, ["local"]);
  assert.equal(merged.error, undefined);

  const other = mergePlanLists({ local: { rows: [row(1, "l1")] }, cloud: { error: new Error("offline") } });
  assert.deepEqual(other.rows.map((r) => r.slug), ["l1"]);
  assert.deepEqual(other.failed, ["cloud"]);
  assert.equal(other.error, undefined);
});

test("the list is an error only when no home answered", () => {
  const a = new Error("a");
  const merged = mergePlanLists({ local: { error: a }, cloud: { error: new Error("b") } });
  assert.deepEqual(merged.rows, []);
  assert.deepEqual(merged.failed, ["local", "cloud"]);
  assert.equal(merged.error, a);

  // A home that was not asked is neither an answer nor a failure.
  const alone = mergePlanLists({ local: undefined, cloud: { error: a } });
  assert.deepEqual(alone.failed, ["cloud"]);
  assert.equal(alone.error, a);
  assert.deepEqual(mergePlanLists({}), { rows: [], failed: [], error: undefined });
});
