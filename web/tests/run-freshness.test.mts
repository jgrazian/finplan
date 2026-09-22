import assert from "node:assert/strict";
import { test } from "node:test";
import { preferredRun, resultsAreStale } from "../lib/run/freshness.ts";
import type { Run } from "../lib/api/types.ts";

const scenario = { id: 1, updated_at: "2026-09-13 10:00:10" };
const run = { scenario_id: 1, created_at: "2026-09-13 10:00:00" };

test("edits during a run remain stale regardless of its later finish time", () => {
  assert.equal(resultsAreStale(scenario, run, false), true);
  assert.equal(resultsAreStale({ ...scenario, updated_at: run.created_at }, run, true), true);
  assert.equal(resultsAreStale({ ...scenario, updated_at: run.created_at }, run, false), false);
});

test("scenario switching never borrows freshness from another plan", () => {
  assert.equal(resultsAreStale({ ...scenario, id: 2 }, run, true), false);
  assert.equal(resultsAreStale(scenario, undefined, true), false);
});

test("a failed save does not stale unchanged inputs; a subsequent run covers saved edits", () => {
  const unchanged = { ...scenario, updated_at: run.created_at };
  assert.equal(resultsAreStale(unchanged, run, false), false);
  assert.equal(resultsAreStale(scenario, { ...run, created_at: scenario.updated_at }, false), false);
});

test("reload reconnects to active work and retains failures when no results exist", () => {
  const succeeded = { id: 1, status: "succeeded" } as Run;
  const running = { id: 2, status: "running" } as Run;
  const failed = { id: 3, status: "failed" } as Run;
  assert.equal(preferredRun([failed, running, succeeded]), running);
  assert.equal(preferredRun([failed, succeeded]), succeeded);
  assert.equal(preferredRun([failed]), failed);
  assert.equal(preferredRun([]), undefined);
});

test("percentile reads retain captured ages and dates; a replacement may capture new inputs", async () => {
  const { rememberRunContext } = await import("../lib/run/freshness.ts");
  const contexts = new Map();
  const identity = { ...run, id: 5 };
  const original = { birthDate: "1980-01-01", startDate: "2026-01-01" };
  const edited = { birthDate: "1990-01-01", startDate: "2030-01-01" };
  assert.equal(rememberRunContext(contexts, identity, original), original);
  assert.equal(rememberRunContext(contexts, identity, edited), original);
  assert.equal(rememberRunContext(contexts, { ...identity, scenario_id: 2 }, edited), edited);
  assert.equal(rememberRunContext(contexts, identity, edited, true), edited);
});
