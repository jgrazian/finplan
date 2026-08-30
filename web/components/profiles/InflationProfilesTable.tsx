"use client";

import { Table, Tag, Td, Th } from "@/components/ui";
import type { InflationProfile } from "@/lib/types";
import { pct } from "./distribution";

const MONO = { fontFamily: "ui-monospace, Menlo, monospace", fontSize: 12 } as const;

/**
 * One inflation profile is active per scenario; the rest stay as variants for
 * the What-if panel on Results. The trailing cell is the switch between them.
 */
export function InflationProfilesTable({
  profiles,
  activeId,
  onActivate,
}: {
  profiles: InflationProfile[];
  activeId: string;
  onActivate: (id: string) => void;
}) {
  return (
    <Table>
      <thead>
        <tr>
          <Th>Profile</Th>
          <Th>Distribution</Th>
          <Th align="right">Mean</Th>
          <Th align="right">Spread</Th>
          <Th>Source</Th>
          <Th align="right" />
        </tr>
      </thead>
      <tbody>
        {profiles.map((q) => {
          const active = q.id === activeId;
          return (
            <tr key={q.id}>
              <Td style={MONO}>{q.id}</Td>
              <Td>
                <Tag tone="accent">{q.kind}</Tag>
              </Td>
              <Td align="right" style={{ fontSize: 13 }}>
                {pct(q.mean)}
              </Td>
              <Td align="right" style={{ fontSize: 13 }}>
                {q.sd === 0 ? "fixed" : `± ${pct(q.sd)}`}
              </Td>
              <Td style={{ fontSize: 12 }} muted>
                {q.note}
              </Td>
              <Td align="right">
                {active ? (
                  <Tag tone="accent">in use</Tag>
                ) : (
                  <button
                    type="button"
                    onClick={() => onActivate(q.id)}
                    className="tag tag-outline"
                    style={{ background: "transparent", cursor: "pointer" }}
                  >
                    select
                  </button>
                )}
              </Td>
            </tr>
          );
        })}
      </tbody>
    </Table>
  );
}
