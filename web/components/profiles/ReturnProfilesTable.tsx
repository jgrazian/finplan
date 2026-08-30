"use client";

import { Table, Tag, Td, Th, rowStyle } from "@/components/ui";
import type { ReturnProfile, ReturnProfileId } from "@/lib/types";
import { pct } from "./distribution";

const MONO = { fontFamily: "ui-monospace, Menlo, monospace", fontSize: 12 } as const;

export function ReturnProfilesTable({
  profiles,
  selectedId,
  onSelect,
}: {
  profiles: ReturnProfile[];
  selectedId: ReturnProfileId;
  onSelect: (id: ReturnProfileId) => void;
}) {
  return (
    <Table>
      <thead>
        <tr>
          <Th>Profile</Th>
          <Th>Distribution</Th>
          <Th align="right">Mean</Th>
          <Th align="right">Std dev</Th>
          <Th>Used by</Th>
        </tr>
      </thead>
      <tbody>
        {profiles.map((p) => {
          const selected = p.id === selectedId;
          return (
            <tr
              key={p.id}
              className="rowsel"
              style={rowStyle(selected)}
              aria-selected={selected}
              onClick={() => onSelect(p.id)}
            >
              <Td style={MONO}>{p.id}</Td>
              <Td>
                <Tag tone="accent">{p.kind}</Tag>
              </Td>
              <Td align="right" style={{ fontSize: 13 }}>
                {pct(p.mean)}
              </Td>
              <Td align="right" style={{ fontSize: 13 }}>
                {p.sd === 0 ? "—" : `± ${pct(p.sd)}`}
              </Td>
              <Td>
                <div style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>
                  {p.usedBy.map((u) => (
                    <Tag key={u} tone="neutral">
                      {u}
                    </Tag>
                  ))}
                </div>
              </Td>
            </tr>
          );
        })}
      </tbody>
    </Table>
  );
}
