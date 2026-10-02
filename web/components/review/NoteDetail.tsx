"use client";

import { useState } from "react";
import { Button, Tag } from "@/components/ui";
import { useSuggestionChat } from "@/lib/hooks/useSuggestionChat";
import { CHAT_HIDE, CHAT_PROMPT } from "@/lib/view/chat";
import {
  type Card,
  type CardAction,
  closedText,
  type Destination,
  detailActions,
  KIND_LABEL,
  SECTIONS,
  undoable,
} from "@/lib/view/review";
import { type CardOutcome, DiffRows, OutcomeNotes, PathPicker, StepList } from "./SuggestionCard";
import { SuggestionChat } from "./SuggestionChat";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

const CAPS = {
  fontSize: 11,
  fontWeight: 600,
  letterSpacing: ".09em",
  textTransform: "uppercase",
  color: MUTED,
} as const;

const LINK_BUTTON = {
  padding: 0,
  border: 0,
  background: "none",
  font: "inherit",
  fontWeight: 600,
  color: "var(--color-accent-700)",
  cursor: "pointer",
} as const;

/**
 * One review note in full, beside the list: its claim, what the selected
 * path does to the run figure by figure, the change itself, the reasoning
 * and evidence behind a disclosure, and what can be done about it.
 */
export function NoteDetail({
  card,
  position,
  busy,
  offline,
  outcome,
  onSelect,
  onAction,
  onUndo,
  onNext,
  onNavigate,
  onReviewAgain,
  onOpenCopy,
  onChatSettled,
}: {
  card: Card;
  /** "6 of 9". */
  position: string;
  busy: boolean;
  offline: boolean;
  outcome?: CardOutcome;
  onSelect: (path: string) => void;
  onAction: (action: CardAction, step?: string) => void;
  /** Take back a dismissal or an "it's correct". */
  onUndo: () => void;
  onNext: () => void;
  onNavigate: (to: Destination) => void;
  onReviewAgain: () => void;
  onOpenCopy: (scenarioId: number) => void;
  onChatSettled: () => void;
}) {
  // Both start afresh for each note: the detail is keyed by it.
  const [why, setWhy] = useState(false);
  // The note's thread is read as soon as the note is shown, so a conversation
  // already had about it opens with it rather than hiding behind the link.
  const chat = useSuggestionChat(card.id, card.chat != null, onChatSettled);
  const [chatToggled, setChatToggled] = useState<boolean>();
  const chatOpen = chatToggled ?? (chat.thread?.messages.length ?? 0) > 0;
  const section = SECTIONS.find((s) => s.id === card.section)?.heading;
  const { primary, secondary, dismiss } = detailActions(card);
  const closed = closedText(card);
  const check = outcome?.preview ? { ...outcome.preview, basis: card.check?.basis } : card.check;
  const reasoning = [card.more, card.pathReasoning && card.paths.length === 0 ? card.pathReasoning : undefined].filter(
    (text): text is string => !!text,
  );
  const metrics = card.metrics;
  // A figure already in the strip is not repeated as a plain evidence chip.
  const shown = new Set(metrics.map((m) => `${m.label}: ${m.to}`));
  const evidence = card.evidence.filter((link) => link.to != null || !shown.has(link.label));

  return (
    <article aria-label={card.title} className="review-note-detail">
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          fontSize: 12,
          fontWeight: 600,
          letterSpacing: ".09em",
          textTransform: "uppercase",
          color: "var(--color-accent-700)",
        }}
      >
        <span>
          {KIND_LABEL[card.kind]}
          {section && ` · ${section}`}
        </span>
        {card.source && <Tag tone="outline">{card.source}</Tag>}
        <span style={{ marginLeft: "auto", color: MUTED, fontWeight: 500, letterSpacing: ".02em", textTransform: "none" }}>
          {position}
        </span>
      </div>

      <h1
        style={{
          margin: 0,
          fontFamily: "var(--font-display)",
          fontWeight: 600,
          fontSize: 30,
          lineHeight: 1.15,
          letterSpacing: "-.015em",
          textWrap: "pretty",
        }}
      >
        {card.title}
      </h1>
      {card.applied && card.status === "open" && (
        <div style={{ fontSize: 13, fontWeight: 600, color: "var(--color-accent-700)" }}>{card.applied}</div>
      )}
      <p style={{ margin: 0, fontSize: 16, lineHeight: 1.55, maxWidth: 640, textWrap: "pretty" }}>{card.summary}</p>

      {metrics.length > 0 && (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(170px, 1fr))",
            border: "1px solid var(--color-divider)",
            borderRadius: "var(--radius-lg)",
            background: "var(--color-raised)",
            overflow: "hidden",
          }}
        >
          {metrics.map((m) => (
            <div
              key={m.label}
              style={{
                padding: "14px 16px",
                borderRight: "1px solid var(--color-rule)",
                display: "flex",
                flexDirection: "column",
                gap: 4,
              }}
            >
              <span style={CAPS}>
                {m.label}
                {m.estimate && " · est."}
              </span>
              <span style={{ display: "flex", alignItems: "baseline", gap: 8, flexWrap: "wrap" }}>
                {m.from != null && (
                  <>
                    <span style={{ color: MUTED, fontSize: 14 }}>{m.from}</span>
                    <span aria-label="to" style={{ color: MUTED }}>
                      →
                    </span>
                  </>
                )}
                <span style={{ fontFamily: "var(--font-display)", fontWeight: 600, fontSize: 22 }}>{m.to}</span>
              </span>
            </div>
          ))}
        </div>
      )}
      {check && (
        <div style={{ display: "flex", flexDirection: "column", gap: 2, marginTop: -12, fontSize: 12, color: MUTED }}>
          <span>
            {check.text}
            {!check.simulated && " · not simulated yet"}
          </span>
          {check.basis && <span>{check.basis}</span>}
          {check.caveat && <span>{check.caveat}</span>}
        </div>
      )}

      {(card.paths.length > 0 || card.steps.length > 0 || card.diff.length > 0) && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={CAPS}>{card.paths.length > 0 ? "Proposed changes" : "Proposed change"}</span>
          <PathPicker card={card} busy={busy} onSelect={onSelect} />
          <StepList card={card} busy={busy} onAction={onAction} />
          {card.diff.length > 0 && (
            <div
              style={{
                padding: "12px 14px",
                background: "var(--color-raised)",
                border: "1px solid var(--color-divider)",
                borderRadius: 10,
                font: "13px/1.6 var(--font-mono)",
                overflowWrap: "anywhere",
              }}
            >
              <DiffRows rows={card.diff} />
            </div>
          )}
        </div>
      )}

      {(reasoning.length > 0 || evidence.length > 0) && (
        <div style={{ display: "flex", flexDirection: "column", borderTop: "1px solid var(--color-divider)" }}>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "baseline", gap: "4px 12px", padding: "12px 0" }}>
            {reasoning.length > 0 ? (
              <button
                type="button"
                aria-expanded={why}
                onClick={() => setWhy((open) => !open)}
                style={{ ...LINK_BUTTON, display: "flex", gap: 8, fontWeight: 500, color: "inherit" }}
              >
                <span aria-hidden style={{ fontFamily: "var(--font-mono)", color: MUTED, width: 12 }}>
                  {why ? "−" : "+"}
                </span>
                Reasoning
              </button>
            ) : (
              <span style={{ fontWeight: 500 }}>Evidence</span>
            )}
            {evidence.map((link, i) =>
              link.to ? (
                <a
                  key={`${link.label}:${i}`}
                  href="#"
                  style={{ fontSize: 13 }}
                  onClick={(e) => {
                    e.preventDefault();
                    onNavigate(link.to!);
                  }}
                >
                  {link.label} →
                </a>
              ) : (
                <span key={`${link.label}:${i}`} style={{ fontSize: 13, color: MUTED }}>
                  {link.label}
                </span>
              ),
            )}
          </div>
          {why &&
            reasoning.map((text, i) => (
              <p key={i} style={{ margin: "0 0 12px 20px", color: "var(--color-neutral-700)", maxWidth: 640, textWrap: "pretty" }}>
                {text}
              </p>
            ))}
        </div>
      )}

      <OutcomeNotes outcome={outcome} onReviewAgain={onReviewAgain} onOpenCopy={onOpenCopy} />

      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          gap: "10px 18px",
          paddingTop: 16,
          borderTop: "1px solid var(--color-divider)",
        }}
      >
        {closed ? (
          <>
            <span style={{ color: "var(--color-neutral-700)" }}>{closed}</span>
            {undoable(card) && (
              <button type="button" style={LINK_BUTTON} disabled={busy} onClick={onUndo}>
                Undo
              </button>
            )}
          </>
        ) : (
          <>
            {primary && (
              <Button variant="primary" disabled={busy} onClick={() => onAction(primary.action)}>
                {primary.label}
              </Button>
            )}
            {secondary.map((a) => (
              <Button key={a.action} variant="ghost" disabled={busy} onClick={() => onAction(a.action)}>
                {a.label}
              </Button>
            ))}
            {dismiss && (
              <button type="button" style={LINK_BUTTON} disabled={busy} onClick={() => onAction("dismiss")}>
                Dismiss
              </button>
            )}
          </>
        )}
        <span style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 18 }}>
          {card.chat && (
            <button
              type="button"
              style={LINK_BUTTON}
              aria-expanded={chatOpen}
              disabled={offline && !chatOpen}
              onClick={() => setChatToggled(!chatOpen)}
            >
              {chatOpen ? CHAT_HIDE : CHAT_PROMPT.quiet.label}
            </button>
          )}
          <Button variant="secondary" shortcut="j" onClick={onNext}>
            Next note
          </Button>
        </span>
      </div>
      {card.chat && chatOpen && (
        <SuggestionChat suggestionId={card.id} offline={offline} onSettled={onChatSettled} chat={chat} />
      )}
    </article>
  );
}
