"use client";

import { useEffect, useRef } from "react";
import { SegmentedControl, Tag } from "@/components/ui";
import type { SuggestionKind } from "@/lib/api/suggestions";
import {
  KIND_LABEL,
  type NoteList as NoteListView,
  type NoteRow,
  type NoteView,
  type RowStatus,
} from "@/lib/view/review";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

/** The dot beside each note: what kind of attention it asks for. */
export const KIND_DOT: Record<SuggestionKind, string> = {
  add: "var(--color-accent-2)",
  fix: "var(--color-accent)",
  check: "var(--color-neutral-500)",
  stress: "var(--color-series-2)",
  read: "var(--color-series-6)",
};

/** A handled note's dot: done with, whatever its kind. */
const HANDLED_DOT = "var(--color-neutral-300)";

const PILL: Record<RowStatus["tone"], { background: string; color: string; border: string }> = {
  done: {
    background: "var(--color-accent-100)",
    color: "var(--color-accent-700)",
    border: "color-mix(in srgb, var(--color-accent) 30%, transparent)",
  },
  started: {
    background: "var(--color-accent-100)",
    color: "var(--color-accent-700)",
    border: "color-mix(in srgb, var(--color-accent) 30%, transparent)",
  },
  dismissed: {
    background: "var(--color-neutral-100)",
    color: "var(--color-neutral-600)",
    border: "var(--color-rule)",
  },
};

const CAPS = {
  fontSize: 11,
  fontWeight: 600,
  letterSpacing: ".08em",
  textTransform: "uppercase",
} as const;

/**
 * The Review tab's note list: a view over the notes (open, handled, all),
 * kind chips that narrow it, then the notes grouped by section, each a kind,
 * a claim and its result in a line. Picking one shows it beside the list.
 */
export function NoteList({
  list,
  show,
  onShow,
  onToggleKind,
  onClearKinds,
  selected,
  onPick,
}: {
  list: NoteListView;
  show: NoteView;
  onShow: (view: NoteView) => void;
  onToggleKind: (kind: SuggestionKind) => void;
  onClearKinds: () => void;
  selected?: number;
  onPick: (id: number) => void;
}) {
  const anyKind = list.kinds.some((k) => k.on);
  return (
    <aside aria-label="Notes" className="review-notes-list">
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 10,
          padding: "12px 16px",
          borderBottom: "1px solid var(--color-divider)",
        }}
      >
        <div className="review-notes-views">
          <SegmentedControl
            ariaLabel="Show notes"
            options={list.views.map((v) => ({ value: v.id, label: v.label, count: v.count }))}
            value={show}
            onChange={onShow}
          />
        </div>
        <div role="group" aria-label="Filter by kind" style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
          {list.kinds.map((k) => (
            <button
              key={k.id}
              type="button"
              aria-pressed={k.on}
              onClick={() => onToggleKind(k.id)}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "3px 10px",
                borderRadius: "var(--radius-pill)",
                font: "inherit",
                fontSize: 13,
                cursor: "pointer",
                opacity: k.count > 0 || k.on ? 1 : 0.45,
                background: k.on ? "var(--color-accent-200)" : "var(--color-raised)",
                color: k.on ? "var(--color-accent-700)" : "var(--color-text)",
                border: `1px solid ${k.on ? "color-mix(in srgb, var(--color-accent) 35%, transparent)" : "var(--color-divider)"}`,
              }}
            >
              <i aria-hidden style={{ width: 7, height: 7, borderRadius: "50%", background: KIND_DOT[k.id] }} />
              {k.label}
              <span style={{ color: MUTED }}>{k.count}</span>
            </button>
          ))}
          {anyKind && (
            <button
              type="button"
              onClick={onClearKinds}
              style={{
                marginLeft: "auto",
                padding: 0,
                border: 0,
                background: "none",
                font: "inherit",
                fontSize: 13,
                fontWeight: 600,
                color: "var(--color-accent-700)",
                cursor: "pointer",
              }}
            >
              Clear
            </button>
          )}
        </div>
      </div>
      {list.empty && (
        <p style={{ margin: 0, padding: "32px 16px", fontSize: 14, color: MUTED, textAlign: "center" }}>{list.empty}</p>
      )}
      {list.groups.map((group) => (
        <section key={group.key} aria-label={group.heading ?? "Handled notes"}>
          {group.heading && (
            <h2
              style={{
                ...CAPS,
                margin: 0,
                padding: "14px 16px 6px",
                fontFamily: "var(--font-body)",
                letterSpacing: ".09em",
                color: MUTED,
              }}
            >
              {group.heading}
            </h2>
          )}
          <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
            {group.rows.map((row) => (
              <NoteListRow key={row.card.id} row={row} selected={row.card.id === selected} onPick={onPick} />
            ))}
          </ul>
        </section>
      ))}
    </aside>
  );
}

function NoteListRow({ row, selected, onPick }: { row: NoteRow; selected: boolean; onPick: (id: number) => void }) {
  const { card } = row;
  const ref = useRef<HTMLButtonElement>(null);
  // A pick from the keyboard can land below the fold.
  useEffect(() => {
    if (selected) ref.current?.scrollIntoView({ block: "nearest" });
  }, [selected]);
  return (
    <li>
      <button
        ref={ref}
        type="button"
        aria-current={selected ? "true" : undefined}
        onClick={() => onPick(card.id)}
        className="review-note-row"
        style={{
          display: "grid",
          gridTemplateColumns: "8px minmax(0, 1fr)",
          gap: 10,
          width: "100%",
          padding: `11px 16px 12px ${13 + row.depth * 16}px`,
          border: 0,
          borderLeft: `3px solid ${selected ? "var(--color-accent)" : "transparent"}`,
          borderBottom: "1px solid var(--color-rule)",
          background: selected ? "var(--color-accent-100)" : "transparent",
          color: "inherit",
          font: "inherit",
          textAlign: "left",
          cursor: "pointer",
        }}
      >
        <i
          aria-hidden
          style={{
            width: 8,
            height: 8,
            marginTop: 7,
            borderRadius: "50%",
            background: row.closed ? HANDLED_DOT : KIND_DOT[card.kind],
          }}
        />
        <span style={{ display: "flex", flexDirection: "column", gap: 3, minWidth: 0 }}>
          <span style={{ ...CAPS, display: "flex", alignItems: "center", gap: 8, color: "var(--color-accent-700)" }}>
            {row.depth > 0 && <span style={{ color: MUTED }}>From the chat ·</span>}
            {KIND_LABEL[card.kind]}
            {card.source && <Tag tone="outline">{card.source}</Tag>}
            {row.status && (
              <span
                style={{
                  marginLeft: "auto",
                  padding: "1px 8px",
                  borderRadius: "var(--radius-pill)",
                  fontSize: 11.5,
                  fontWeight: 500,
                  letterSpacing: ".02em",
                  textTransform: "none",
                  whiteSpace: "nowrap",
                  background: PILL[row.status.tone].background,
                  color: PILL[row.status.tone].color,
                  border: `1px solid ${PILL[row.status.tone].border}`,
                }}
              >
                {row.status.label}
                {row.status.date && <span style={{ opacity: 0.75 }}> · {row.status.date}</span>}
              </span>
            )}
          </span>
          <span
            style={{
              fontFamily: "var(--font-display)",
              fontWeight: 600,
              fontSize: 16,
              lineHeight: 1.25,
              textWrap: "pretty",
              color:
                row.status?.tone === "dismissed"
                  ? "var(--color-neutral-600)"
                  : row.closed
                    ? "var(--color-neutral-700)"
                    : undefined,
              textDecoration: row.status?.tone === "dismissed" ? "line-through" : undefined,
              textDecorationColor: "color-mix(in srgb, var(--color-text) 35%, transparent)",
            }}
          >
            {card.title}
          </span>
          {card.delta && (
            <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--color-neutral-700)" }}>
              {card.delta}
            </span>
          )}
        </span>
      </button>
    </li>
  );
}
