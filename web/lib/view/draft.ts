import type { AiDrafts } from "../api/generated/AiDrafts.ts";
import type { Account } from "../api/generated/Account.ts";
import type { Asset } from "../api/generated/Asset.ts";
import type { Event } from "../api/generated/Event.ts";
import type { TriggerSpec } from "../api/generated/TriggerSpec.ts";
import type { AnswerType } from "../api/generated/AnswerType.ts";
import type { DocumentManifest } from "../api/generated/DocumentManifest.ts";
import type { DraftCounts } from "../api/generated/DraftCounts.ts";
import type { DraftEstimate } from "../api/generated/DraftEstimate.ts";
import type { DraftQuestion } from "../api/generated/DraftQuestion.ts";
import type { DraftStatus } from "../api/generated/DraftStatus.ts";
import type { Entitlements } from "../api/generated/Entitlements.ts";
import { fmtCurrency, fmtInt } from "../format.ts";
import type { DraftColumn } from "../api/generated/DraftColumn.ts";
import type { Suggestion } from "../api/suggestions.ts";
import { accessPresentation } from "./access.ts";
import { type Card, type CardAction, type Names, toCard } from "./review.ts";

/**
 * Describe & upload (design 2a): the draft's status as the panel on the
 * right of New Scenario shows it, and the small strings around it.
 */

/** How often a draft that is still being written is read again. */
export const DRAFT_POLL_MS = 2_000;

/** A draft in these states is done changing until the person acts. */
export function draftSettled(state: DraftStatus["state"]): boolean {
  return state !== "drafting";
}

function plural(count: number, one: string, many = `${one}s`): string {
  return `${fmtInt(count)} ${count === 1 ? one : many}`;
}

/**
 * "3 accounts, 6 events, 7 parameters" for what the draft holds, leaving out
 * what it does not hold yet; "nothing yet" for an empty draft.
 */
export function countsLine(counts: Pick<DraftCounts, "accounts" | "events" | "parameters">): string {
  const parts = [
    counts.accounts > 0 ? plural(counts.accounts, "account") : undefined,
    counts.events > 0 ? plural(counts.events, "event") : undefined,
    counts.parameters > 0 ? plural(counts.parameters, "parameter") : undefined,
  ].filter((part): part is string => part != null);
  return parts.length > 0 ? parts.join(", ") : "nothing yet";
}

/** "est. 81%": funding success when the agent measured it, else success; the reason it could not, otherwise. */
export function estimateLine(estimate: DraftEstimate | null): string | undefined {
  if (estimate == null) return undefined;
  const rate = estimate.funding_success_rate ?? estimate.success_rate;
  if (rate != null) return `est. ${Math.round(rate * 100)}%`;
  return estimate.blocked ?? undefined;
}

/**
 * "waiting on question 1" for a note whose questions are still open. The
 * number is the question's place in the list the person is shown.
 */
export function waitingOn(
  keys: readonly string[],
  questions: ReadonlyArray<Pick<DraftQuestion, "key">>,
): string | undefined {
  const numbers = keys
    .map((key) => questions.findIndex((q) => q.key === key) + 1)
    .filter((n) => n > 0)
    .sort((a, b) => a - b);
  if (numbers.length === 0) return undefined;
  if (numbers.length === 1) return `waiting on question ${numbers[0]}`;
  return `waiting on questions ${numbers.slice(0, -1).join(", ")} and ${numbers[numbers.length - 1]}`;
}

/** "Free · 1 of 2 AI drafts left this month". */
export function quotaLine(
  access: Pick<Entitlements, "access_mode" | "pro">,
  drafts: Pick<AiDrafts, "remaining" | "per_month">,
): string {
  const { label } = accessPresentation(access);
  const total = Math.max(drafts.per_month, drafts.remaining);
  return `${label} · ${drafts.remaining} of ${total} AI ${total === 1 ? "draft" : "drafts"} left this month`;
}

/** Why the person cannot start one, when they cannot. */
export function quotaBlock(drafts: Pick<AiDrafts, "enabled" | "remaining">): string | undefined {
  if (drafts.enabled) return undefined;
  return drafts.remaining <= 0
    ? "You have used this month's AI drafts."
    : "Your plan slots are full. Delete a plan to start a draft.";
}

const MB = 1024 * 1024;

/** "Up to 10 files, 20 MB and 60 pages in all." */
export function limitsLine(drafts: Pick<AiDrafts, "max_files" | "max_bytes" | "max_pages">): string {
  const size = drafts.max_bytes >= MB ? `${Math.round(drafts.max_bytes / MB)} MB` : `${Math.round(drafts.max_bytes / 1024)} KB`;
  return `Up to ${plural(drafts.max_files, "file")}, ${size} and ${plural(drafts.max_pages, "page")} in all.`;
}

/** "1.2 MB", "340 KB". */
export function fileSize(bytes: number): string {
  if (bytes >= MB) return `${(bytes / MB).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

/**
 * What a file that was read carries with it, for its row in the list: text
 * that could not be redacted is the one the person must know about.
 */
export function documentNote(doc: Pick<DocumentManifest, "status" | "pages" | "note">): string {
  switch (doc.status) {
    case "parsed":
      return doc.pages > 1 ? `${plural(doc.pages, "page")} read` : "Read";
    case "needs_ocr":
      return "No text layer: cannot be redacted";
    case "image_unredacted":
      return "Image: sent as-is, cannot be redacted";
    case "failed":
      return doc.note ?? "Could not be read";
  }
}

/** The consent shown above the attach control. Kept here so one place says it. */
export const CONSENT_LINES: readonly string[] = [
  "What you write and the text of your files are sent to our AI provider, over zero-data-retention routing, to write the draft.",
  "Account numbers, names and other identifiers are redacted from the text first.",
  "Images and scans with no text layer cannot be redacted and are sent as they are.",
  "Original files are not kept: only the redacted text is stored, and it is deleted once the draft is made unless you choose to keep it.",
];

export const RETENTION_OPTIONS = [
  { value: "delete", label: "Delete once the draft is made" },
  { value: "keep", label: "Keep with this plan" },
] as const;

export type Retention = (typeof RETENTION_OPTIONS)[number]["value"];

/** How a question's answer is entered, and what it is sent as. */
export interface AnswerInput {
  type: AnswerType;
  /** Text as typed or picked; money and dates arrive as strings from inputs. */
  raw: string | undefined;
}

/**
 * The value `POST /drafts/{id}/answers` takes for one question, or undefined
 * while it is not answerable: a choice sends the option's value, money a
 * number, a date `YYYY-MM-DD`, text the trimmed text.
 */
export function answerValue({ type, raw }: AnswerInput): string | number | undefined {
  const text = raw?.trim();
  if (!text) return undefined;
  switch (type) {
    case "choice":
    case "text":
      return text;
    case "money": {
      const value = Number(text.replace(/[$,\s]/g, ""));
      return Number.isFinite(value) ? value : undefined;
    }
    case "date":
      return /^\d{4}-\d{2}-\d{2}$/.test(text) ? text : undefined;
  }
}

/** Every open question has an answer the server would take. */
export function answersOf(
  questions: ReadonlyArray<Pick<DraftQuestion, "key" | "answer_type">>,
  raw: Readonly<Record<string, string>>,
): Record<string, string | number> | undefined {
  const answers: Record<string, string | number> = {};
  for (const q of questions) {
    const value = answerValue({ type: q.answer_type, raw: raw[q.key] });
    if (value === undefined) return undefined;
    answers[q.key] = value;
  }
  return questions.length > 0 ? answers : undefined;
}

export interface DraftPanel {
  /** "Drafting…", "Waiting on your answers", "Draft ready", "Drafting stopped". */
  heading: string;
  /** "3 accounts, 6 events, 7 parameters so far". */
  summary: string;
  /** The agent's own progress line, while it is working. */
  progress?: string;
  estimate?: string;
  /** Notes held back by an open question. */
  waiting: Array<{ id: number; title: string; text: string }>;
  /** The header of Review: "12 notes · 7 added · 3 to confirm". */
  notes?: string;
  error?: string;
  busy: boolean;
}

export function draftPanel(status: DraftStatus): DraftPanel {
  const { state, counts } = status;
  const summary = state === "drafting" ? `${countsLine(counts)} so far` : countsLine(counts);
  const heading = {
    drafting: "Drafting…",
    awaiting_answers: "Waiting on your answers",
    ready: "Draft ready",
    failed: "Drafting stopped",
  }[state];
  const waiting = status.blocked_notes.flatMap((note) => {
    const text = waitingOn(note.waiting_on, status.questions);
    return text ? [{ id: note.id, title: note.title, text }] : [];
  });
  return {
    heading,
    summary,
    progress: state === "drafting" ? (status.progress ?? undefined) : undefined,
    estimate: estimateLine(status.estimate),
    waiting,
    notes: counts.notes > 0 ? notesHeader(counts) : undefined,
    error: state === "failed" ? (status.error ?? "The draft could not be written.") : undefined,
    busy: state === "drafting",
  };
}

/** "12 notes · 7 added · 3 to confirm", leaving out a zero part. */
export function notesHeader(counts: Pick<DraftCounts, "notes" | "notes_added" | "notes_to_confirm">): string {
  return [
    plural(counts.notes, "note"),
    counts.notes_added > 0 ? `${fmtInt(counts.notes_added)} added` : undefined,
    counts.notes_to_confirm > 0 ? `${fmtInt(counts.notes_to_confirm)} to confirm` : undefined,
  ]
    .filter(Boolean)
    .join(" · ");
}

/** The buttons of a draft's note, in a draft's words: nothing here is a plan yet. */
export const DRAFT_ACTION_LABEL: Partial<Record<CardAction, string>> = {
  apply: "Add to draft",
  "apply-path": "Add to draft",
  confirm: "It's correct",
  dismiss: "Leave out",
};

export const DRAFT_COLUMNS: ReadonlyArray<{ id: DraftColumn; heading: string }> = [
  { id: "portfolio", heading: "Portfolio" },
  { id: "plan", heading: "Plan" },
  { id: "to_confirm", heading: "To confirm" },
];

export interface DraftCard {
  card: Card;
  /** "Added": the note's changes are in the draft. */
  badge?: string;
  /** "waiting on question 1", for a note an unanswered question holds back. */
  waiting?: string;
}

export interface DraftBoard {
  /** "12 notes · 7 added · 3 to confirm". */
  headline: string;
  columns: Array<{ column: DraftColumn; heading: string; cards: DraftCard[] }>;
}

/** Where a note sits: its column hint, else by the part of the plan it is about. */
export function columnOf(suggestion: Pick<Suggestion, "column" | "section">): DraftColumn {
  return suggestion.column ?? (suggestion.section === "portfolio" ? "portfolio" : "plan");
}

/**
 * A draft's notes as Review's three columns (design 2c). A note the agent or
 * the person applied stays, marked "Added"; one set aside is gone. Nothing
 * here can be simulated or copied: a draft has no run.
 */
export function draftBoard(
  suggestions: readonly Suggestion[],
  {
    names,
    questions = [],
    selections = {},
    chat = true,
  }: {
    names?: Names;
    questions?: ReadonlyArray<Pick<DraftQuestion, "key">>;
    selections?: Readonly<Record<number, string>>;
    chat?: boolean;
  } = {},
): DraftBoard {
  const shown = suggestions.filter((s) => s.status !== "dismissed");
  const cards = new Map<DraftColumn, DraftCard[]>(DRAFT_COLUMNS.map(({ id }) => [id, []]));
  for (const s of shown) {
    const base = toCard(s, names, { selected: selections[s.id], chat });
    const card: Card = {
      ...base,
      applied: undefined,
      actions: base.actions.filter((a) => a in DRAFT_ACTION_LABEL),
    };
    cards.get(columnOf(s))?.push({
      card,
      badge: s.status === "applied" ? "Added" : undefined,
      waiting: s.status === "open" ? waitingOn(s.blocked_by, questions) : undefined,
    });
  }
  return {
    headline: notesHeader({
      notes: shown.length,
      notes_added: shown.filter((s) => s.status === "applied").length,
      notes_to_confirm: shown.filter((s) => s.status === "open" && columnOf(s) === "to_confirm").length,
    }),
    columns: DRAFT_COLUMNS.map(({ id, heading }) => ({ column: id, heading, cards: cards.get(id) ?? [] })),
  };
}

// ── what the draft holds (design 2a, the Draft rail's lists) ────────────────

export interface DraftAccountLine {
  id: number;
  name: string;
  /** "Bank", "Taxable", "Tax-deferred"…: the tag beside the name. */
  tag: string;
  tone: "neutral" | "accent" | "accent-2" | "outline";
  /** Signed: a loan is negative. */
  balance: number;
}

export interface DraftEventLine {
  id: number;
  name: string;
  /** "yearly until age 65", "once at age 35". */
  when: string;
}

export interface DraftContents {
  accounts: DraftAccountLine[];
  /** The accounts' balances summed: the opening net worth. */
  total: number;
  events: DraftEventLine[];
}

const TAX_TAG: Record<string, Pick<DraftAccountLine, "tag" | "tone">> = {
  Taxable: { tag: "Taxable", tone: "neutral" },
  TaxDeferred: { tag: "Tax-deferred", tone: "accent" },
  TaxFree: { tag: "Tax-free", tone: "accent-2" },
};

const INTERVAL_WORD: Record<string, string> = {
  Never: "once",
  Weekly: "weekly",
  BiWeekly: "every 2 weeks",
  Monthly: "monthly",
  Quarterly: "quarterly",
  Yearly: "yearly",
};

/**
 * An account's opening balance: cash plus holdings marked at each asset's
 * opening price, as the engine starts from (and `toViewAccounts` shows).
 */
function openingBalance(account: Account, price: (assetId: number) => number): number {
  const held = account.positions.reduce((sum, lot) => sum + lot.units * price(lot.asset_id), 0);
  switch (account.flavor) {
    case "Bank":
      return account.cash_value;
    case "Investment":
      return account.cash_value + held;
    case "Property":
      return account.value;
    case "Liability":
      return -account.principal;
  }
}

/** A trigger as a point in a sentence: "age 65", "2031-05-01", "Retirement". */
function point(trigger: TriggerSpec, eventName: (id: number) => string): string {
  switch (trigger.kind) {
    case "Age":
      return `age ${trigger.years}`;
    case "Date":
      return trigger.on_date;
    case "AgeParameter":
      return "a set age";
    case "DateParameter":
      return "a set date";
    case "RelativeToEvent":
      return eventName(trigger.event_id);
    default:
      return "a condition";
  }
}

/** When an event fires, in a few words for the rail. */
export function eventWhen(trigger: TriggerSpec, eventName: (id: number) => string): string {
  switch (trigger.kind) {
    case "Repeating": {
      const parts = [INTERVAL_WORD[trigger.interval] ?? trigger.interval.toLowerCase()];
      if (trigger.start_condition) parts.push(`from ${point(trigger.start_condition, eventName)}`);
      if (trigger.end_condition) parts.push(`until ${point(trigger.end_condition, eventName)}`);
      return parts.join(" ");
    }
    case "Age":
    case "AgeParameter":
      return `once at ${point(trigger, eventName)}`;
    case "Date":
    case "DateParameter":
      return `once on ${point(trigger, eventName)}`;
    case "RelativeToEvent": {
      const size = Math.abs(trigger.value);
      const unit = trigger.unit.toLowerCase().replace(/s$/, size === 1 ? "" : "s");
      return size === 0
        ? `with ${eventName(trigger.event_id)}`
        : `${size} ${unit} ${trigger.value < 0 ? "before" : "after"} ${eventName(trigger.event_id)}`;
    }
    case "AccountBalance":
    case "AssetBalance":
      return `when a balance crosses ${fmtCurrency(trigger.threshold)}`;
    case "NetWorth":
      return `when net worth crosses ${fmtCurrency(trigger.threshold)}`;
    case "And":
    case "Or":
      return "when its conditions hold";
    case "Manual":
      return "when triggered";
  }
}

/** The draft's accounts and events as the rail lists them, in the plan's order. */
export function draftContents(
  accounts: readonly Account[],
  assets: ReadonlyArray<Pick<Asset, "id" | "initial_price">>,
  events: readonly Event[],
): DraftContents {
  const prices = new Map(assets.map((asset) => [asset.id, asset.initial_price]));
  const price = (id: number) => prices.get(id) ?? 0;
  const names = new Map(events.map((event) => [event.id, event.name]));
  const eventName = (id: number) => names.get(id) ?? "another event";
  const lines = [...accounts]
    .sort((a, b) => a.sort_order - b.sort_order)
    .map((account): DraftAccountLine => {
      const tag =
        account.flavor === "Bank"
          ? { tag: "Bank", tone: "neutral" as const }
          : account.flavor === "Investment"
            ? (TAX_TAG[account.tax_status] ?? TAX_TAG.Taxable)
            : account.flavor === "Property"
              ? { tag: "Property", tone: "neutral" as const }
              : { tag: "Loan", tone: "outline" as const };
      return { id: account.id, name: account.name, ...tag, balance: openingBalance(account, price) };
    });
  return {
    accounts: lines,
    total: lines.reduce((sum, line) => sum + line.balance, 0),
    events: [...events]
      .sort((a, b) => a.sort_order - b.sort_order)
      .map((event) => ({ id: event.id, name: event.name, when: eventWhen(event.trigger, eventName) })),
  };
}

/** An answer as the person's message shows it: a choice by its label, money as dollars. */
export function answerLabel(
  question: Pick<DraftQuestion, "answer_type" | "options">,
  raw: string | undefined,
): string {
  if (raw == null || raw === "") return "—";
  if (question.answer_type === "choice")
    return question.options.find((option) => option.value === raw)?.label ?? raw;
  if (question.answer_type === "money" && Number.isFinite(Number(raw))) return fmtCurrency(Number(raw));
  return raw;
}

/**
 * What goes to the agent from the composer: what was typed, and the names of
 * files attached since the last message, so the agent knows to read them.
 */
export function outgoingMessage(text: string, attached: ReadonlyArray<Pick<DocumentManifest, "filename">>): string {
  const files = attached.length > 0 ? `(Attached: ${attached.map((doc) => doc.filename).join(", ")})` : "";
  return [text.trim(), files].filter(Boolean).join("\n\n");
}
