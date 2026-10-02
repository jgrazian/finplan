import assert from "node:assert/strict";
import { test } from "node:test";
import type { Review, Suggestion, SuggestionThread } from "../lib/api/suggestions.ts";
import {
  CHAT_MAX_CHARS,
  CHAT_MAX_MESSAGES,
  CHAT_PROMPT,
  cardAnchor,
  chatLine,
  chatOffer,
  lastQuestion,
  allowanceLine,
  settled,
  threadView,
} from "../lib/view/chat.ts";
import { board, parentOf, toCard } from "../lib/view/review.ts";

// ── fixtures ────────────────────────────────────────────────────────────────

function step() {
  return {
    key: "a",
    title: "Pay the down payment from USAA",
    reasoning: null,
    changes: [{ op: "remove", target: { event: 5 }, path: "/effects/0" }],
    diff: [{ label: "Plan › Home Purchase › effects › Sweep", from: "Sweep $200,000 → USAA", to: null }],
    applied: false,
    applied_at: null,
  };
}

function path() {
  return { key: "a", label: "Pay from cash", reasoning: null, recommended: true, steps: [step()], estimate: null, check: null };
}

function note(overrides: Record<string, unknown> = {}): Suggestion {
  return {
    id: 1,
    scenario_id: 1,
    run_id: 7,
    source: "rules",
    rule: "sweep_sells_while_cash",
    kind: "fix",
    section: "plan",
    title: "Home Purchase sells $218k of investments while USAA holds $1.04M",
    summary: null as string | null,
    reasoning: "The Sweep runs before the down payment.",
    evidence: [],
    paths: [path()],
    applied_path: null,
    status: "open",
    created_at: "2026-09-27 12:04:00",
    resolved_at: null,
    parent_id: null,
    ...overrides,
  } as unknown as Suggestion;
}

const ON = { status: "done", stop: "finished", error: null, started_at: "2026-09-27 12:05:00", finished_at: "2026-09-27 12:06:00" };

function review(suggestions: Suggestion[], ai: unknown = ON): Review {
  return { run_id: 7, reviewed_at: "2026-09-27 12:05:00", ai, suggestions } as unknown as Review;
}

function thread(overrides: Partial<SuggestionThread> = {}): SuggestionThread {
  return { suggestion_id: 1, status: "idle", error: null, messages: [], ...overrides };
}

function message(id: number, role: "user" | "assistant", text: string, suggestion_ids: number[] = []) {
  return { id, role, text, created_at: "2026-09-27 12:10:00", suggestion_ids };
}

const cards = (b: ReturnType<typeof board>) => b.columns.flatMap((c) => c.cards);

// ── offering the chat ───────────────────────────────────────────────────────

test("the chat is offered only while AI review is on, louder on a note with no change", () => {
  assert.equal(chatOffer({ enabled: false, paths: 0 }), undefined);
  assert.equal(chatOffer({ enabled: false, paths: 2 }), undefined);
  assert.equal(chatOffer({ enabled: true, paths: 0 }), "prominent");
  assert.equal(chatOffer({ enabled: true, paths: 1 }), "quiet");
  assert.equal(CHAT_PROMPT.prominent.lead, "No change offered —");
  assert.equal(CHAT_PROMPT.prominent.label, "Chat about this");
  assert.equal(CHAT_PROMPT.quiet.label, "Chat about this");
});

test("the board offers the chat on every card when Review.ai is set, and on none when it is null", () => {
  const readNote = note({ id: 2, kind: "read", section: "results", paths: [] });
  const on = cards(board(review([note(), readNote])));
  assert.deepEqual(on.map((c) => [c.id, c.chat]), [
    [1, "quiet"],
    [2, "prominent"],
  ]);
  const off = cards(board(review([note(), readNote], null)));
  assert.deepEqual(off.map((c) => c.chat), [undefined, undefined]);
  // A failed AI pass still means the model is configured: chat stays on.
  const failed = cards(board(review([note()], { ...ON, status: "failed", error: "unreachable" })));
  assert.equal(failed[0].chat, "quiet");
  // toCard on its own offers nothing unless asked.
  assert.equal(toCard(note()).chat, undefined);
  assert.equal(toCard(note(), undefined, { chat: true }).chat, "quiet");
});

// ── the thread ──────────────────────────────────────────────────────────────

test("an empty thread takes a message once there is one to send", () => {
  const empty = threadView(undefined);
  assert.equal(empty.state, "empty");
  assert.equal(empty.canSend, false, "nothing to send");
  assert.equal(empty.status, undefined);
  assert.equal(empty.counter, "0 / 2,000");
  assert.equal(threadView(thread(), { draft: "   " }).canSend, false, "whitespace is not a message");
  assert.equal(threadView(thread(), { draft: "Why no Social Security?" }).canSend, true);
});

test("an idle thread lists its messages in order, speakers named", () => {
  const view = threadView(
    thread({ messages: [message(1, "user", "Why?\nTwo lines."), message(2, "assistant", "Because.")] }),
    { draft: "Thanks" },
  );
  assert.equal(view.state, "idle");
  assert.deepEqual(view.lines.map((l) => [l.speaker, l.text]), [
    ["You", "Why?\nTwo lines."],
    ["AI", "Because."],
  ]);
  assert.equal(view.canSend, true);
});

test("while a reply is written, or the message is still on its way, the box waits", () => {
  const running = threadView(thread({ status: "running", messages: [message(1, "user", "Why?")] }), { draft: "More" });
  assert.equal(running.state, "running");
  assert.equal(running.status, "Thinking…");
  assert.equal(running.canSend, false);
  const sending = threadView(thread(), { draft: "Why?", sending: true });
  assert.equal(sending.state, "running");
  assert.equal(sending.canSend, false);
});

test("a failed turn says why and offers to send the last question again", () => {
  const failed = threadView(
    thread({ status: "failed", error: "the review model could not be reached", messages: [message(1, "user", "Why?")] }),
  );
  assert.equal(failed.state, "failed");
  assert.equal(failed.status, "The reply did not arrive: the review model could not be reached.");
  assert.equal(failed.retry, "Try again");
  assert.equal(lastQuestion(thread({ messages: [message(1, "user", "Why?"), message(2, "assistant", "…")] })), "Why?");
  // Nothing asked, nothing to retry.
  assert.equal(threadView(thread({ status: "failed", error: "x" })).retry, undefined);
});

test("a full thread stops taking messages and says so", () => {
  const messages = Array.from({ length: CHAT_MAX_MESSAGES }, (_, i) =>
    message(i + 1, i % 2 === 0 ? "user" : "assistant", `m${i}`),
  );
  const full = threadView(thread({ messages }), { draft: "One more" });
  assert.equal(full.state, "full");
  assert.equal(full.canSend, false);
  assert.equal(full.blocked, "This thread has reached its 20-message limit.");
  assert.equal(full.retry, undefined);
});

test("a draft over the limit is flagged and not sent", () => {
  const view = threadView(thread(), { draft: "x".repeat(CHAT_MAX_CHARS + 1) });
  assert.equal(view.over, true);
  assert.equal(view.canSend, false);
  assert.equal(view.counter, "2,001 / 2,000");
});

test("a reply that added suggestions links to them; a question never does", () => {
  assert.equal(chatLine(message(2, "assistant", "Added one.", [9])).link, "Added a suggestion ↓");
  assert.equal(chatLine(message(2, "assistant", "Added two.", [9, 10])).link, "Added 2 suggestions ↓");
  assert.equal(chatLine(message(2, "assistant", "Just text.")).link, undefined);
  assert.equal(chatLine(message(1, "user", "Odd", [9])).link, undefined);
  assert.equal(cardAnchor(9), "suggestion-9");
});

test("a turn has settled when a running thread reads back not running", () => {
  assert.equal(settled(thread({ status: "running" }), thread({ status: "idle" })), true);
  assert.equal(settled(thread({ status: "running" }), thread({ status: "failed" })), true);
  assert.equal(settled(thread({ status: "running" }), thread({ status: "running" })), false);
  assert.equal(settled(thread({ status: "idle" }), thread({ status: "idle" })), false);
  assert.equal(settled(undefined, thread({ status: "idle" })), false, "a first read is not a turn ending");
});

// ── notes a chat wrote ──────────────────────────────────────────────────────

test("notes a chat reply wrote sit under the note they answer, oldest first", () => {
  const parent = note({ id: 1, kind: "read", section: "results", paths: [] });
  const later = note({ id: 5, source: "ai", section: "plan", parent_id: 1 });
  const earlier = note({ id: 3, source: "ai", section: "portfolio", parent_id: 1 });
  const b = board(review([parent, later, earlier]));
  const top = cards(b);
  assert.deepEqual(top.map((c) => c.id), [1], "children are not listed beside their parent");
  assert.deepEqual(top[0].children.map((c) => [c.id, c.parentId]), [
    [3, 1],
    [5, 1],
  ]);
  // They are whole cards: paths, actions and their own chat.
  assert.deepEqual(top[0].children[0].actions.includes("apply"), true);
  assert.equal(top[0].children[0].chat, "quiet");
  // The parent's column counts only its own notes.
  assert.equal(b.columns.find((c) => c.section === "results")?.cards.length, 1);
  assert.equal(b.columns.find((c) => c.section === "plan")?.cards.length, 0);
});

test("a chat can go a level deeper, and a child whose parent is set aside stands alone", () => {
  const parent = note({ id: 1 });
  const child = note({ id: 2, parent_id: 1 });
  const grandchild = note({ id: 3, parent_id: 2 });
  const orphan = note({ id: 4, parent_id: 99 });
  const dismissedParent = note({ id: 5, status: "dismissed" });
  const underDismissed = note({ id: 6, parent_id: 5 });
  const top = cards(board(review([parent, child, grandchild, orphan, dismissedParent, underDismissed])));
  assert.deepEqual(top.map((c) => c.id).sort(), [1, 4, 6]);
  const one = top.find((c) => c.id === 1)!;
  assert.deepEqual(one.children.map((c) => c.id), [2]);
  assert.deepEqual(one.children[0].children.map((c) => c.id), [3]);
});

test("a cycle of parents never hides its notes", () => {
  const a = note({ id: 1, parent_id: 2 });
  const b = note({ id: 2, parent_id: 1 });
  const self = note({ id: 3, parent_id: 3 });
  const top = cards(board(review([a, b, self])));
  const seen = new Set<number>();
  const walk = (list: typeof top) => {
    for (const c of list) {
      assert.equal(seen.has(c.id), false, `card ${c.id} shown twice`);
      seen.add(c.id);
      walk(c.children);
    }
  };
  walk(top);
  assert.deepEqual([...seen].sort(), [1, 2, 3]);
});

test("reviews stored before chat carry no parent_id and read as top-level", () => {
  const old = note();
  delete (old as { parent_id?: unknown }).parent_id;
  assert.equal(parentOf(old), null);
  assert.deepEqual(cards(board(review([old]))).map((c) => c.id), [1]);
});

// ── plan chat ───────────────────────────────────────────────────────────────

function planThread(messages: number) {
  return {
    status: "idle" as const,
    error: null,
    messages: Array.from({ length: messages }, (_, i) => ({
      id: i + 1,
      role: i % 2 === 0 ? ("user" as const) : ("assistant" as const),
      text: `message ${i + 1}`,
      created_at: "2026-09-28 12:00:00",
      suggestion_ids: [],
    })),
  };
}

test("a plan's thread has no message cap, only the month's allowance", () => {
  const long = planThread(CHAT_MAX_MESSAGES + 10);
  const open = threadView(long, { draft: "Retire at 62", maxMessages: null, allowance: { remaining: 3, per_month: 10 } });
  assert.equal(open.state, "idle");
  assert.equal(open.canSend, true);
  assert.equal(open.blocked, undefined);

  const spent = threadView(long, { draft: "Retire at 62", maxMessages: null, allowance: { remaining: 0, per_month: 10 } });
  assert.equal(spent.canSend, false);
  assert.match(spent.blocked ?? "", /used this month's 10 plan chat messages/);
  assert.equal(spent.retry, undefined);
});

test("the allowance reads as what is left of the month", () => {
  assert.equal(allowanceLine({ remaining: 12, per_month: 200 }), "12 of 200 messages left this month");
  assert.equal(allowanceLine({ remaining: 1_500, per_month: 2_000 }), "1,500 of 2,000 messages left this month");
});
