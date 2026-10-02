"use client";

import type React from "react";
import { useState } from "react";
import { Blueprint, Tag } from "@/components/ui";
import { CHAT_HIDE, CHAT_PROMPT } from "@/lib/view/chat";
import {
  ACTION_LABEL,
  type CardAction,
  type Card,
  type CheckLine,
  type Destination,
  type DiffRow,
  refusalHeading,
} from "@/lib/view/review";
import { SuggestionChat } from "./SuggestionChat";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";
const FAINT = "color-mix(in srgb, var(--color-text) 55%, transparent)";

/** What happened the last time this card was acted on, shown under it. */
export interface CardOutcome {
  /** A preview's result, in the card's own check-line words. */
  preview?: CheckLine;
  /** Why the last apply or preview was refused, one sentence each. */
  problems?: string[];
  /** The refusal was the plan moving since the note was written. */
  stale?: boolean;
  /** ...and the move was another note applied from this board. */
  conflict?: boolean;
  /** Any other failure. */
  error?: string;
  /** A copy was made: its row id, for "Open it". */
  copyId?: number;
}

const MONO: React.CSSProperties = {
  font: "12px/1.55 var(--font-mono)",
  border: "1px solid var(--color-divider)",
  padding: "8px 10px",
  display: "flex",
  flexDirection: "column",
  overflowWrap: "anywhere",
};

/** The server's diff lines, additions and removals marked as such. */
export function DiffRows({ rows }: { rows: DiffRow[] }) {
  return (
    <>
      {rows.map((row, i) => (
        <div key={`${row.label}:${i}`} style={{ marginTop: i > 0 ? 6 : 0 }}>
          <div style={{ color: FAINT }}>
            {row.label}
            {row.change !== "edit" && (
              <span
                style={{
                  marginLeft: 6,
                  fontSize: 10,
                  letterSpacing: ".06em",
                  textTransform: "uppercase",
                  color: row.change === "add" ? "var(--color-accent-800)" : FAINT,
                }}
              >
                {row.change === "add" ? "added" : "removed"}
              </span>
            )}
          </div>
          {row.from != null && (
            <div style={row.change === "remove" ? { textDecoration: "line-through", color: FAINT } : undefined}>
              − {row.from}
            </div>
          )}
          {row.to != null && <div style={{ color: "var(--color-accent-800)" }}>+ {row.to}</div>}
        </div>
      ))}
    </>
  );
}

/**
 * One review note (design 14a): kicker, claim, reasoning, the server's diff
 * of what applying it changes, and what that did when simulated. A note with
 * several paths lists them as a picker; a path of several steps lists them in
 * order, each applyable once the one before it is. The check covers the
 * selected path as a whole.
 */
export function SuggestionCard({
  card,
  busy,
  outcome,
  onSelect,
  onAction,
  onNavigate,
  onReviewAgain,
  onOpenCopy,
  offline = false,
  onChatSettled,
  labels,
  badge,
  waiting,
}: {
  card: Card;
  busy: boolean;
  outcome?: CardOutcome;
  /** Pick one of the note's paths; the actions then act on it. */
  onSelect: (path: string) => void;
  /** `step` with `apply-step`: apply the path's steps through that one. */
  onAction: (action: CardAction, step?: string) => void;
  onNavigate: (to: Destination) => void;
  onReviewAgain: () => void;
  onOpenCopy: (scenarioId: number) => void;
  offline?: boolean;
  /** A chat reply finished: reload the board, for any suggestion it added. */
  onChatSettled?: () => void;
  /** Button words that replace `ACTION_LABEL`'s, per action: a draft's "Add to draft". */
  labels?: Partial<Record<CardAction, string>>;
  /** A mark beside the kicker, e.g. "Added" for a note in a draft. */
  badge?: string;
  /** Why the note cannot be acted on yet, e.g. "waiting on question 1". */
  waiting?: string;
}) {
  const label = (action: CardAction) => labels?.[action] ?? ACTION_LABEL[action];
  const [chatOpen, setChatOpen] = useState(false);
  const primary = card.actions.find((a) => a === "apply" || a === "apply-path" || a === "apply-copy");
  const secondary = card.actions.filter((a) => a !== primary);
  // A preview is simulated against the reviewed run too, so it keeps the basis.
  const check = outcome?.preview ? { ...outcome.preview, basis: card.check?.basis } : card.check;

  return (
    <Blueprint
      className="card"
      style={{ padding: 14, display: "flex", flexDirection: "column", gap: 8 }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <div className="card-kicker">{card.kicker}</div>
        {(card.source || badge) && (
          <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
            {badge && <Tag tone="accent">{badge}</Tag>}
            {card.source && <Tag tone="outline">{card.source}</Tag>}
          </span>
        )}
      </div>
      {waiting && <div style={{ fontSize: 12, fontStyle: "italic", color: MUTED }}>{waiting}</div>}
      {card.applied && (
        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--color-accent-800)" }}>{card.applied}</div>
      )}
      <div className="card-title" style={{ fontSize: 17, textWrap: "pretty" }}>
        {card.title}
      </div>
      <div className="card-body" style={{ fontSize: 12.5, textWrap: "pretty" }}>
        {card.summary}
        {card.more && ` ${card.more}`}
      </div>
      {card.chat === "prominent" && !chatOpen && (
        <p style={{ margin: 0, fontSize: 12.5, display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
          <span style={{ color: MUTED }}>{CHAT_PROMPT.prominent.lead}</span>
          <button className="sbtn" type="button" aria-expanded={false} onClick={() => setChatOpen(true)}>
            {CHAT_PROMPT.prominent.label}
          </button>
        </p>
      )}

      <PathPicker card={card} busy={busy} onSelect={onSelect} />

      <StepList card={card} busy={busy} onAction={onAction} />

      {(card.diff.length > 0 || check) && (
        <div style={MONO}>
          <DiffRows rows={card.diff} />
          {check && (
            <div style={{ color: FAINT, marginTop: card.diff.length > 0 ? 6 : 0 }}>
              {check.text}
              {!check.simulated && " · not simulated yet"}
            </div>
          )}
          {check?.basis && <div style={{ color: FAINT, fontSize: 11 }}>{check.basis}</div>}
          {check?.caveat && <div style={{ color: FAINT, fontSize: 11 }}>{check.caveat}</div>}
        </div>
      )}

      {card.evidence.length > 0 && (
        <div style={{ display: "flex", flexWrap: "wrap", gap: "4px 12px", fontSize: 12 }}>
          {card.evidence.map((link, i) =>
            link.to ? (
              <a
                key={`${link.label}:${i}`}
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  onNavigate(link.to!);
                }}
              >
                {link.label} →
              </a>
            ) : (
              <span key={`${link.label}:${i}`} style={{ color: MUTED }}>
                {link.label}
              </span>
            ),
          )}
        </div>
      )}

      <OutcomeNotes outcome={outcome} onReviewAgain={onReviewAgain} onOpenCopy={onOpenCopy} />

      {(card.actions.length > 0 || (card.chat && (card.chat === "quiet" || chatOpen))) && (
        <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
          {primary && (
            <button
              className="btn btn-primary blueprint"
              type="button"
              style={{ fontSize: 12 }}
              disabled={busy}
              onClick={() => onAction(primary)}
            >
              <i className="corner tl" />
              <i className="corner tr" />
              <i className="corner bl" />
              <i className="corner br" />
              {label(primary)}
            </button>
          )}
          {secondary.map((action) => (
            <button
              key={action}
              className="btn btn-ghost"
              type="button"
              style={{ fontSize: 12 }}
              disabled={busy}
              onClick={() => onAction(action)}
            >
              {label(action)}
            </button>
          ))}
          {card.chat && (card.chat === "quiet" || chatOpen) && (
            <button
              className="btn btn-ghost"
              type="button"
              style={{ fontSize: 12, marginLeft: "auto" }}
              aria-expanded={chatOpen}
              onClick={() => setChatOpen((open) => !open)}
            >
              {chatOpen ? CHAT_HIDE : CHAT_PROMPT.quiet.label}
            </button>
          )}
        </div>
      )}
      {card.chat && chatOpen && (
        <SuggestionChat suggestionId={card.id} offline={offline} onSettled={onChatSettled ?? (() => undefined)} />
      )}
    </Blueprint>
  );
}

/**
 * The paths to pick between, when a note offers several, then the picked
 * one's label and reasoning.
 */
export function PathPicker({
  card,
  busy,
  onSelect,
}: {
  card: Card;
  busy: boolean;
  onSelect: (path: string) => void;
}) {
  return (
    <>
      {card.paths.length > 0 && (
        <fieldset style={{ border: 0, margin: 0, padding: 0, minWidth: 0, display: "flex", flexDirection: "column", gap: 4 }}>
          <legend
            style={{
              padding: 0,
              marginBottom: 4,
              fontSize: 10.5,
              fontWeight: 600,
              letterSpacing: ".06em",
              textTransform: "uppercase",
              color: FAINT,
            }}
          >
            {card.paths.length} paths
          </legend>
          {card.paths.map((path) => (
            <label
              key={path.key}
              style={{
                display: "flex",
                flexWrap: "wrap",
                alignItems: "center",
                gap: "4px 8px",
                padding: "6px 8px",
                fontSize: 12.5,
                cursor: busy || path.locked ? "default" : "pointer",
                opacity: path.locked ? 0.55 : 1,
                border: `1px solid ${path.selected ? "var(--color-accent-700)" : "var(--color-divider)"}`,
                background: path.selected ? "var(--color-accent-100)" : "transparent",
              }}
            >
              <input
                type="radio"
                name={`suggestion-${card.id}-path`}
                value={path.key}
                checked={path.selected}
                disabled={busy || path.locked}
                onChange={() => onSelect(path.key)}
                style={{ margin: 0, flex: "none", accentColor: "var(--color-accent-700)" }}
              />
              <span style={{ flex: "1 1 120px", minWidth: 0, fontWeight: path.selected ? 600 : 400 }}>
                {path.label}
                {path.steps && <span style={{ fontWeight: 400, color: FAINT }}> · {path.steps}</span>}
              </span>
              {path.recommended && <Tag tone="outline">Recommended</Tag>}
              {path.headline && (
                <span style={{ marginLeft: "auto", fontSize: 11.5, color: FAINT, whiteSpace: "nowrap" }}>
                  {path.headline}
                </span>
              )}
            </label>
          ))}
          {card.locked && <div style={{ fontSize: 11.5, color: FAINT }}>{card.locked}</div>}
        </fieldset>
      )}
      {card.pathLabel && card.paths.length === 0 && (
        <div style={{ fontSize: 12.5, fontWeight: 600 }}>{card.pathLabel}</div>
      )}
      {card.pathReasoning && (
        <div className="card-body" style={{ fontSize: 12.5, textWrap: "pretty" }}>
          {card.pathReasoning}
        </div>
      )}
    </>
  );
}

/** The selected path's steps, each with its diff and, for the next one, its Apply. */
export function StepList({
  card,
  busy,
  onAction,
}: {
  card: Card;
  busy: boolean;
  onAction: (action: CardAction, step?: string) => void;
}) {
  if (card.steps.length === 0) return null;
  return (
    <ol aria-label="Steps" style={{ margin: 0, padding: 0, listStyle: "none", display: "flex", flexDirection: "column", gap: 8 }}>
      {card.steps.map((step) => (
        <li key={step.key} style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "baseline", gap: "4px 10px", fontSize: 12.5 }}>
            <span style={{ flex: "1 1 160px", minWidth: 0, fontWeight: 600 }}>
              {step.number}. {step.title}
            </span>
            {step.status && (
              <span style={{ fontSize: 12, fontWeight: 600, color: "var(--color-accent-800)" }}>{step.status}</span>
            )}
            {step.apply && (
              <button
                className="btn btn-ghost"
                type="button"
                style={{ fontSize: 12 }}
                disabled={busy}
                aria-label={`Apply step ${step.number}: ${step.title}`}
                onClick={() => onAction("apply-step", step.key)}
              >
                {ACTION_LABEL["apply-step"]}
              </button>
            )}
            {step.after != null && <span style={{ fontSize: 11.5, color: FAINT }}>after {step.after}</span>}
          </div>
          {step.reasoning && (
            <div className="card-body" style={{ fontSize: 12, textWrap: "pretty" }}>
              {step.reasoning}
            </div>
          )}
          {step.diff.length > 0 && (
            <div style={{ ...MONO, opacity: step.applied ? 0.7 : 1 }}>
              <DiffRows rows={step.diff} />
            </div>
          )}
        </li>
      ))}
    </ol>
  );
}

/** What happened when the note was last acted on: a refusal, an error, a copy made. */
export function OutcomeNotes({
  outcome,
  onReviewAgain,
  onOpenCopy,
}: {
  outcome?: CardOutcome;
  onReviewAgain: () => void;
  onOpenCopy: (scenarioId: number) => void;
}) {
  return (
    <>
      {outcome?.problems && outcome.problems.length > 0 && (
        <div
          role="alert"
          style={{
            fontSize: 12,
            padding: "8px 10px",
            background: "var(--color-accent-100)",
            borderLeft: "3px solid var(--color-accent-700)",
            color: "var(--color-accent-900)",
            display: "flex",
            flexDirection: "column",
            gap: 4,
          }}
        >
          <b>{refusalHeading(outcome)}</b>
          {outcome.problems.map((text, i) => (
            <span key={i}>{text}</span>
          ))}
          <div>
            <button className="sbtn" type="button" onClick={onReviewAgain}>
              Review again
            </button>
          </div>
        </div>
      )}
      {outcome?.error && (
        <p role="alert" style={{ margin: 0, fontSize: 12, color: "var(--color-accent-800)" }}>
          {outcome.error}
        </p>
      )}
      {outcome?.copyId != null && (
        <div style={{ fontSize: 12, display: "flex", gap: 8, alignItems: "baseline" }}>
          <span>Added as a new scenario.</span>
          <a
            href="#"
            onClick={(e) => {
              e.preventDefault();
              onOpenCopy(outcome.copyId!);
            }}
          >
            Open it →
          </a>
        </div>
      )}
    </>
  );
}
