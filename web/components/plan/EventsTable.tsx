"use client";

import { Table, Tag, Td, Th, rowStyle } from "@/components/ui";
import type { EventId, PlanEvent } from "@/lib/types";

const MONO = {
  fontFamily: "ui-monospace, Menlo, monospace",
  fontSize: 12,
} as const;

export function EventsTable({
  events,
  selectedId,
  onSelect,
}: {
  events: PlanEvent[];
  selectedId: EventId;
  onSelect: (id: EventId) => void;
}) {
  return (
    <Table>
      <thead>
        <tr>
          <Th>Event</Th>
          <Th>Trigger</Th>
          <Th>Effects</Th>
          <Th align="right">Amount</Th>
        </tr>
      </thead>
      <tbody>
        {events.map((e) => {
          const selected = e.id === selectedId;
          return (
            <tr
              key={e.id}
              className="rowsel"
              style={rowStyle(selected)}
              aria-selected={selected}
              onClick={() => onSelect(e.id)}
            >
              <Td style={MONO}>{e.id}</Td>
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
  );
}
