import assert from "node:assert/strict";
import { test } from "node:test";
import { clockTime, pathChecks, summarizeIssues } from "../lib/view/issues.ts";

const succeeded = {
  status: "succeeded", error_message: null, completed_iterations: 2000,
  iterations: 2000, max_iterations: null, converge: false,
};
const issue = (severity: string, code = "invalid_plan") =>
  ({ code, severity, message: `${code} message`, section: "plan", record_id: null });
const base = { run: succeeded, preflight: undefined, stale: false, warnings: [] };

test("a clean run with every iteration funded shows no strip", () => {
  const summary = summarizeIssues({ ...base, fundingSuccessRate: 1, iterations: 2000 });
  assert.equal(summary.strip, undefined);
  assert.equal(summary.count, 0);
});

test("a clean run with shortfall iterations gets the outline strip", () => {
  const summary = summarizeIssues({
    ...base, fundingSuccessRate: 0.9505, iterations: 2000,
    warnings: [{ id: "1", kind: "CashShortfall", title: "", detail: "2041-01-01 — USAA short" }],
  });
  assert.equal(summary.strip?.tone, "clean");
  assert.equal(summary.shortfall?.failed, 99);
  assert.equal(summary.shortfall?.firstInPath, "2041-01-01 — USAA short");
  assert.match(summary.strip!.message, /99 of 2,000 iterations/);
});

test("a failed run outranks a changed plan and carries the engine's message", () => {
  const summary = summarizeIssues({
    ...base,
    run: { ...succeeded, status: "failed", completed_iterations: 412, error_message: "account not found" },
    stale: true,
    preflight: { issues: [issue("error")], can_run: false },
  });
  assert.equal(summary.strip?.tone, "failed");
  assert.equal(summary.strip?.message, "account not found");
  assert.match(summary.runError!.message, /412 of 2,000/);
  assert.equal(summary.blocking.length, 1);
});

test("stale results with blocking issues count them in the label", () => {
  const summary = summarizeIssues({
    ...base, stale: true, lastRunAt: "12:04",
    preflight: { issues: [issue("error"), issue("error"), issue("warning", "empty_event")], can_run: false },
  });
  assert.equal(summary.strip?.label, "Plan changed · 2 issues");
  assert.match(summary.strip!.message, /last valid run \(12:04\)/);
  assert.equal(summary.review.length, 1);
});

test("the always-present preflight reminders are not reported as issues", () => {
  const summary = summarizeIssues({
    ...base,
    preflight: { issues: [issue("warning", "assumptions"), issue("warning", "horizon")], can_run: true },
  });
  assert.equal(summary.review.length, 0);
  assert.equal(summary.strip, undefined);
});

test("zoneless SQLite stamps are read as UTC", () => {
  const expected = new Date("2026-09-27T12:04:00Z")
    .toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false });
  assert.equal(clockTime("2026-09-27 12:04:11"), expected);
  assert.equal(clockTime(undefined), undefined);
});

test("event failures on the shown path are named and linkable; the first shortfall names its account", () => {
  const summary = summarizeIssues({
    ...base, fundingSuccessRate: 0.5, iterations: 10, pathLabel: "P10 path",
    warnings: [
      { id: "a", kind: "CashShortfall", title: "", detail: "x", date: "2041-02-01", accountId: 7, message: "short" },
      { id: "b", kind: "EffectSkipped", title: "", detail: "x", date: "2032-06-01", eventId: 3, message: "failed to apply effect: account “Chase” not found" },
      { id: "c", kind: "EvaluationFailed", title: "", detail: "x", message: "loan payment failed" },
    ],
    names: { event: (id) => (id === 3 ? "Home Purchase" : undefined), account: (id) => (id === 7 ? "USAA" : undefined) },
  });
  assert.equal(summary.shortfall?.firstInPath, "2041-02-01 · USAA");
  assert.deepEqual(summary.eventFailures.map((f) => [f.event, f.eventId]), [
    ["Home Purchase", 3],
    ["A scheduled payment", undefined],
  ]);
  assert.match(summary.eventFailures[0].message, /“Chase”/);
});

const flavors: Record<string, "Bank" | "Investment"> = { "1": "Bank", "2": "Investment" };
const flavorOf = (id: string) => flavors[id];
const years = [2030, 2031, 2032, 2033, 2034, 2035, 2036];

test("a withdrawal rate above 4% of the prior year-end invested balance is flagged at its peak", () => {
  const checks = pathChecks({
    years: years.slice(0, 3),
    cashFlows: [
      { year: 2031, withdrawals: 30_000, expenses: 0 }, // 3% of 1M
      { year: 2032, withdrawals: 60_000, expenses: 0 }, // 6% of 1M
    ],
    accountSeries: [{ accountId: "2", label: "Brokerage", values: [1e6, 1e6, 9e5] }],
    flavorOf,
  });
  assert.equal(checks.length, 1);
  assert.equal(checks[0].code, "withdrawal-rate");
  assert.match(checks[0].title, /6\.0%/);
  assert.match(checks[0].detail, /^2032: /);
});

test("cash is idle only when it grows past two years of spending five years running", () => {
  const spend = years.map((year) => ({ year, withdrawals: 0, expenses: 10_000 }));
  const idle = pathChecks({
    years, cashFlows: spend, flavorOf,
    accountSeries: [{ accountId: "1", label: "USAA", values: [15e3, 25e3, 30e3, 35e3, 40e3, 45e3, 44e3] }],
  });
  assert.equal(idle.length, 1);
  assert.match(idle[0].detail, /from 2031 to 2035/);

  const brief = pathChecks({
    years, cashFlows: spend, flavorOf,
    accountSeries: [{ accountId: "1", label: "USAA", values: [15e3, 25e3, 30e3, 29e3, 40e3, 45e3, 50e3] }],
  });
  assert.equal(brief.length, 0);
  // An investment account growing is the plan working, not idle cash.
  const invested = pathChecks({
    years, cashFlows: spend, flavorOf,
    accountSeries: [{ accountId: "2", label: "Brokerage", values: [15e3, 25e3, 30e3, 35e3, 40e3, 45e3, 50e3] }],
  });
  assert.equal(invested.length, 0);
});

test("path checks alone light the outline strip", () => {
  const summary = summarizeIssues({
    ...base, fundingSuccessRate: 1, iterations: 100, pathLabel: "P50 path",
    checks: [{ code: "idle-cash", title: "t", detail: "d" }],
  });
  assert.equal(summary.strip?.tone, "clean");
  assert.match(summary.strip!.message, /1 thing worth checking on the P50 path/);
});
