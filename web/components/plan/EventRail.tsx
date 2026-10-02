"use client";

import { DragHandle, DropLine, Tag, rowStyle } from "@/components/ui";
import { FAMILY_LABEL, effectFamily } from "@/lib/view/effectFamily";
import { useReorder } from "@/lib/hooks/useReorder";
import type { EventId, PlanEvent } from "@/lib/types";
import { NO_AMOUNT } from "@/lib/view/events";
import { noteLink } from "@/lib/view/review";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

/**
 * Artboard 10a — the plan's events as a narrow rail down the left edge. Its
 * header — the Events | Parameters switch and Add — belongs to the screen,
 * which owns both lists (artboard 17a).
 *
 * A table needed five columns to say what two lines say here: what the event
 * is called, and — under it — when it fires and for how much. That buys the
 * editor beside it the width to put When and What side by side.
 *
 * Order is presentation, not schedule: what makes an event fire is its trigger,
 * so dragging one changes the list you read, never the plan the engine runs.
 */
export function EventRail({
  events,
  selectedId,
  onSelect,
  onReorder,
  notes,
  onOpenNotes,
}: {
  events: PlanEvent[];
  selectedId: EventId;
  onSelect: (id: EventId) => void;
  /** Open review notes that edit each event, by server id. */
  notes?: ReadonlyMap<number, number>;
  /** Opens the Review tab, from a row's "review note" link. */
  onOpenNotes?: () => void;
  /** Server ids in their new order. Omitted where writes are refused. */
  onReorder?: (ids: number[]) => void | Promise<unknown>;
}) {
  const byServerId = new Map(events.map((e) => [e.serverId, e]));
  // Destructured rather than kept as one object: a `ref` prop taken off a
  // value marks the whole value as a ref to the React compiler, and the rest of
  // what the hook returns is ordinary render state.
  const { order, attachList, attachRow, dragging, indicator, handleProps, listStyle } =
    useReorder({ keys: events.map((e) => e.serverId), onReorder });

  return (
    <div style={{ display: "flex", flexDirection: "column", minWidth: 0, height: "100%" }}>
      <div ref={attachList} style={listStyle} role="listbox" aria-label="Events">
        <DropLine at={indicator} />
        {order.map((serverId) => {
          const event = byServerId.get(serverId);
          if (!event) return null;
          const selected = event.id === selectedId;
          return (
            <div
              key={event.id}
              ref={attachRow(serverId)}
              className="rowsel griprow event-row"
              role="option"
              tabIndex={0}
              aria-selected={selected}
              onClick={() => onSelect(event.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onSelect(event.id);
                }
              }}
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 6,
                padding: "9px 16px 9px 2px",
                borderTop: "1px solid var(--color-divider)",
                opacity: dragging === serverId ? 0.5 : event.enabled ? undefined : 0.6,
                ...rowStyle(selected),
              }}
            >
              <DragHandle label={event.id} props={handleProps(serverId)} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div
                  className="event-row-head"
                  style={{
                    display: "flex",
                    alignItems: "baseline",
                    justifyContent: "space-between",
                    gap: 6,
                  }}
                >
                  <span
                    className="event-row-name"
                    style={{
                      fontFamily: "var(--font-mono)",
                      fontSize: 12.5,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {event.id}
                  </span>
                  <span style={{ display: "flex", gap: 4, flex: "none" }}>
                    {/* Disabled is not deleted, and a row that looked the same
                        either way would leave the run quietly short an event. */}
                    {!event.enabled && <Tag>off</Tag>}
                    <KindTag event={event} />
                  </span>
                </div>
                {/* When it fires, and — where there is one — for how much. An
                    event with no figure of its own is a marker other events are
                    timed against, so it says when instead. */}
                <div className="event-row-summary" style={{ fontSize: 12, marginTop: 3, color: MUTED }}>
                  {event.trigger} · {event.amount === NO_AMOUNT ? event.next : event.amount}
                </div>
                {onOpenNotes && noteLink(notes?.get(serverId)) && (
                  <a
                    href="#"
                    style={{ display: "inline-block", fontSize: 12, marginTop: 3 }}
                    onClick={(e) => {
                      // The row is a button of its own; this link is not a pick.
                      e.preventDefault();
                      e.stopPropagation();
                      onOpenNotes();
                    }}
                  >
                    {noteLink(notes?.get(serverId))}
                  </a>
                )}
              </div>
              {/* A phone opens the row as a page; the chevron says so. */}
              <span className="event-row-chev" aria-hidden="true">›</span>
            </div>
          );
        })}
      </div>
      <div style={{ borderTop: "1px solid var(--color-divider)" }} />
    </div>
  );
}

/**
 * What the event does, in one word: the kind of its first effect, or `marker`
 * for an event that only exists so others can be timed against it. The dot
 * names its family — money in, money out, moving money — so the rail can be
 * scanned by colour, with the word always beside it.
 */
function KindTag({ event }: { event: PlanEvent }) {
  const first = event.effects[0];
  const family = effectFamily(first?.kind);
  return (
    <span title={FAMILY_LABEL[family]} style={{ display: "inline-flex" }}>
      <Tag tone="neutral">
        <i className="kind-dot" data-family={family} aria-hidden />
        {first ? first.kind : "marker"}
        {event.effects.length > 1 ? ` +${event.effects.length - 1}` : ""}
      </Tag>
    </span>
  );
}
