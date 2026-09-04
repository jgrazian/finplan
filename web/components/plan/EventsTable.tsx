"use client";

import { DragHandle, DropLine, Table, Tag, Td, Th, rowStyle } from "@/components/ui";
import { useReorder } from "@/lib/hooks/useReorder";
import type { EventId, PlanEvent } from "@/lib/types";

const MONO = {
  fontFamily: "ui-monospace, Menlo, monospace",
  fontSize: 12,
} as const;

/**
 * The plan's events, in the order the user has put them in.
 *
 * Order is presentation, not schedule: what makes an event fire is its trigger,
 * so dragging one changes the list you read, never the plan the engine runs.
 */
export function EventsTable({
  events,
  selectedId,
  onSelect,
  onReorder,
}: {
  events: PlanEvent[];
  selectedId: EventId;
  onSelect: (id: EventId) => void;
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
    <div ref={attachList} style={listStyle}>
      <DropLine at={indicator} />
      <Table>
        <thead>
          <tr>
            <Th style={{ width: 22, padding: 0 }} aria-label="Order" />
            <Th>Event</Th>
            <Th>Trigger</Th>
            <Th>Effects</Th>
            <Th align="right">Amount</Th>
          </tr>
        </thead>
        <tbody>
          {order.map((serverId) => {
            const e = byServerId.get(serverId);
            if (!e) return null;
            const selected = e.id === selectedId;
            return (
              <tr
                key={e.id}
                ref={attachRow(serverId)}
                className={dragging === serverId ? "rowsel dragging" : "rowsel"}
                style={{ ...rowStyle(selected), opacity: e.enabled ? undefined : 0.55 }}
                aria-selected={selected}
                onClick={() => onSelect(e.id)}
              >
                <Td style={{ padding: 0 }}>
                  <DragHandle label={e.id} props={handleProps(serverId)} />
                </Td>
                <Td style={MONO}>
                  <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    {e.id}
                    {/* Disabled is not deleted, and a row that looked the same
                        either way would leave the run quietly short an event. */}
                    {!e.enabled && <Tag>off</Tag>}
                  </span>
                </Td>
                <Td style={{ fontSize: 13 }}>{e.trigger}</Td>
                <Td>
                  <div style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>
                    {e.effects.map((f, i) => (
                      <Tag key={`${f.kind}-${i}`} tone="accent">
                        {f.kind}
                      </Tag>
                    ))}
                  </div>
                </Td>
                <Td align="right" style={{ fontSize: 13 }}>
                  {e.amount}
                </Td>
              </tr>
            );
          })}
        </tbody>
      </Table>
    </div>
  );
}
