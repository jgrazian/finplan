import assert from "node:assert/strict";
import { test } from "node:test";
import type { Change } from "../lib/api/generated/Change.ts";
import type { DraftColumn } from "../lib/api/generated/DraftColumn.ts";
import { clockTime } from "../lib/view/issues.ts";
import {
  ACTION_LABEL,
  actionsFor,
  aiLine,
  appliedLabel,
  board,
  checkLine,
  diffRow,
  evidenceLink,
  kicker,
  noteCounts,
  noteLink,
  pathHeadline,
  previewLine,
  problemText,
  problemsIn,
  refusalHeading,
  reviewBanner,
  selectedPath,
  stepProblemText,
  stepProblemsIn,
  toCard,
} from "../lib/view/review.ts";

const stats = (success: number, funding: number | null) => ({
  success_rate: success,
  funding_success_rate: funding,
  real_final: null,
  funding: null,
});

const removeSweep = {
  op: "remove" as const,
  target: { event: 5 },
  path: "/effects/0",
  expect: { kind: "Sweep" },
};

type Check = { iterations: number; paired: boolean; base: ReturnType<typeof stats>; edited: ReturnType<typeof stats> };
type Estimate = { success_rate: number | null; funding_success_rate: number | null };
type Diff = { label: string; from: string | null; to: string | null };

function step(overrides: Record<string, unknown> = {}) {
  return {
    key: "s1",
    title: "Pay the down payment from USAA",
    reasoning: null as string | null,
    changes: [removeSweep] as unknown as Change[],
    diff: [{ label: "Plan › Home Purchase › effects › Sweep", from: "Sweep $200,000 → USAA", to: null }] as Diff[],
    applied: false,
    applied_at: null as string | null,
    ...overrides,
  };
}

function path(overrides: Record<string, unknown> = {}) {
  return {
    key: "a",
    label: "Pay from cash",
    reasoning: null as string | null,
    recommended: false,
    steps: [step()],
    estimate: null as Estimate | null,
    check: null as Check | null,
    ...overrides,
  };
}

/**
 * A note. `changes` and `diff` shape its one path's one step, and `estimate`
 * and `check` that path's, as single-fix notes have; `changes: []` means no
 * path at all (a read note, or a check without an edit). `paths` replaces them.
 */
function note(overrides: Record<string, unknown> = {}) {
  const { changes, diff, estimate, check, paths, ...rest } = overrides;
  const single = path({
    steps: [step({ ...(changes !== undefined && { changes }), ...(diff !== undefined && { diff }) })],
    ...(estimate !== undefined && { estimate }),
    ...(check !== undefined && { check }),
  });
  return {
    id: 1,
    scenario_id: 1,
    run_id: 7,
    source: "rules" as const,
    rule: "sweep_sells_while_cash",
    kind: "fix" as const,
    section: "plan" as const,
    title: "Home Purchase sells $218k of investments while USAA holds $1.04M",
    reasoning: "The Sweep runs before the down payment.",
    evidence: [],
    paths: (paths ?? (Array.isArray(changes) && changes.length === 0 ? [] : [single])) as ReturnType<typeof path>[],
    applied_path: null as string | null,
    status: "open" as const,
    created_at: "2026-09-27 12:04:00",
    resolved_at: null as string | null,
    parent_id: null as number | null,
    note_key: null as string | null,
    blocked_by: [] as string[],
    column: null as DraftColumn | null,
    auto_added: false,
    ...rest,
  };
}

/** A single-fix note applied whole, at `resolved_at`. */
function appliedNote(overrides: Record<string, unknown> = {}) {
  const { check, ...rest } = overrides;
  return note({
    paths: [path({ steps: [step({ applied: true })], ...(check !== undefined && { check }) })],
    applied_path: "a",
    status: "applied" as const,
    ...rest,
  });
}

/** A card's actions, for the path its card would act on. */
const acts = (n: ReturnType<typeof note>) => actionsFor(n, selectedPath(n));

test("kicker names the kind and the rule's topic, falling back to the rule then the section", () => {
  assert.equal(kicker(note()), "Fix · sales you don't need");
  assert.equal(kicker(note({ kind: "check", rule: "brand_new_rule" })), "Check · brand new rule");
  assert.equal(kicker(note({ kind: "read", rule: null, section: "results" })), "Read · results");
});

test("actions follow the kind, and only open notes with changes apply or preview", () => {
  assert.deepEqual(acts(note()), ["apply", "preview", "dismiss"]);
  assert.deepEqual(acts(note({ check: { iterations: 2000, paired: true, base: stats(0.9, 0.9), edited: stats(0.9, 0.91) } })), ["apply", "dismiss"]);
  assert.deepEqual(acts(note({ kind: "check", changes: [] })), ["confirm", "dismiss"]);
  assert.deepEqual(acts(note({ kind: "check" })), ["apply", "confirm", "preview", "dismiss"]);
  assert.deepEqual(acts(note({ kind: "stress" })), ["apply-copy", "preview", "dismiss"]);
  assert.deepEqual(acts(note({ kind: "read", changes: [] })), ["dismiss"]);
  assert.deepEqual(acts(note({ status: "applied" })), []);
});

test("a simulated check quotes funding success when both sides have it", () => {
  const line = checkLine(path({
    check: { iterations: 2000, paired: true, base: stats(0.909, 0.9056), edited: stats(0.91, 0.908) },
  }));
  assert.deepEqual(line, {
    text: "funding 90.6% → 90.8% · simulated, 2,000 iterations",
    simulated: true,
    caveat: undefined,
  });
});

test("an unpaired check says part of the difference is sampling", () => {
  const line = checkLine(path({
    check: { iterations: 500, paired: false, base: stats(0.909, null), edited: stats(0.965, null) },
  }));
  assert.equal(line?.text, "success 90.9% → 96.5% · simulated, 500 iterations");
  assert.match(line?.caveat ?? "", /Not paired/);
});

test("without a check the estimate shows, marked as one; with neither there is no line", () => {
  assert.deepEqual(checkLine(path({ estimate: { success_rate: 0.95, funding_success_rate: 0.98 } })), {
    text: "est. funding ~98.0% · preview to simulate",
    simulated: false,
  });
  assert.equal(checkLine(path({ estimate: { success_rate: 0.95, funding_success_rate: null } }))?.text,
    "est. success ~95.0% · preview to simulate");
  assert.equal(checkLine(path()), undefined);
});

test("a preview reads like a check, and a refused one has no line", () => {
  const base = { base_run_id: 7, iterations: 2000, paired: true, diff: [], problems: [] };
  assert.equal(previewLine({ ...base, base: stats(0.9, 0.9), edited: stats(0.9, 0.92) })?.text,
    "funding 90.0% → 92.0% · simulated, 2,000 iterations");
  assert.equal(previewLine({ ...base, base: null, edited: null }), undefined);
});

test("evidence links name rows from the live plan and point at where they live", () => {
  const names = {
    account: (id: number) => (id === 6 ? "USAA" : undefined),
    event: (id: number) => (id === 5 ? "Home Purchase" : undefined),
  };
  assert.deepEqual(evidenceLink({ ref: "ledger", year: 2032, event_id: 5, account_id: null }, names), {
    label: "Ledger 2032 · Home Purchase",
    to: { tab: "results", year: 2032 },
  });
  assert.deepEqual(evidenceLink({ ref: "account_series", account_id: 6, date: "2031-12-31", value: 1_044_000 }, names), {
    label: "USAA · 2031: $1.04M",
    to: { tab: "portfolio", accountId: 6 },
  });
  assert.deepEqual(evidenceLink({ ref: "account_series", account_id: 99, date: "2031-12-31", value: 500 }, names).label,
    "Account · 2031: $500");
  assert.deepEqual(evidenceLink({ ref: "stat", name: "funding_success_rate", value: 0.9056 }), {
    label: "Funding success rate: 90.6%",
  });
  assert.deepEqual(evidenceLink({ ref: "diagnostic", field: "median_first_shortfall_year", value: 2041 }), {
    label: "Median first shortfall year: 2041",
    to: { tab: "results" },
  });
});

test("a card carries the server's diff as rows, a removal having no `to`", () => {
  const card = toCard(note({ source: "ai" }));
  assert.deepEqual(card.diff, [
    { label: "Plan › Home Purchase › effects › Sweep", from: "Sweep $200,000 → USAA", to: undefined, change: "remove" },
  ]);
  // One path of one step: no picker, no step list, and the actions act on it.
  assert.deepEqual(card.paths, []);
  assert.deepEqual(card.steps, []);
  assert.equal(card.path, "a");
  assert.equal(card.pathLabel, undefined);
  assert.equal(card.pathReasoning, undefined);
  assert.equal(card.source, "AI");
  assert.equal(toCard(note()).source, undefined);
});

test("the board shows open notes by section, fixes first, and counts the rest", () => {
  const review = {
    run_id: 7,
    reviewed_at: "2026-09-27 12:10:00",
    ai: null,
    suggestions: [
      note({ id: 1, kind: "read", section: "results", changes: [] }),
      note({ id: 2, kind: "check", section: "portfolio", rule: "idle_bank_cash", changes: [] }),
      note({ id: 3, kind: "fix", section: "plan" }),
      note({ id: 4, kind: "fix", section: "portfolio", rule: "liability_payment_inflation_adjusted" }),
      note({ id: 5, kind: "check", section: "plan", status: "dismissed" }),
    ],
  };
  const view = board(review, { runCreatedAt: "2026-09-27T12:04:00Z" });
  // Clock times are local, so the expectation is too.
  assert.equal(view.headline, `Reviewed the ${clockTime("2026-09-27T12:04:00Z")} run · 4 notes`);
  assert.equal(view.open, 4);
  assert.equal(view.applied, 0);
  assert.equal(view.resolved, 1);
  assert.deepEqual(view.columns.map((c) => c.heading), ["Portfolio", "Scenario & events", "Results"]);
  assert.deepEqual(view.columns.map((c) => c.cards.map((card) => card.id)), [[4, 2], [3], [1]]);
});

test("a review of an unknown run time falls back to the review's own time, then the run id", () => {
  const review = { run_id: 7, reviewed_at: "2026-09-27 12:10:00", ai: null, suggestions: [note()] };
  assert.equal(board(review).headline, `Reviewed the ${clockTime(review.reviewed_at)} run · 1 note`);
  assert.equal(board({ ...review, reviewed_at: "" }).headline, "Reviewed run #7 · 1 note");
});

test("apply buttons only apply: neither label promises a run", () => {
  assert.equal(ACTION_LABEL.apply, "Apply");
  assert.equal(ACTION_LABEL["apply-copy"], "Add as scenario");
  for (const label of Object.values(ACTION_LABEL)) assert.doesNotMatch(label, /run/i);
});

test("applied notes stay on the board after the open ones, marked, and are counted once", () => {
  const review = {
    run_id: 7,
    reviewed_at: "2026-09-27 12:10:00",
    ai: null,
    suggestions: [
      appliedNote({ id: 1, resolved_at: "2026-09-27 12:20:00" }),
      note({ id: 2 }),
      appliedNote({ id: 3, kind: "stress", resolved_at: "2026-09-27 12:21:00" }),
      appliedNote({ id: 4, kind: "check", resolved_at: "2026-09-27 12:22:00" }),
    ],
  };
  const latest = { id: 7, created_at: "2026-09-27 12:04:00" };
  const view = board(review, { runCreatedAt: latest.created_at, latest });
  const run = `the ${clockTime(latest.created_at)} run`;
  // The stress note went to a copy: it is not a change waiting on this plan.
  assert.equal(view.headline, `Reviewed ${run} · 1 note · 2 applied`);
  assert.equal(view.applied, 2);
  assert.equal(view.pending, 2);
  assert.deepEqual(view.columns[1].cards.map((c) => c.id), [2, 1, 4, 3]);
  const [open, fix, check, stress] = view.columns[1].cards;
  assert.equal(open.applied, undefined);
  assert.deepEqual(open.actions, ["apply", "preview", "dismiss"]);
  assert.equal(fix.applied, "Applied · not yet run");
  assert.deepEqual(fix.actions, []);
  assert.equal(check.applied, "Applied · not yet run");
  assert.equal(stress.applied, "Added as a scenario");
});

test("an applied note reads as in a run once a run made after it has finished", () => {
  const applied = appliedNote({ resolved_at: "2026-09-27 12:20:00" });
  assert.equal(appliedLabel(applied, { id: 7, created_at: "2026-09-27 12:04:00" }), "Applied · not yet run");
  const after = { id: 8, created_at: "2026-09-27 12:31:00" };
  assert.equal(appliedLabel(applied, after), `Applied · in the ${clockTime(after.created_at)} run`);
  assert.equal(appliedLabel(note(), after), undefined);
});

test("once a note is applied, the open notes' checks say what they were simulated against", () => {
  const check = { iterations: 2000, paired: true, base: stats(0.9, 0.9), edited: stats(0.9, 0.92) };
  const review = {
    run_id: 7,
    reviewed_at: "2026-09-27 12:10:00",
    ai: null,
    suggestions: [note({ id: 1, check }), appliedNote({ id: 2, check, resolved_at: "2026-09-27 12:20:00" })],
  };
  const runCreatedAt = "2026-09-27 12:04:00";
  const [open, applied] = board(review, { runCreatedAt }).columns[1].cards;
  assert.equal(open.check?.basis, `simulated against the ${clockTime(runCreatedAt)} run, before the applied changes`);
  assert.equal(applied.check?.basis, undefined);
  // Nothing applied yet: no caveat.
  const fresh = board({ ...review, suggestions: [note({ id: 1, check })] }, { runCreatedAt });
  assert.equal(fresh.columns[1].cards[0].check?.basis, undefined);
});

test("the banner offers a re-run for applied changes, then a review of the run that has them", () => {
  const base = { reviewRunId: 7, latestRunId: 7, planChanged: false, running: false, applied: 0, pending: 0 };
  assert.equal(reviewBanner(base), undefined);
  assert.deepEqual(reviewBanner({ ...base, planChanged: true, applied: 2, pending: 2 }), {
    text: "2 changes applied · Re-run to see the combined effect",
    action: "rerun",
  });
  assert.deepEqual(reviewBanner({ ...base, planChanged: true, applied: 1, pending: 1 }), {
    text: "1 change applied · Re-run to see the combined effect",
    action: "rerun",
  });
  // Edits made elsewhere read as such, and still offer the run.
  const edited = reviewBanner({ ...base, planChanged: true });
  assert.match(edited?.text ?? "", /plan changed/);
  assert.equal(edited?.action, "rerun");
  // While the run goes, no action: the Run is already under way.
  assert.deepEqual(reviewBanner({ ...base, planChanged: true, running: true, applied: 2, pending: 2 }), {
    text: "Running the plan with 2 applied changes…",
  });
  // The run finished: a fresh review is the next step, not an automatic one.
  const latestAt = "2026-09-27 12:31:00";
  assert.deepEqual(reviewBanner({ ...base, latestRunId: 8, applied: 2, latestAt }), {
    text: `The ${clockTime(latestAt)} run includes 2 applied changes. Review again to check it.`,
    action: "review",
  });
  assert.deepEqual(reviewBanner({ ...base, latestRunId: 8 }), {
    text: "A newer run has finished since this review. Review again to check it.",
    action: "review",
  });
});

test("a stale refusal after another applied note reads as the two touching the same field", () => {
  assert.equal(refusalHeading({ stale: true, conflict: true }), "An earlier change touched the same field.");
  assert.equal(refusalHeading({ stale: true }), "The plan changed since this note was written.");
  assert.equal(refusalHeading({}), "These changes no longer fit the plan.");
});

test("problems read as sentences, and are found wherever the error body carries them", () => {
  const stale = { kind: "stale" as const, change: 0, path: "/effects/0/amount", expected: 1, actual: 2 };
  assert.equal(problemText(stale), "/effects/0/amount changed since the note was written.");
  assert.equal(problemText({ kind: "unknown_target", change: 0, target: { account: 3 } }),
    "The account this note edits no longer exists.");
  assert.equal(problemText({ kind: "invalid_body", change: 0, target: { new_event: "a" }, message: "name taken" }),
    "The edited event would not be valid: name taken.");
  assert.deepEqual(problemsIn({ problems: [stale] }), [stale]);
  assert.deepEqual(problemsIn({ error: { code: "stale", message: "", problems: [stale] } }), [stale]);
  assert.deepEqual(problemsIn({ error: { code: "conflict", message: "" } }), []);
  assert.deepEqual(problemsIn(undefined), []);
});

test("note counts are per row, once per note, open notes only", () => {
  const counts = noteCounts([
    note({ id: 1, changes: [removeSweep, { ...removeSweep, path: "/effects/1" }] }),
    // Two paths (or two steps) editing the same row count once.
    note({
      id: 6,
      paths: [
        path({ key: "a", steps: [step({ changes: [{ op: "replace", target: { asset: 2 }, path: "/name", value: "x" }] })] }),
        path({
          key: "b",
          steps: [
            step({ key: "b1", changes: [{ op: "replace", target: { asset: 2 }, path: "/description", value: "y" }] }),
            step({ key: "b2", changes: [{ op: "replace", target: { asset: 2 }, path: "/initial_price", value: 1 }] }),
          ],
        }),
      ],
    }),
    note({ id: 2, changes: [{ op: "replace", target: { asset: 2 }, path: "/return_profile_id", value: 5 }] }),
    note({ id: 3, changes: [{ op: "replace", target: { event: 5 }, path: "/name", value: "x" }] }),
    note({ id: 4, status: "applied", applied_path: "a", changes: [{ op: "replace", target: { account: 6 }, path: "/cash_value", value: 1 }] }),
    note({ id: 5, changes: [{ op: "add", target: { new_event: "k" }, path: "", value: {} }] }),
  ]);
  assert.deepEqual([...counts.events], [[5, 2]]);
  assert.deepEqual([...counts.assets], [[2, 2]]);
  assert.deepEqual([...counts.accounts], []);
  assert.equal(noteLink(1), "1 review note →");
  assert.equal(noteLink(2), "2 review notes →");
  assert.equal(noteLink(0), undefined);
});

test("the Review tab is a path of its own, with no sub-tab", async () => {
  const { parseNav, toHref } = await import("../lib/nav/url.ts");
  assert.deepEqual(parseNav("/review", "?scenario=s1&sec=x"), {
    scenario: "s1", tab: "review", section: undefined, selection: undefined,
  });
  assert.equal(toHref({ scenario: "s1", tab: "review" }), "/review?scenario=s1");
});

test("the model pass reads as running, failed or stopped early, and says nothing otherwise", () => {
  const ai = (status: "running" | "done" | "failed", error: string | null = null) => ({
    ai: { status, stop: null, error, started_at: "2026-09-27 12:10:00", finished_at: null },
  });
  assert.equal(aiLine({ ai: null }), undefined);
  assert.deepEqual(aiLine(ai("running")), {
    text: "AI review in progress… rule notes shown below",
    running: true,
  });
  const failed = aiLine(ai("failed", "the review model could not be reached"));
  assert.equal(failed?.running, false);
  assert.match(failed?.text ?? "", /did not finish: the review model could not be reached/);
  assert.match(failed?.text ?? "", /review again/);
  assert.doesNotMatch(failed?.text ?? "", /Claude/, "the provider may not be Anthropic");
  assert.equal(aiLine(ai("done")), undefined);
  assert.match(aiLine(ai("done", "the review model stopped partway through"))?.text ?? "", /stopped early/);
});

// ── Paths of steps ───────────────────────────────────────────────────────────

const retireLater = path({
  key: "retire-later",
  label: "Retire later",
  reasoning: "Two more salary years, and a buffer for the first bad year.",
  recommended: true,
  steps: [
    step({
      key: "retire-42",
      title: "Retire at 42",
      reasoning: "Two more salary years cover the gap before spending steps down.",
      changes: [{ op: "replace", target: { event: 3 }, path: "/trigger/years", expect: 40, value: 42 }],
      diff: [{ label: "Plan › Retirement › trigger › age", from: "40", to: "42" }],
    }),
    step({
      key: "cash-floor",
      title: "Raise cash floor to 1y",
      changes: [{ op: "replace", target: { event: 6 }, path: "/effects/0/amount/inner/value", expect: 20000, value: 172000 }],
      diff: [{ label: "Plan › Sweep › effects › Sweep › amount", from: "$20,000", to: "$172,000" }],
    }),
  ],
  check: { iterations: 2000, paired: true, base: stats(0.909, 0.906), edited: stats(0.97, 0.961) },
});
const spendLess = path({
  key: "spend-less",
  label: "Spend $7,000 a month",
  steps: [
    step({
      key: "spend-7k",
      title: "Spend $7,000 a month",
      changes: [{ op: "replace", target: { event: 2 }, path: "/effects/0/amount/inner/value", expect: 8000, value: 7000 }],
      diff: [{ label: "Plan › Expenses › effects › Expense › amount", from: "$8,000", to: "$7,000" }],
    }),
  ],
  estimate: { success_rate: null, funding_success_rate: 0.94 },
});
const brokerage = path({
  key: "brokerage",
  label: "Invest idle cash",
  steps: [
    step({
      key: "open",
      title: "Open a brokerage",
      changes: [{ op: "add", target: { new_account: "brokerage" }, path: "", value: {} }],
      diff: [{ label: "Portfolio › + Brokerage", from: null, to: "Taxable investment · $50,000 cash · 2 lots" }],
    }),
  ],
});
const choices = (overrides: Record<string, unknown> = {}) =>
  note({ id: 9, kind: "check", section: "results", rule: null, source: "ai", paths: [spendLess, retireLater, brokerage], ...overrides });

/** `choices()` with Retire later started: its first `done` steps applied. */
function started(done: number, overrides: Record<string, unknown> = {}) {
  const steps = retireLater.steps.map((s, i) => ({ ...s, applied: i < done }));
  return choices({ paths: [spendLess, { ...retireLater, steps }, brokerage], applied_path: "retire-later", ...overrides });
}

test("a note with several paths defaults to the recommended one, else the first", () => {
  assert.equal(selectedPath(choices())?.key, "retire-later");
  assert.equal(selectedPath(note({ paths: [spendLess, brokerage] }))?.key, "spend-less");
  assert.equal(selectedPath(choices(), "brokerage")?.key, "brokerage");
  // A pick the note no longer has falls back to the default.
  assert.equal(selectedPath(choices(), "gone")?.key, "retire-later");
  assert.equal(selectedPath(note({ changes: [] })), undefined);
  // Once a step is applied, that path is the one shown, whatever was picked.
  assert.equal(selectedPath(started(1), "brokerage")?.key, "retire-later");
});

test("the picker lists each path with its badge, step count and combined result", () => {
  const card = toCard(choices());
  assert.deepEqual(card.paths, [
    { key: "spend-less", label: "Spend $7,000 a month", recommended: false, headline: "est. funding ~94.0%", steps: undefined, selected: false, locked: false },
    { key: "retire-later", label: "Retire later", recommended: true, headline: "funding 96.1% · simulated", steps: "2 steps", selected: true, locked: false },
    { key: "brokerage", label: "Invest idle cash", recommended: false, headline: undefined, steps: undefined, selected: false, locked: false },
  ]);
  assert.equal(card.path, "retire-later");
  assert.equal(card.pathLabel, "Retire later");
  assert.equal(card.pathReasoning, "Two more salary years, and a buffer for the first bad year.");
  // One check over the whole path.
  assert.equal(card.check?.text, "funding 90.6% → 96.1% · simulated, 2,000 iterations · all steps");
});

test("a path of several steps lists them in order: the first applyable, later ones after it", () => {
  const card = toCard(choices());
  assert.deepEqual(card.diff, [], "each step carries its own diff");
  assert.deepEqual(
    card.steps.map(({ number, title, applied, apply, after, status }) => ({ number, title, applied, apply, after, status })),
    [
      { number: 1, title: "Retire at 42", applied: false, apply: true, after: undefined, status: undefined },
      { number: 2, title: "Raise cash floor to 1y", applied: false, apply: false, after: 1, status: undefined },
    ],
  );
  assert.equal(card.steps[0].reasoning, "Two more salary years cover the gap before spending steps down.");
  assert.deepEqual(card.steps[1].diff.map((r) => r.label), ["Plan › Sweep › effects › Sweep › amount"]);
  // The whole path applies at once too; already simulated, so no preview.
  assert.deepEqual(card.actions, ["apply-path", "confirm", "dismiss"]);
  assert.equal(ACTION_LABEL["apply-path"], "Apply path");
});

test("picking a single-step path shows its diff and plain Apply, no step list", () => {
  const card = toCard(choices(), undefined, { selected: "spend-less" });
  assert.equal(card.path, "spend-less");
  assert.deepEqual(card.steps, []);
  assert.deepEqual(card.diff.map((r) => r.label), ["Plan › Expenses › effects › Expense › amount"]);
  assert.equal(card.check?.text, "est. funding ~94.0% · preview to simulate");
  assert.deepEqual(card.actions, ["apply", "confirm", "preview", "dismiss"]);
  assert.equal(card.pathReasoning, undefined, "this path gives no reasoning of its own");
});

test("a lone path of several steps names itself but shows no picker", () => {
  const card = toCard(note({ paths: [retireLater] }));
  assert.deepEqual(card.paths, []);
  assert.equal(card.pathLabel, "Retire later");
  assert.equal(card.steps.length, 2);
  assert.deepEqual(card.actions, ["apply-path", "dismiss"]);
});

test("once a step is applied, its path is locked in, the others close, and the next step is up", () => {
  const latest = { id: 7, created_at: "2026-09-27 12:04:00" };
  const card = toCard(started(1), undefined, { latest, selected: "brokerage" });
  assert.equal(card.path, "retire-later");
  assert.deepEqual(card.paths.map((p) => [p.key, p.selected, p.locked]), [
    ["spend-less", false, true],
    ["retire-later", true, false],
    ["brokerage", false, true],
  ]);
  assert.match(card.locked ?? "", /other paths are closed/);
  assert.equal(card.applied, "Started: Retire later · 1 of 2 steps applied");
  const [first, second] = card.steps;
  assert.equal(first.status, "Applied · not yet run");
  assert.equal(first.apply, false);
  assert.equal(second.apply, true);
  assert.equal(second.after, undefined);
  // Committed to the path: carry on, but no confirming, dismissing or re-previewing it.
  assert.deepEqual(card.actions, ["apply-path"]);
});

test("a part-applied step's own applied_at says whether a later run includes it", () => {
  const stamped = (at: string) => {
    const steps = retireLater.steps.map((s, i) => ({ ...s, applied: i < 1, applied_at: i < 1 ? at : null }));
    return choices({ paths: [spendLess, { ...retireLater, steps }, brokerage], applied_path: "retire-later" });
  };
  const latest = { id: 8, created_at: "2026-09-27 12:31:00" };
  // Applied before the 12:31 run: that run includes it.
  assert.equal(toCard(stamped("2026-09-27 12:20:00"), undefined, { latest }).steps[0].status, `Applied · in the ${clockTime(latest.created_at)} run`);
  // Applied after it: still waiting for a run.
  assert.equal(toCard(stamped("2026-09-27 12:40:00"), undefined, { latest }).steps[0].status, "Applied · not yet run");
  // No stamp (older notes) and a newer run: not claimed either way.
  assert.equal(toCard(started(1), undefined, { latest }).steps[0].status, "Applied");
});

test("the header counts applied steps, and a fully applied path names itself", () => {
  const latest = { id: 7, created_at: "2026-09-27 12:04:00" };
  const review = { run_id: 7, reviewed_at: "2026-09-27 12:10:00", ai: null, suggestions: [started(1), note({ id: 2 })] };
  const view = board(review, { runCreatedAt: latest.created_at, latest });
  assert.equal(view.applied, 1);
  assert.equal(view.pending, 1);
  assert.equal(view.headline, `Reviewed the ${clockTime(latest.created_at)} run · 1 note · 1 applied`);
  const done = started(2, { status: "applied", resolved_at: "2026-09-27 12:20:00" });
  const full = board({ ...review, suggestions: [done] }, { latest });
  assert.equal(full.applied, 2);
  assert.equal(full.pending, 2);
  assert.equal(appliedLabel(done, latest), "Applied: Retire later · not yet run");
  const after = { id: 8, created_at: "2026-09-27 12:31:00" };
  assert.equal(appliedLabel(done, after), `Applied: Retire later · in the ${clockTime(after.created_at)} run`);
  assert.equal(board({ ...review, suggestions: [done] }, { latest: after }).pending, 0);
  // A copy is not this plan: stress paths count no applied steps here.
  const stress = started(2, { kind: "stress", status: "applied", resolved_at: "2026-09-27 12:20:00" });
  assert.equal(board({ ...review, suggestions: [stress] }, { latest }).applied, 0);
  assert.equal(appliedLabel(stress), "Added as a scenario: Retire later");
});

test("a step applied part-way is not claimed in or out of a newer run", () => {
  // Steps carry no time: a run after the review may or may not include step 1.
  const card = toCard(started(1), undefined, { latest: { id: 8, created_at: "2026-09-27 12:31:00" } });
  assert.equal(card.steps[0].status, "Applied");
  // Against the reviewed run itself, it cannot be in any run yet.
  assert.equal(toCard(started(1), undefined, { latest: { id: 7, created_at: "2026-09-27 12:04:00" } }).steps[0].status,
    "Applied · not yet run");
});

test("stress paths go to a copy whole: no step buttons, and not once part-applied", () => {
  const card = toCard(choices({ kind: "stress" }));
  assert.deepEqual(card.actions, ["apply-copy", "dismiss"]);
  assert.ok(card.steps.every((s) => !s.apply && s.after == null));
});

test("the board keeps each card's pick, and the caveat follows the picked path", () => {
  const review = {
    run_id: 7,
    reviewed_at: "2026-09-27 12:10:00",
    ai: null,
    suggestions: [choices(), appliedNote({ id: 2, resolved_at: "2026-09-27 12:20:00" })],
  };
  const runCreatedAt = "2026-09-27 12:04:00";
  const cards = board(review, { runCreatedAt, selections: { 9: "spend-less" } }).columns[2].cards;
  assert.equal(cards[0].path, "spend-less");
  assert.equal(cards[0].check?.basis, `simulated against the ${clockTime(runCreatedAt)} run, before the applied changes`);
});

test("path headlines quote the combined simulated result, else the estimate, else nothing", () => {
  assert.equal(pathHeadline(retireLater), "funding 96.1% · simulated");
  assert.equal(pathHeadline(spendLess), "est. funding ~94.0%");
  assert.equal(pathHeadline(path({ estimate: { success_rate: 0.93, funding_success_rate: null } })), "est. success ~93.0%");
  assert.equal(pathHeadline(path()), undefined);
  assert.equal(checkLine({ ...spendLess, steps: retireLater.steps })?.text, "est. funding ~94.0% · all steps · preview to simulate");
});

test("a preview of a path of several steps says it covers them all", () => {
  const preview = { base_run_id: 7, iterations: 2000, paired: true, diff: [], problems: [], base: stats(0.9, 0.9), edited: stats(0.9, 0.95) };
  assert.equal(previewLine(preview, 2)?.text, "funding 90.0% → 95.0% · simulated, 2,000 iterations · all steps");
  assert.equal(previewLine(preview)?.text, "funding 90.0% → 95.0% · simulated, 2,000 iterations");
});

test("diff rows say whether they add, remove or edit", () => {
  assert.deepEqual(diffRow({ label: "Portfolio › + Brokerage", from: null, to: "Taxable investment · $50,000 cash · 2 lots" }), {
    label: "Portfolio › + Brokerage",
    from: undefined,
    to: "Taxable investment · $50,000 cash · 2 lots",
    change: "add",
  });
  assert.equal(diffRow({ label: "x", from: "Sweep", to: null }).change, "remove");
  assert.equal(diffRow({ label: "x", from: "$8,000", to: "$7,000" }).change, "edit");
  const card = toCard(choices(), undefined, { selected: "brokerage" });
  assert.deepEqual(card.diff.map((r) => r.change), ["add"]);
});

test("problems are narrowed to the path acted on, and name their step", () => {
  const stale = { kind: "stale" as const, change: 0, path: "/trigger/years", expected: 40, actual: 41 };
  const bad = { kind: "bad_path" as const, change: 0, path: "/effects/3", reason: "no such effect" };
  // The server's envelope: every problem, and the same grouped by path and step.
  const body = {
    error: { code: "stale", message: "the plan changed" },
    problems: [stale, bad],
    by_step: [
      { path: "retire-later", step: "cash-floor", problems: [stale] },
      { path: "spend-less", step: "spend-7k", problems: [bad] },
    ],
  };
  assert.deepEqual(stepProblemsIn(body, "retire-later"), [{ problem: stale, step: "cash-floor" }]);
  assert.deepEqual(problemsIn(body, "spend-less"), [bad]);
  assert.deepEqual(problemsIn(body), [stale, bad]);
  // Without the grouping every problem shows, unattributed.
  assert.deepEqual(stepProblemsIn({ problems: [stale, bad] }, "retire-later"), [{ problem: stale }, { problem: bad }]);
  // Named by step when the path has several.
  assert.equal(stepProblemText({ problem: stale, step: "cash-floor" }, retireLater.steps),
    "Step 2: /trigger/years changed since the note was written.");
  assert.equal(stepProblemText({ problem: stale, step: "spend-7k" }, spendLess.steps),
    "/trigger/years changed since the note was written.");
});

test("problems about new items and their references read as sentences", () => {
  assert.equal(problemText({ kind: "duplicate_key", change: 1, key: "brokerage" }), 'Two new items share the name "brokerage".');
  assert.equal(problemText({ kind: "unknown_reference", change: 2, key: "vti" }),
    'A change refers to "vti", which nothing in this note creates.');
  assert.equal(
    problemText({ kind: "wrong_reference_kind", change: 2, key: "brokerage", field: "asset_id", expected: "asset", found: "account" }),
    '"brokerage" is an account, but asset_id needs an asset.',
  );
  assert.equal(problemText({ kind: "reference_cycle", change: 0, keys: ["a", "b", "a"] }),
    "New items refer to each other in a loop: a → b → a.");
});

test("problems name created rows by what they create", () => {
  assert.equal(problemText({ kind: "unknown_target", change: 0, target: { new_account: "brokerage" } }),
    "The account this note edits no longer exists.");
  assert.equal(problemText({ kind: "invalid_body", change: 0, target: { new_asset: "vti" }, message: "price must be positive" }),
    "The edited asset would not be valid: price must be positive.");
});
