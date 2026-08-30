import { SectionHeading, Tag } from "@/components/ui";
import type { EventId } from "@/lib/types";

/** Events that read or write this account — the reverse dependency edge. */
export function ReferencedBy({ eventIds }: { eventIds: EventId[] }) {
  return (
    <div>
      <SectionHeading className="mb-[6px]">Referenced by</SectionHeading>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
        {eventIds.length === 0 ? (
          <span className="text-muted" style={{ fontSize: 12 }}>
            Not referenced by any event.
          </span>
        ) : (
          eventIds.map((id) => (
            <Tag key={id} tone="neutral">
              {id}
            </Tag>
          ))
        )}
      </div>
    </div>
  );
}
