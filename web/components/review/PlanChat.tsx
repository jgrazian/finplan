"use client";

import { type ReactNode, useId, useState } from "react";
import { ChatActivity } from "@/components/chat/ChatActivity";
import { ChatBubble, ChatText } from "@/components/chat/ChatBubble";
import { Button } from "@/components/ui";
import type { AiPlanChat } from "@/lib/api/suggestions";
import { usePlanChat } from "@/lib/hooks/useSuggestionChat";
import { CHAT_MAX_CHARS, allowanceLine, lastQuestion, threadView } from "@/lib/view/chat";

const EXAMPLES = ["Retire at 62 instead", "Add a $40,000 car purchase in 2029", "My 401(k) is now $310,000"];

/**
 * Plan chat (Review › Chat): a conversation about the whole plan, in the
 * look of Describe & upload. The model answers and may propose changes; each
 * comes back as a note, drawn as its card under the reply that wrote it, to
 * apply, adjust or dismiss like any other. Nothing here edits the plan.
 */
export function PlanChat({
  scenarioId,
  allowance,
  offline,
  renderNote,
  onSettled,
  onSpent,
}: {
  scenarioId: number;
  /** This month's messages; each send spends one. */
  allowance: AiPlanChat;
  offline: boolean;
  /** The card for a note a reply added, or what became of it. */
  renderNote: (suggestionId: number) => ReactNode;
  /** A turn finished: reload the board so its notes are there. */
  onSettled: () => void;
  /** A message was taken: reload the allowance. */
  onSpent: () => void;
}) {
  const chat = usePlanChat(scenarioId, onSettled);
  const [draft, setDraft] = useState("");
  const counterId = useId();
  const view = threadView(chat.thread, {
    draft,
    sending: chat.sending,
    maxMessages: null,
    allowance,
  });
  const busy = offline || chat.clearing;

  const submit = async (text: string) => {
    const message = text.trim();
    if (!message) return;
    // Keep the draft until the server takes it, so a refusal loses nothing.
    if (await chat.send(message)) {
      setDraft((held) => (held.trim() === message ? "" : held));
      onSpent();
    }
  };

  const retry = () => {
    const again = lastQuestion(chat.thread);
    if (again) void submit(again);
  };

  return (
    <section
      aria-label="Plan chat"
      style={{ display: "flex", flexDirection: "column", maxWidth: 860, width: "100%", margin: "0 auto" }}
    >
      <ol className="ns-thread" aria-label="Conversation" style={{ margin: 0, listStyle: "none" }}>
        <ChatBubble as="li" from="finplan">
          <ChatText>
            {
              "Ask me about this plan, or tell me what to change. I read the plan and its reviewed run, and when a change fits I write it as a note under my reply: you apply it, adjust it or dismiss it. I never change the plan myself."
            }
          </ChatText>
          {view.state === "empty" && (
            <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
              {EXAMPLES.map((example) => (
                <button
                  key={example}
                  type="button"
                  className="sbtn"
                  disabled={busy}
                  onClick={() => setDraft(example)}
                >
                  {example}
                </button>
              ))}
            </div>
          )}
        </ChatBubble>

        {chat.loadError && (
          <li role="alert" style={{ fontSize: 12.5, color: "var(--color-accent-800)" }}>
            Cannot load this chat: {chat.loadError}
          </li>
        )}

        {view.lines.map((line) => (
          <li key={line.id} style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <ChatBubble from={line.role === "user" ? "you" : "finplan"}>
              <ChatText>{line.text}</ChatText>
            </ChatBubble>
            {line.suggestionIds.length > 0 && (
              <div
                aria-label="Proposed changes"
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 10,
                  marginLeft: 14,
                  paddingLeft: 12,
                  borderLeft: "2px solid var(--color-divider)",
                }}
              >
                <span className="ns-lbl">
                  {line.suggestionIds.length === 1 ? "Proposed change" : "Proposed changes"} · not applied until you
                  apply {line.suggestionIds.length === 1 ? "it" : "them"}
                </span>
                {line.suggestionIds.map((id) => (
                  <div key={id}>{renderNote(id)}</div>
                ))}
              </div>
            )}
          </li>
        ))}

        {view.activity.length > 0 ? (
          <li>
            <ChatActivity lines={view.activity} />
          </li>
        ) : view.status && (
          <li
            role="status"
            aria-live="polite"
            className="ns-mut"
            style={{
              fontSize: 12.5,
              fontStyle: view.state === "running" ? "italic" : undefined,
              color: view.state === "failed" ? "var(--color-accent-800)" : undefined,
            }}
          >
            {view.status}{" "}
            {view.retry && (
              <button className="sbtn" type="button" disabled={busy} onClick={retry}>
                {view.retry}
              </button>
            )}
          </li>
        )}
      </ol>

      <form
        className="ns-composer"
        aria-label="Message about this plan"
        onSubmit={(e) => {
          e.preventDefault();
          if (view.canSend && !busy) void submit(draft);
        }}
      >
        {view.blocked ? (
          <p className="ns-mut" style={{ margin: 0, fontSize: 12.5 }}>
            {view.blocked}
          </p>
        ) : (
          <textarea
            className="input"
            rows={3}
            value={draft}
            maxLength={CHAT_MAX_CHARS}
            aria-label="Message"
            aria-describedby={counterId}
            placeholder={
              view.state === "running"
                ? "Thinking… you can write again when the reply lands."
                : "Ask a question, or describe a change to the plan…"
            }
            disabled={busy}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              // Enter sends; Shift+Enter is a new line; an IME composing is left alone.
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                if (view.canSend && !busy) void submit(draft);
              }
            }}
            style={{ resize: "vertical", fontSize: 13 }}
          />
        )}
        {chat.sendError && (
          <p role="alert" style={{ margin: 0, fontSize: 12, color: "var(--color-accent-800)" }}>
            Not sent: {chat.sendError}
          </p>
        )}
        <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 10 }}>
          <span id={counterId} className="ns-mut" style={{ fontSize: 11.5 }}>
            {allowanceLine(allowance)}
            {!view.blocked && (
              <>
                {" · "}
                <span style={{ color: view.over ? "var(--color-accent-800)" : undefined }}>{view.counter}</span>
              </>
            )}
          </span>
          {view.lines.length > 0 && (
            <Button
              variant="ghost"
              disabled={busy || view.state === "running"}
              title="Clear the conversation. Notes it added stay on the review."
              onClick={() => void chat.clear()}
            >
              {chat.clearing ? "Clearing…" : "Start over"}
            </Button>
          )}
          <Button type="submit" variant="primary" style={{ marginLeft: "auto" }} disabled={busy || !view.canSend}>
            {chat.sending ? "Sending…" : "Send"}
          </Button>
        </div>
      </form>
    </section>
  );
}
