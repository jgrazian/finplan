/**
 * Chat threads on the Review tab, from what the server keeps to what is
 * shown: "Chat about this" under one note, and plan chat about the whole
 * plan.
 *
 * The model answers in text and, where a change fits, writes a new
 * suggestion: under the note it was asked about, or on the board for plan
 * chat. It never edits the plan; the user applies what it wrote.
 */
import type { AiPlanChat, ChatMessage, SuggestionThread } from "../api/suggestions.ts";
import { type ActivityLine, activityLines, runningStatus } from "./activity.ts";

export type { ActivityLine } from "./activity.ts";

/** What both kinds of thread share. */
export type ChatThread = Pick<SuggestionThread, "status" | "error" | "messages" | "activity">;

/** Longest message the server takes. */
export const CHAT_MAX_CHARS = 2_000;
/** Messages a thread holds, both sides counted, before the server refuses more. */
export const CHAT_MAX_MESSAGES = 20;

/** How the chat is offered on a card, if at all. */
export type ChatOffer = "quiet" | "prominent";

/**
 * Whether a card offers the chat, and how loudly. Only while AI review is on
 * (`Review.ai` is null when the server has no model). A note that offers no
 * change asks for it most: talking it through is the way to get one.
 */
export function chatOffer({ enabled, paths }: { enabled: boolean; paths: number }): ChatOffer | undefined {
  if (!enabled) return undefined;
  return paths === 0 ? "prominent" : "quiet";
}

/**
 * The prompt shown for each offer: a note with no change leads with that, then
 * the same label; the button itself always reads "Chat about this".
 */
export const CHAT_PROMPT: Record<ChatOffer, { lead?: string; label: string }> = {
  quiet: { label: "Chat about this" },
  prominent: { lead: "No change offered —", label: "Chat about this" },
};

/** The toggle's label once the thread is open. */
export const CHAT_HIDE = "Hide chat";

export interface ChatLine {
  id: number;
  role: ChatMessage["role"];
  /** "You" or "AI". */
  speaker: string;
  /** Verbatim; line breaks kept, never read as markup. */
  text: string;
  /** Suggestions this reply added, to scroll to. */
  suggestionIds: number[];
  /** "Added a suggestion ↓" / "Added 2 suggestions ↓". */
  link?: string;
}

export type ThreadState = "empty" | "idle" | "running" | "failed" | "full";

export interface ThreadView {
  lines: ChatLine[];
  state: ThreadState;
  /**
   * While a turn runs, what it is doing now ("Thinking…", "Simulating the
   * change…"); the failure's words when it failed.
   */
  status?: string;
  /** What the running turn has done so far, oldest first; empty otherwise. */
  activity: ActivityLine[];
  /** Offer to send the last message again (after a failed turn). */
  retry?: string;
  /** Whether the box takes a message now. */
  canSend: boolean;
  /** Why it doesn't, shown in its place. */
  blocked?: string;
  /** "123 / 2,000". */
  counter: string;
  /** The draft is past the limit. */
  over: boolean;
}

function addedLink(count: number): string | undefined {
  if (count === 0) return undefined;
  return count === 1 ? "Added a suggestion ↓" : `Added ${count} suggestions ↓`;
}

export function chatLine(message: ChatMessage): ChatLine {
  return {
    id: message.id,
    role: message.role,
    speaker: message.role === "user" ? "You" : "AI",
    text: message.text,
    suggestionIds: message.suggestion_ids,
    link: message.role === "assistant" ? addedLink(message.suggestion_ids.length) : undefined,
  };
}

/** The last thing the user asked, to send again after a failed turn. */
export function lastQuestion(thread: ChatThread | undefined): string | undefined {
  const messages = thread?.messages ?? [];
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === "user") return messages[i].text;
  }
  return undefined;
}

/**
 * What the thread shows and whether it takes a message. `sending` covers the
 * request that starts a turn, before the server says it is running.
 * `maxMessages` caps a note's thread (a plan's has none: `null`), and
 * `allowance` is plan chat's month, which stops the box once it is spent.
 */
export function threadView(
  thread: ChatThread | undefined,
  {
    draft = "",
    sending = false,
    maxMessages = CHAT_MAX_MESSAGES,
    allowance,
  }: {
    draft?: string;
    sending?: boolean;
    maxMessages?: number | null;
    allowance?: Pick<AiPlanChat, "remaining" | "per_month">;
  } = {},
): ThreadView {
  const messages = thread?.messages ?? [];
  const running = sending || thread?.status === "running";
  const spent = allowance != null && allowance.remaining <= 0;
  const full = spent || (maxMessages != null && messages.length >= maxMessages);
  const failed = !running && thread?.status === "failed";
  const state: ThreadState = running
    ? "running"
    : failed
      ? "failed"
      : full
        ? "full"
        : messages.length === 0
          ? "empty"
          : "idle";
  const over = draft.length > CHAT_MAX_CHARS;
  const activity = running ? activityLines(thread?.activity ?? []) : [];
  const blocked =
    full && !running
      ? spent
        ? `You have used this month's ${allowance.per_month.toLocaleString("en-US")} plan chat messages. They renew at the start of next month (UTC).`
        : `This thread has reached its ${maxMessages}-message limit.`
      : undefined;
  return {
    lines: messages.map(chatLine),
    state,
    activity,
    status: running
      ? runningStatus(activity)
      : failed
        ? `The reply did not arrive: ${thread?.error ?? "the model could not answer"}.`
        : undefined,
    retry: failed && !full && lastQuestion(thread) ? "Try again" : undefined,
    canSend: !running && !full && !over && draft.trim().length > 0,
    blocked,
    counter: `${draft.length.toLocaleString("en-US")} / ${CHAT_MAX_CHARS.toLocaleString("en-US")}`,
    over,
  };
}

/**
 * A turn just finished: the thread was running and no longer is. The board
 * then reloads, so suggestions the reply added appear under the note.
 */
export function settled(before: ChatThread | undefined, after: ChatThread | undefined): boolean {
  return before?.status === "running" && after != null && after.status !== "running";
}

/** "12 of 200 messages left this month", under plan chat's box. */
export function allowanceLine(allowance: Pick<AiPlanChat, "remaining" | "per_month">): string {
  const left = allowance.remaining.toLocaleString("en-US");
  return `${left} of ${allowance.per_month.toLocaleString("en-US")} messages left this month`;
}

/** The DOM id a card is rendered with, for "Added a suggestion ↓" to scroll to. */
export function cardAnchor(suggestionId: number): string {
  return `suggestion-${suggestionId}`;
}
