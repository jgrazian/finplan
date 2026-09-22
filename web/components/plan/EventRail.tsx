"use client";

import { Button, DragHandle, DropLine, Kbd, Tag } from "@/components/ui";
import { useReorder } from "@/lib/hooks/useReorder";
import type { EventId, PlanEvent } from "@/lib/types";
import { NO_AMOUNT } from "@/lib/view/events";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

/**
 * Artboard 10a — the plan's events as a narrow rail down the left edge.
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
  onAdd,
  onReorder,
  adding,
  offline,
}: {
  events: PlanEvent[];
  selectedId: EventId;
  onSelect: (id: EventId) => void;
  onAdd?: () => void;
  /** Server ids in their new order. Omitted where writes are refused. */
  onReorder?: (ids: number[]) => void | Promise<unknown>;
  /** A write is in flight, so Add is closed — a second click would ask for a
   *  second event under the name the first one has not claimed yet. */
  adding?: boolean;
  offline?: boolean;
}) {
  const byServerId = new Map(events.map((e) => [e.serverId, e]));
  // Destructured rather than kept as one object: a `ref` prop taken off a
  // value marks the whole value as a ref to the React compiler, and the rest of
  // what the hook returns is ordinary render state.
  const { order, attachList, attachRow, dragging, indicator, handleProps, listStyle } =
    useReorder({ keys: events.map((e) => e.serverId), onReorder });

  return (
    <div style={{ display: "flex", flexDirection: "column", minWidth: 0, height: "100%" }}>
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          justifyContent: "space-between",
          gap: 8,
          padding: "14px 16px 8px",
        }}
      >
        <h4 style={{ margin: 0 }}>
          Events{" "}
          <span className="text-muted" style={{ fontSize: 13 }}>
            {events.length}
          </span>
        </h4>
        <Button
          variant="ghost"
          shortcut="a"
          onClick={onAdd}
          disabled={offline || adding}
          title={offline ? "No connection to the server." : undefined}
        >
          Add
        </Button>
      </div>

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
              className="rowsel griprow"
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
                ...(selected
                  ? {
                      background: "color-mix(in srgb, var(--color-accent) 14%, transparent)",
                      boxShadow: "inset 3px 0 0 var(--color-accent)",
                    }
                  : null),
              }}
            >
              <DragHandle label={event.id} props={handleProps(serverId)} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div
                  style={{
                    display: "flex",
                    alignItems: "baseline",
                    justifyContent: "space-between",
                    gap: 6,
                  }}
                >
                  <span
                    style={{
                      fontFamily: "ui-monospace, Menlo, monospace",
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
                <div style={{ fontSize: 12, marginTop: 3, color: MUTED }}>
                  {event.trigger} · {event.amount === NO_AMOUNT ? event.next : event.amount}
                </div>
              </div>
            </div>
          );
        })}
      </div>
      <div style={{ borderTop: "1px solid var(--color-divider)" }} />

      <div
        style={{
          marginTop: "auto",
          padding: "12px 16px 14px",
          fontSize: 11.5,
          color: MUTED,
        }}
      >
        <Kbd>⠿</Kbd> reorders the list you read, not the order the engine fires
        them in.
      </div>
    </div>
  );
}

/**
 * What the event does, in one word: the kind of its first effect, or `marker`
 * for an event that only exists so others can be timed against it.
 */
function KindTag({ event }: { event: PlanEvent }) {
  const first = event.effects[0];
  if (!first) return <Tag tone="outline">marker</Tag>;
  return (
    <Tag tone="accent">
      {first.kind}
      {event.effects.length > 1 ? ` +${event.effects.length - 1}` : ""}
    </Tag>
  );
}
