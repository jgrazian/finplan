import assert from "node:assert/strict";
import { test } from "node:test";
import type { DraftStatus } from "../lib/api/generated/DraftStatus.ts";
import type { Suggestion } from "../lib/api/suggestions.ts";
import {
  answersOf,
  answerValue,
  countsLine,
  documentNote,
  draftBoard,
  draftPanel,
  estimateLine,
  limitsLine,
  notesHeader,
  quotaBlock,
  quotaLine,
  waitingOn,
} from "../lib/view/draft.ts";

const status = (over: Partial<DraftStatus> = {}): DraftStatus =>
  ({
    id: 5,
    state: "drafting",
    counts: { accounts: 3, assets: 0, events: 6, parameters: 7, open_suggestions: 0, notes: 0, notes_added: 0, notes_to_confirm: 0 },
    retain_documents: false,
    document_count: 0,
    progress: null,
    error: null,
    stop: null,
    questions: [],
    answered: [],
    blocked_notes: [],
    estimate: null,
    ...over,
  }) as unknown as DraftStatus;

const note = (over: Partial<Suggestion> = {}): Suggestion =>
  ({
    id: 1,
    scenario_id: 5,
    run_id: null,
    source: "ai",
    rule: null,
    kind: "add",
    section: "portfolio",
    title: "Checking account",
    reasoning: "From the statement.",
    evidence: [],
    paths: [],
    applied_path: null,
    status: "open",
    created_at: "2026-09-27 12:04:00",
    resolved_at: null,
    parent_id: null,
    note_key: null,
    blocked_by: [],
    column: null,
    auto_added: false,
    ...over,
  }) as unknown as Suggestion;

test("the live count reads as the design does", () => {
  const panel = draftPanel(status());
  assert.equal(panel.heading, "Drafting…");
  assert.equal(panel.summary, "3 accounts, 6 events, 7 parameters so far");
  assert.equal(panel.busy, true);
  assert.equal(countsLine({ accounts: 1, events: 0, parameters: 0 }), "1 account");
  assert.equal(countsLine({ accounts: 0, events: 0, parameters: 0 }), "nothing yet");
});

test("settled states stop claiming progress", () => {
  const ready = draftPanel(status({ state: "ready" }));
  assert.equal(ready.busy, false);
  assert.equal(ready.summary, "3 accounts, 6 events, 7 parameters");
  const failed = draftPanel(status({ state: "failed", error: "The model timed out." }));
  assert.equal(failed.heading, "Drafting stopped");
  assert.equal(failed.error, "The model timed out.");
  assert.equal(draftPanel(status({ state: "failed" })).error, "The draft could not be written.");
  assert.equal(draftPanel(status({ state: "awaiting_answers" })).heading, "Waiting on your answers");
});

test("blocked notes name the question by its place in the list", () => {
  const questions = [{ key: "bonus" }, { key: "mix" }];
  assert.equal(waitingOn(["bonus"], questions), "waiting on question 1");
  assert.equal(waitingOn(["mix", "bonus"], questions), "waiting on questions 1 and 2");
  assert.equal(waitingOn(["gone"], questions), undefined);
  const panel = draftPanel(
    status({
      state: "awaiting_answers",
      questions: questions as never,
      blocked_notes: [{ id: 9, key: "k", title: "Bonus", waiting_on: ["mix"] }],
    }),
  );
  assert.deepEqual(panel.waiting, [{ id: 9, title: "Bonus", text: "waiting on question 2" }]);
});

test("the estimate prefers funding success and falls back to why it could not run", () => {
  assert.equal(estimateLine(null), undefined);
  assert.equal(estimateLine({ success_rate: 0.9, funding_success_rate: 0.812, iterations: 100, blocked: null }), "est. 81%");
  assert.equal(estimateLine({ success_rate: 0.9, funding_success_rate: null, iterations: 100, blocked: null }), "est. 90%");
  assert.equal(estimateLine({ success_rate: null, funding_success_rate: null, iterations: 0, blocked: "No accounts yet" }), "No accounts yet");
});

test("the quota line names the tier and what is left", () => {
  assert.equal(
    quotaLine({ access_mode: "subscription", pro: false }, { remaining: 1, per_month: 2 }),
    "Free · 1 of 2 AI drafts left this month",
  );
  assert.equal(
    quotaLine({ access_mode: "subscription", pro: true }, { remaining: 1, per_month: 1 }),
    "Pro · 1 of 1 AI draft left this month",
  );
  assert.equal(quotaBlock({ enabled: true, remaining: 1 }), undefined);
  assert.match(quotaBlock({ enabled: false, remaining: 0 }) ?? "", /used this month/);
  assert.match(quotaBlock({ enabled: false, remaining: 1 }) ?? "", /plan slots/);
});

test("limits and file notes", () => {
  assert.equal(limitsLine({ max_files: 10, max_bytes: 20 * 1024 * 1024, max_pages: 60 }), "Up to 10 files, 20 MB and 60 pages in all.");
  assert.equal(documentNote({ status: "parsed", pages: 3, note: null }), "3 pages read");
  assert.match(documentNote({ status: "image_unredacted", pages: 1, note: null }), /cannot be redacted/);
  assert.equal(documentNote({ status: "failed", pages: 0, note: "Encrypted PDF" }), "Encrypted PDF");
});

test("answers are sent in the type the server takes", () => {
  assert.equal(answerValue({ type: "money", raw: "$1,200" }), 1200);
  assert.equal(answerValue({ type: "money", raw: "lots" }), undefined);
  assert.equal(answerValue({ type: "date", raw: "2026-01-31" }), "2026-01-31");
  assert.equal(answerValue({ type: "date", raw: "Jan 31" }), undefined);
  assert.equal(answerValue({ type: "choice", raw: "yes" }), "yes");
  assert.equal(answerValue({ type: "text", raw: "  " }), undefined);
  const questions = [
    { key: "bonus", answer_type: "choice" as const },
    { key: "cash", answer_type: "money" as const },
  ];
  assert.deepEqual(answersOf(questions, { bonus: "no", cash: "500" }), { bonus: "no", cash: 500 });
  assert.equal(answersOf(questions, { bonus: "no" }), undefined);
  assert.equal(answersOf([], {}), undefined);
});

test("the board groups notes by column and marks what was added", () => {
  const board = draftBoard(
    [
      note({ id: 1, column: "portfolio", status: "applied", auto_added: true }),
      note({ id: 2, column: "plan", blocked_by: ["bonus"] }),
      note({ id: 3, column: "to_confirm", kind: "check" }),
      note({ id: 4, column: null, section: "plan" }),
      note({ id: 5, status: "dismissed" }),
    ],
    { questions: [{ key: "bonus" }] },
  );
  assert.equal(board.headline, "4 notes · 1 added · 1 to confirm");
  const [portfolio, plan, confirm] = board.columns;
  assert.deepEqual(portfolio.cards.map((c) => c.card.id), [1]);
  assert.equal(portfolio.cards[0].badge, "Added");
  assert.deepEqual(plan.cards.map((c) => c.card.id), [2, 4]);
  assert.equal(plan.cards[0].waiting, "waiting on question 1");
  assert.deepEqual(confirm.cards.map((c) => c.card.id), [3]);
  assert.equal(portfolio.cards[0].card.applied, undefined);
});

test("a draft note offers no preview or copy", () => {
  const step = { key: "s", title: "t", reasoning: null, changes: [{}], diff: [], applied: false, applied_at: null };
  const withPath = note({
    paths: [{ key: "p", label: "Add", reasoning: null, recommended: true, steps: [step], estimate: null, check: null }] as never,
  });
  const [only] = draftBoard([withPath]).columns[0].cards;
  assert.deepEqual(only.card.actions, ["apply", "dismiss"]);
});

test("notes header leaves out zero parts", () => {
  assert.equal(notesHeader({ notes: 1, notes_added: 0, notes_to_confirm: 0 }), "1 note");
});
