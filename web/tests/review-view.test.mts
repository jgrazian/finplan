import assert from "node:assert/strict";
import { test } from "node:test";
import type { AiStep } from "../lib/api/generated/AiStep.ts";
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
  citedMetrics,
  closedText,
  deltaLine,
  detailActions,
  diffRow,
  evidenceLink,
  leadOf,
  listStatus,
  kicker,
  noteCounts,
  noteLink,
  noteList,
  pathHeadline,
  pathMetrics,
  pickedNote,
  previewLine,
  problemText,
  problemsIn,
  refusalHeading,
  reviewBanner,
  reviewChecks,
  reviewedLine,
  selectedPath,
  shortDate,
  splitReasoning,
  steppedNote,
  stepProblemText,
  stepProblemsIn,
  toCard,
  undoable,
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
    summary: null as string | null,
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
  // A plan's notes are set aside with Dismiss alone; "It's correct" is a draft's.
  assert.deepEqual(acts(note({ kind: "check", changes: [] })), ["dismiss"]);
  assert.deepEqual(acts(note({ kind: "check" })), ["apply", "preview", "dismiss"]);
  assert.deepEqual(actionsFor(note({ kind: "check", changes: [] }) as never, undefined, { confirm: true }), [
    "confirm",
    "dismiss",
  ]);
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

test("the header says when the review was written, as a local date and time", () => {
  const at = "2026-10-02 13:40:00";
  assert.equal(reviewedLine({ reviewed_at: at, run_id: 7 }), `Last reviewed ${new Date(`2026-10-02T13:40:00Z`).toLocaleDateString("en-US", { month: "short", day: "numeric" })} at ${clockTime(at)}`);
  assert.match(reviewedLine({ reviewed_at: "2026-10-02T12:00:00Z", run_id: 7 }), /^Last reviewed (Oct 1|Oct 2|Oct 3) at \d\d:\d\d$/);
  assert.equal(reviewedLine({ reviewed_at: "garbage", run_id: 7 }), "Last reviewed run #7");
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
  assert.equal(view.headline, `${reviewedLine(review)} · 4 notes`);
  assert.equal(view.open, 4);
  assert.equal(view.applied, 0);
  assert.equal(view.resolved, 1);
  assert.deepEqual(view.columns.map((c) => c.heading), ["Portfolio", "Scenario & events", "Results"]);
  assert.deepEqual(view.columns.map((c) => c.cards.map((card) => card.id)), [[4, 2], [3], [1]]);
});

test("a review of an unknown run time falls back to the review's own time, then the run id", () => {
  const review = { run_id: 7, reviewed_at: "2026-09-27 12:10:00", ai: null, suggestions: [note()] };
  assert.equal(board(review).headline, `Last reviewed Sep 27 at ${clockTime(review.reviewed_at)} · 1 note`);
  assert.equal(board({ ...review, reviewed_at: "" }).headline, "Last reviewed run #7 · 1 note");
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
  // The stress note went to a copy: it is not a change waiting on this plan.
  assert.equal(view.headline, `${reviewedLine(review)} · 1 note · 2 applied`);
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

test("the Review tab opens on Notes, and Chat is a sub-tab in the query", async () => {
  const { parseNav, toHref } = await import("../lib/nav/url.ts");
  assert.deepEqual(parseNav("/review", "?scenario=s1"), {
    scenario: "s1", tab: "review", section: "notes", selection: undefined,
  });
  assert.deepEqual(parseNav("/review", "?scenario=s1&sec=chat"), {
    scenario: "s1", tab: "review", section: "chat", selection: undefined,
  });
  // The default is left out of the URL.
  assert.equal(toHref({ scenario: "s1", tab: "review", section: "notes" }), "/review?scenario=s1");
  assert.equal(toHref({ scenario: "s1", tab: "review", section: "chat" }), "/review?scenario=s1&sec=chat");
});

test("the model pass reads as running, failed or stopped early, and says nothing otherwise", () => {
  const ai = (status: "running" | "done" | "failed", error: string | null = null) => ({
    ai: { status, stop: null, error, started_at: "2026-09-27 12:10:00", finished_at: null, activity: [] },
  });
  assert.equal(aiLine({ ai: null }), undefined);
  assert.deepEqual(aiLine(ai("running")), { text: "AI review in progress…", running: true });
  const failed = aiLine(ai("failed", "the review model could not be reached"));
  assert.equal(failed?.running, false);
  assert.match(failed?.text ?? "", /did not finish: the review model could not be reached/);
  assert.match(failed?.text ?? "", /review again/);
  assert.doesNotMatch(failed?.text ?? "", /Claude/, "the provider may not be Anthropic");
  assert.equal(aiLine(ai("done")), undefined);
  assert.match(aiLine(ai("done", "the review model stopped partway through"))?.text ?? "", /stopped early/);
});

test("a running model pass names the last tool it called", () => {
  const running = (activity: AiStep[]) => ({
    ai: { status: "running" as const, stop: null, error: null, started_at: "", finished_at: null, activity },
  });
  // Nothing called yet, or only thinking: the generic line.
  assert.equal(aiLine(running([{ kind: "thinking", done: false }]))?.text, "AI review in progress…");
  // A call in progress reads as under way...
  assert.equal(
    aiLine(
      running([
        { kind: "tool", name: "preview_changes", done: true, failed: false },
        { kind: "narration", text: "Checking the taxes next." },
        { kind: "tool", name: "estimate_taxes", done: false, failed: false },
      ]),
    )?.text,
    "AI review: Estimating taxes…",
  );
  // ...and stays named while the model thinks about its result.
  assert.equal(
    aiLine(
      running([
        { kind: "tool", name: "goal_seek", done: true, failed: false },
        { kind: "thinking", done: false },
      ]),
    )?.text,
    "AI review: Searching for the amount that reaches the goal",
  );
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
  assert.deepEqual(card.actions, ["apply-path", "dismiss"]);
  assert.equal(ACTION_LABEL["apply-path"], "Apply path");
});

test("picking a single-step path shows its diff and plain Apply, no step list", () => {
  const card = toCard(choices(), undefined, { selected: "spend-less" });
  assert.equal(card.path, "spend-less");
  assert.deepEqual(card.steps, []);
  assert.deepEqual(card.diff.map((r) => r.label), ["Plan › Expenses › effects › Expense › amount"]);
  assert.equal(card.check?.text, "est. funding ~94.0% · preview to simulate");
  assert.deepEqual(card.actions, ["apply", "preview", "dismiss"]);
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
  assert.equal(view.headline, `${reviewedLine(review)} · 1 note · 1 applied`);
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

// ── The note list and detail ─────────────────────────────────────────────────

test("long reasoning leads with its first sentence; figures with points stay whole", () => {
  assert.deepEqual(splitReasoning("  Short and whole.  "), { summary: "Short and whole." });
  const lead = "VFIAX is 1464.6 × $709 = $1.04M, dated at the plan's start with a cost basis of $1.04M too.";
  const rest = "If those lots were bought earlier for less, every sale is taxed too lightly. ".repeat(3).trim();
  assert.deepEqual(splitReasoning(`${lead} ${rest}`), { summary: lead, more: rest });
  // Not after an abbreviation, nor inside brackets.
  const goog =
    "GOOG (Alphabet Inc. Class C) is one company, but it is modelled on US Total Market with no tracking error.";
  const tail = "A single stock swings well beyond its index; a tracking error models that spread. ".repeat(2).trim();
  assert.deepEqual(splitReasoning(`${goog} ${tail}`), { summary: goog, more: tail });
  // No sentence break to cut at: whole.
  const run = "x".repeat(300);
  assert.deepEqual(splitReasoning(run), { summary: run });
});

test("a checked path reads as from → to figures, an estimate as its rates alone", () => {
  const base = { ...stats(0.9785, 0.9575), real_final: { p5: 2_080_000, p10: 0, p25: 0, p50: 0, p75: 0, p90: 0, p95: 0 } };
  const edited = { ...stats(0.986, 0.961), real_final: { ...base.real_final, p5: 2_840_000 } };
  const checked = { check: { iterations: 400, paired: true, base, edited }, estimate: null };
  assert.equal(deltaLine(checked), "funding 95.8% → 96.1%");
  assert.deepEqual(pathMetrics(checked as never), [
    { label: "Funding", from: "95.8%", to: "96.1%" },
    { label: "Success", from: "97.9%", to: "98.6%" },
    { label: "P5 real final", from: "$2.08M", to: "$2.84M" },
  ]);
  // A Roth conversion's check: the after-tax ending balance and lifetime tax.
  const converted = {
    check: {
      iterations: 400,
      paired: true,
      base: { ...base, after_tax_final: 6_100_000, lifetime_taxes: 2_400_000 },
      edited: { ...edited, after_tax_final: 6_650_000, lifetime_taxes: 1_900_000 },
    },
    estimate: null,
  };
  assert.deepEqual(pathMetrics(converted as never)?.slice(3), [
    { label: "After-tax ending", from: "$6.10M", to: "$6.65M" },
    { label: "Lifetime tax", from: "$2.40M", to: "$1.90M" },
  ]);
  const estimated = { check: null, estimate: { success_rate: null, funding_success_rate: 0.93 } };
  assert.equal(deltaLine(estimated), "est. funding ~93.0%");
  assert.deepEqual(pathMetrics(estimated), [{ label: "Funding", to: "~93.0%", estimate: true }]);
  assert.equal(pathMetrics({ check: null, estimate: null }), undefined);
});

test("a note with nothing to measure cites its stats as figures", () => {
  const evidence = [
    { ref: "ledger", year: 2033, event_id: null, account_id: null },
    { ref: "stat", name: "taxable_value", value: 2_250_000 },
    { ref: "diagnostic", field: "funding_success_rate", value: 0.953 },
  ];
  assert.deepEqual(citedMetrics(evidence as never), [
    { label: "Taxable value", to: "$2.25M" },
    { label: "Funding success rate", to: "95.3%" },
  ]);
  const card = toCard(note({ kind: "read", changes: [], evidence }) as never);
  assert.equal(card.delta, "Taxable value $2.25M");
  assert.equal(card.metrics.length, 2);
});

test("the detail's primary is the apply, else Dismiss (Got it on a read note)", () => {
  assert.deepEqual(detailActions({ kind: "fix", actions: ["apply", "preview", "dismiss"] }), {
    primary: { action: "apply", label: "Apply" },
    secondary: [{ action: "preview", label: "Preview" }],
    dismiss: true,
  });
  // Nothing to apply: Dismiss is the primary.
  assert.deepEqual(detailActions({ kind: "check", actions: ["dismiss"] }), {
    primary: { action: "dismiss", label: "Dismiss" },
    secondary: [],
    dismiss: false,
  });
  assert.deepEqual(detailActions({ kind: "read", actions: ["dismiss"] }), {
    primary: { action: "dismiss", label: "Got it" },
    secondary: [],
    dismiss: false,
  });
  assert.deepEqual(detailActions({ kind: "fix", actions: [] }), { primary: undefined, secondary: [], dismiss: false });
});

test("a closed note says where it stands; only one set aside can be undone", () => {
  assert.equal(closedText({ kind: "fix", status: "open" }), undefined);
  assert.equal(closedText({ kind: "check", status: "confirmed" }), "Marked as correct.");
  assert.equal(closedText({ kind: "read", status: "dismissed" }), "Marked as read.");
  assert.equal(closedText({ kind: "fix", status: "dismissed" }), "Dismissed.");
  assert.equal(closedText({ kind: "fix", status: "applied", applied: "Applied · not yet run" }), "Applied · not yet run");
  assert.equal(undoable({ status: "dismissed" }), true);
  assert.equal(undoable({ status: "confirmed" }), true);
  assert.equal(undoable({ status: "applied" }), false);
  assert.equal(undoable({ status: "open" }), false);
});

test("notes applied in an earlier review stay listed as handled but are not counted as changes awaiting a run", () => {
  const applied = {
    status: "applied",
    applied_path: "a",
    resolved_at: "2026-09-20 10:00:00",
    paths: [path({ steps: [step({ applied: true, applied_at: "2026-09-20 10:00:00" })] })],
  };
  const review = {
    run_id: 9,
    reviewed_at: "2026-09-27 12:10:00",
    ai: null,
    suggestions: [note({ id: 1, run_id: 3, ...applied }), note({ id: 2, run_id: 9 })],
  };
  const view = board(review as never);
  assert.equal(view.applied, 0);
  assert.equal(view.pending, 0);
  assert.doesNotMatch(view.headline, /applied/);
  assert.deepEqual(noteList(view, { show: "handled" }).visible.map((c) => c.id), [1]);
  // The same note applied since this review counts.
  const now = board({ ...review, suggestions: [note({ id: 1, run_id: 9, ...applied })] } as never);
  assert.equal(now.applied, 1);
});

test("the list opens on the open notes; handled ones are a view of their own, with every note under All", () => {
  const review = {
    run_id: 7,
    reviewed_at: "2026-09-27 12:10:00",
    ai: null,
    suggestions: [
      note({ id: 1, kind: "read", section: "results", changes: [] }),
      note({ id: 2, kind: "check", section: "portfolio", changes: [], status: "dismissed" }),
      note({ id: 3, kind: "fix", section: "plan" }),
      note({ id: 4, kind: "fix", section: "portfolio" }),
      note({ id: 5, kind: "fix", section: "plan", status: "applied" }),
    ],
  };
  const view = board(review as never);
  assert.deepEqual(view.setAside.map((c) => c.id), [2]);

  const open = noteList(view);
  assert.deepEqual(open.views.map((v) => [v.label, v.count]), [["Open", 3], ["Handled", 2], ["All", 5]]);
  assert.deepEqual(open.visible.map((c) => c.id), [4, 3, 1]);
  assert.deepEqual(open.groups.map((g) => g.heading), ["Portfolio", "Scenario & events", "Results"]);
  assert.equal(open.empty, undefined);
  assert.equal(open.all.length, 5, "the position counts over every note");

  // Handled notes with no date to sort by go newest id first.
  const done = noteList(view, { show: "handled" });
  assert.deepEqual(done.visible.map((c) => c.id), [5, 2]);
  const [applied, dismissed] = done.groups[0].rows;
  assert.deepEqual([dismissed.closed, dismissed.status], [true, { label: "Dismissed", tone: "dismissed" }]);
  assert.deepEqual(applied.status, { label: "✓ Applied", tone: "done" });

  assert.deepEqual(noteList(view, { show: "all" }).visible.map((c) => c.id), [4, 2, 3, 5, 1]);
});

test("the Handled view is one plain list by when notes were handled, newest first", () => {
  const ago = (days: number, hour = 12) => {
    const d = new Date();
    d.setDate(d.getDate() - days);
    d.setHours(hour, 0, 0, 0);
    return d.toISOString();
  };
  const review = {
    run_id: 7,
    reviewed_at: "",
    ai: null,
    suggestions: [
      note({ id: 1, section: "results", status: "dismissed", resolved_at: ago(5) }),
      note({ id: 2, section: "portfolio", status: "dismissed", resolved_at: ago(0, 9) }),
      note({ id: 3, section: "plan", status: "applied", resolved_at: ago(1) }),
      note({ id: 4, section: "portfolio", status: "confirmed", resolved_at: ago(0, 11) }),
      note({ id: 5, section: "plan", status: "dismissed", resolved_at: null }),
      note({ id: 6, section: "plan" }),
    ],
  };
  const list = noteList(board(review as never), { show: "handled", keep: 6 });
  assert.equal(list.groups.length, 1);
  assert.equal(list.groups[0].heading, undefined, "no headings");
  // The note taken back while in view heads it; the undated one closes it.
  assert.deepEqual(list.visible.map((c) => c.id), [6, 4, 2, 3, 1, 5]);
  // Open and All keep their sections.
  assert.deepEqual(noteList(board(review as never), { show: "all" }).groups.map((g) => g.heading), [
    "Portfolio",
    "Scenario & events",
    "Results",
  ]);
});

test("kind chips narrow the view, counted within it; with none on, every kind shows", () => {
  const review = {
    run_id: 7,
    reviewed_at: "2026-09-27 12:10:00",
    ai: null,
    suggestions: [
      note({ id: 1, kind: "read", section: "results", changes: [] }),
      note({ id: 2, kind: "check", section: "portfolio", changes: [] }),
      note({ id: 3, kind: "fix", section: "plan" }),
      note({ id: 4, kind: "stress", section: "results", status: "applied" }),
    ],
  };
  const view = board(review as never);
  const list = noteList(view, { kinds: ["fix", "read"] });
  assert.deepEqual(list.kinds.map((k) => [k.label, k.count, k.on]), [
    ["Fix", 1, true],
    ["Stress", 0, false],
    ["Check", 1, false],
    ["Read", 1, true],
  ]);
  assert.deepEqual(list.visible.map((c) => c.id), [3, 1]);
  assert.equal(noteList(view, { kinds: ["stress"] }).empty, "No notes match these filters.");
  assert.equal(noteList(view, { show: "handled", kinds: ["stress"] }).visible[0].id, 4);
});

test("the note just acted on stays in the view until the reader moves on; empty views say why", () => {
  const review = { run_id: 7, reviewed_at: "", ai: null, suggestions: [note({ id: 1, status: "dismissed" })] };
  const view = board(review as never);
  assert.equal(noteList(view).empty, "Nothing left to act on.");
  assert.deepEqual(noteList(view, { keep: 1 }).visible.map((c) => c.id), [1]);
  const none = board({ ...review, suggestions: [note({ id: 2 })] } as never);
  assert.equal(noteList(none, { show: "handled" }).empty, "Nothing dismissed or applied yet.");
});

test("a handled note says when: a day on its pill, a day and time in the detail", () => {
  const at = "2026-10-02 13:40:00";
  const day = shortDate(at);
  const when = `${day} at ${clockTime(at)}`;
  assert.match(day ?? "", /^Oct [123]$/);
  assert.deepEqual(listStatus({ kind: "fix", status: "dismissed", resolvedAt: at }), {
    label: "Dismissed",
    tone: "dismissed",
    date: day,
  });
  assert.equal(closedText({ kind: "fix", status: "dismissed", resolvedAt: at }), `Dismissed on ${when}.`);
  assert.equal(closedText({ kind: "check", status: "confirmed", resolvedAt: at }), `Marked as correct on ${when}.`);
  assert.equal(
    closedText({ kind: "fix", status: "applied", applied: "Applied: Retire later · not yet run", resolvedAt: at }),
    `Applied: Retire later on ${when} · not yet run`,
  );
  assert.equal(closedText({ kind: "stress", status: "applied", resolvedAt: at }), `Added as a scenario on ${when}.`);
  // The card carries it once the note is handled, and not while it is open.
  assert.equal(toCard(note({ status: "dismissed", resolved_at: at }) as never).resolvedAt, at);
  assert.equal(toCard(note({ resolved_at: at }) as never).resolvedAt, undefined);
});

test("a handled note's pill says what became of it", () => {
  assert.equal(listStatus({ kind: "fix", status: "open" }), undefined);
  assert.deepEqual(listStatus({ kind: "fix", status: "open", applied: "Started · 1 of 2 steps applied" }), {
    label: "Started",
    tone: "started",
  });
  assert.deepEqual(listStatus({ kind: "stress", status: "applied" }), { label: "✓ Added", tone: "done" });
  assert.deepEqual(listStatus({ kind: "check", status: "confirmed" }), { label: "✓ Correct", tone: "done" });
  assert.deepEqual(listStatus({ kind: "read", status: "dismissed" }), { label: "✓ Read", tone: "done" });
  assert.deepEqual(listStatus({ kind: "check", status: "dismissed" }), { label: "Dismissed", tone: "dismissed" });
});

test("an author's summary leads, with the whole reasoning behind it; older notes fall back to the cut", () => {
  assert.deepEqual(leadOf({ summary: " The lead. ", reasoning: "The working." }), {
    summary: "The lead.",
    more: "The working.",
  });
  assert.deepEqual(leadOf({ summary: "Same.", reasoning: "Same." }), { summary: "Same." });
  assert.deepEqual(leadOf({ summary: null, reasoning: "Short and whole." }), { summary: "Short and whole." });
  const card = toCard(note({ summary: "Lead.", reasoning: "Working." }) as never);
  assert.deepEqual([card.summary, card.more], ["Lead.", "Working."]);
});
test("a stale pick falls back to the first open note; next skips closed ones and wraps", () => {
  const card = (id: number, status = "open") => toCard(note({ id, status }) as never);
  const visible = [card(1, "dismissed"), card(2), card(3, "applied"), card(4)];
  assert.equal(pickedNote(visible, 3)?.id, 3);
  assert.equal(pickedNote(visible, 99)?.id, 2);
  assert.equal(pickedNote(visible)?.id, 2);
  assert.equal(pickedNote([]), undefined);
  assert.equal(steppedNote(visible, 2)?.id, 4);
  assert.equal(steppedNote(visible, 4)?.id, 2);
  assert.equal(steppedNote(visible, 2, -1)?.id, 1);
  // Nothing open: plain next.
  const closed = [card(1, "dismissed"), card(2, "applied")];
  assert.equal(steppedNote(closed, 1)?.id, 2);
  assert.equal(steppedNote(closed, 2)?.id, 1);
});

test("What Review checks groups the checks by layer, each with what it came to", () => {
  const check = (id: string, layer: "rule" | "reviewer" | "preflight", notes: number | null, offers = "none") => ({
    id,
    layer,
    section: "plan" as const,
    kind: "fix" as const,
    looks_for: `${id} looks.`,
    offers,
    notes,
  });
  const checks = [
    check("invalid_plan", "preflight", null),
    check("rmd_missing", "rule", 0, "A yearly Apply RMD event."),
    check("idle_bank_cash", "rule", 2),
    check("missing_social_security", "reviewer", null),
  ];
  const view = reviewChecks({ checks, ai: null });
  assert.ok(view);
  assert.equal(view.summary, "2 rules ran, 1 wrote notes · 1 for the AI reviewer · 1 before each run");
  assert.deepEqual(
    view.groups.map((g) => g.layer),
    ["rule", "reviewer", "preflight"],
  );
  const [rules, reviewer, preflight] = view.groups;
  assert.deepEqual(
    rules.rows.map((r) => [r.id, r.state, r.found, r.offers]),
    [
      ["rmd_missing", "Found nothing", false, "A yearly Apply RMD event."],
      ["idle_bank_cash", "2 notes", true, undefined],
    ],
  );
  assert.equal(rules.rows[0].meta, "Fix · Scenario & events");
  // No model on this plan: the reviewer's checks say so.
  assert.equal(reviewer.rows[0].state, "Needs AI review");
  assert.match(reviewer.heading, /not run/);
  assert.equal(preflight.rows[0].state, "Before each run");

  const withAi = reviewChecks({
    checks,
    ai: { status: "done", stop: null, error: null, started_at: "", finished_at: null, activity: [] },
  });
  assert.equal(withAi?.groups[1].rows[0].state, "AI reviewer");
  // A review stored before checks were listed shows none.
  assert.equal(reviewChecks({ checks: [], ai: null }), undefined);
});
