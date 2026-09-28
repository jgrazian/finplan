"use client";

import { useId, useState } from "react";
import { useSuggestionChat } from "@/lib/hooks/useSuggestionChat";
import { CHAT_MAX_CHARS, cardAnchor, lastQuestion, threadView } from "@/lib/view/chat";

const FAINT = "color-mix(in srgb, var(--color-text) 55%, transparent)";

/** Bring a card a reply added into view, and put focus on it. */
function reveal(suggestionId: number) {
  const el = document.getElementById(cardAnchor(suggestionId));
  if (!el) return;
  el.scrollIntoView({ behavior: "smooth", block: "center" });
  el.focus({ preventScroll: true });
}

/**
 * "Chat about this": the follow-up thread under a note, inline in its card.
 * Text only, as written: line breaks kept, nothing read as markup. A reply
 * that writes a suggestion links to it; the board nests it under this note
 * once the turn settles (`onSettled` reloads the board).
 */
export function SuggestionChat({
  suggestionId,
  offline,
  onSettled,
}: {
  suggestionId: number;
  offline: boolean;
  onSettled: () => void;
}) {
  const { thread, loadError, sending, sendError, send } = useSuggestionChat(suggestionId, true, onSettled);
  const [draft, setDraft] = useState("");
  const counterId = useId();
  const view = threadView(thread, { draft, sending });

  const submit = async (text: string) => {
    const message = text.trim();
    if (!message) return;
    // Keep the draft until the server takes it, so a refusal loses nothing.
    if (await send(message)) setDraft((held) => (held.trim() === message ? "" : held));
  };

  const retry = () => {
    const again = lastQuestion(thread);
    if (again) void submit(again);
  };

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 8,
        paddingTop: 8,
        borderTop: "1px solid var(--color-divider)",
      }}
    >
      {loadError && (
        <p role="alert" style={{ margin: 0, fontSize: 12, color: "var(--color-accent-800)" }}>
          Cannot load this chat: {loadError}
        </p>
      )}

      {view.lines.length > 0 && (
        <ol
          aria-label="Chat about this note"
          style={{ margin: 0, padding: 0, listStyle: "none", display: "flex", flexDirection: "column", gap: 8 }}
        >
          {view.lines.map((line) => (
            <li
              key={line.id}
              style={{
                alignSelf: line.role === "user" ? "flex-end" : "flex-start",
                maxWidth: "92%",
                padding: "6px 10px",
                fontSize: 12.5,
                lineHeight: 1.5,
                border: "1px solid var(--color-divider)",
                background: line.role === "user" ? "var(--color-accent-100)" : "transparent",
              }}
            >
              <div
                style={{
                  fontSize: 10.5,
                  fontWeight: 600,
                  letterSpacing: ".06em",
                  textTransform: "uppercase",
                  color: FAINT,
                }}
              >
                {line.speaker}
              </div>
              {/* A text child, never HTML: React escapes it; pre-wrap keeps its lines. */}
              <div style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{line.text}</div>
              {line.link && (
                <a
                  href={`#${cardAnchor(line.suggestionIds[0])}`}
                  style={{ display: "inline-block", marginTop: 4, fontSize: 12 }}
                  onClick={(e) => {
                    e.preventDefault();
                    reveal(line.suggestionIds[0]);
                  }}
                >
                  {line.link}
                </a>
              )}
            </li>
          ))}
        </ol>
      )}

      {view.status && (
        <p
          role="status"
          aria-live="polite"
          style={{
            margin: 0,
            fontSize: 12,
            color: view.state === "failed" ? "var(--color-accent-800)" : FAINT,
            fontStyle: view.state === "running" ? "italic" : undefined,
          }}
        >
          {view.status}{" "}
          {view.retry && (
            <button className="sbtn" type="button" disabled={offline} onClick={retry}>
              {view.retry}
            </button>
          )}
        </p>
      )}

      {sendError && (
        <p role="alert" style={{ margin: 0, fontSize: 12, color: "var(--color-accent-800)" }}>
          Not sent: {sendError}
        </p>
      )}

      {view.blocked ? (
        <p style={{ margin: 0, fontSize: 12, color: FAINT }}>{view.blocked}</p>
      ) : (
        <form
          style={{ display: "flex", flexDirection: "column", gap: 6 }}
          onSubmit={(e) => {
            e.preventDefault();
            if (view.canSend && !offline) void submit(draft);
          }}
        >
          <textarea
            className="input"
            rows={3}
            value={draft}
            maxLength={CHAT_MAX_CHARS}
            aria-label="Message about this note"
            aria-describedby={counterId}
            placeholder={view.state === "empty" ? "Ask about this note, or ask for a change…" : "Reply…"}
            disabled={offline}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              // Enter sends; Shift+Enter is a new line; an IME composing is left alone.
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                if (view.canSend && !offline) void submit(draft);
              }
            }}
            style={{ resize: "vertical", fontSize: 13 }}
          />
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span id={counterId} style={{ fontSize: 11, color: view.over ? "var(--color-accent-800)" : FAINT }}>
              {view.counter}
            </span>
            <span style={{ fontSize: 11, color: FAINT }}>Enter to send · Shift+Enter for a new line</span>
            <button
              className="btn btn-primary"
              type="submit"
              style={{ marginLeft: "auto", fontSize: 12 }}
              disabled={offline || !view.canSend}
            >
              {sending ? "Sending…" : "Send"}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}
