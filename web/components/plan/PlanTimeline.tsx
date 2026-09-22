"use client";

import { SectionHeading } from "@/components/ui";
import type { EventId, PlanEvent } from "@/lib/types";
import type { PlanAxis } from "@/lib/view/axis";
import { type Lane, axisTicks, laneOf } from "./timeline";

/** Width of the name gutter, so the lanes all start on the same rule. */
const GUTTER = 118;
const MUTED = "color-mix(in srgb, var(--color-text) 55%, transparent)";

/**
 * Artboard 10a — the plan at a glance, as a full-width band under the rail and
 * the editor rather than a strip inside the list column.
 *
 * One lane per event, named in the left gutter, so a row here answers to a row
 * up there. Spans draw as bars and one-shot triggers as points; the selected
 * event is the only one drawn at full strength, and clicking a lane selects
 * its event — a second way into the same selection the rail drives.
 */
export function PlanTimeline({
  events,
  axis,
  selectedId,
  onSelect,
}: {
  events: PlanEvent[];
  /** The scenario's own horizon — ages, or years without a birth date. */
  axis: PlanAxis;
  selectedId: EventId;
  onSelect: (id: EventId) => void;
}) {
  const ticks = axisTicks(axis.range, axis.label);

  return (
    <div
      style={{
        borderTop: "1px solid var(--color-divider)",
        padding: "10px 20px 12px",
        background: "color-mix(in srgb, var(--color-text) 3%, transparent)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          justifyContent: "space-between",
          marginBottom: 6,
        }}
      >
        <SectionHeading>Plan at a glance</SectionHeading>
        <span style={{ fontSize: 11, color: MUTED }}>
          one lane per event · click a lane to select it
        </span>
      </div>

      {/* Bounded rather than unbounded: a plan with thirty events would
          otherwise push the age axis off the bottom of the screen, and the
          axis is what makes the lanes mean anything. */}
      <div
        style={{ maxHeight: 168, overflowY: "auto" }}
        role="listbox"
        aria-label="Event lanes"
      >
        {events.map((event) => {
          const lane = laneOf(event, axis.range);
          const selected = event.id === selectedId;
          return (
            <div
              key={event.id}
              className="rowsel"
              role="option"
              tabIndex={0}
              aria-label={`Select ${event.id}`}
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
                alignItems: "center",
                gap: 12,
                padding: "3px 0",
                opacity: event.enabled ? undefined : 0.5,
              }}
            >
              <span
                style={{
                  width: GUTTER,
                  flex: "none",
                  textAlign: "right",
                  fontFamily: "ui-monospace, Menlo, monospace",
                  fontSize: 11,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  color: selected ? "var(--color-accent-800)" : MUTED,
                }}
              >
                {event.id}
              </span>
              <span style={{ position: "relative", flex: 1, height: 10 }}>
                <Mark lane={lane} selected={selected} />
              </span>
            </div>
          );
        })}
      </div>

      <div style={{ display: "flex", gap: 12, marginTop: 4 }}>
        <span style={{ width: GUTTER, flex: "none" }} />
        <span
          style={{
            flex: 1,
            display: "flex",
            justifyContent: "space-between",
            fontSize: 11,
            color: MUTED,
          }}
        >
          {ticks.map((t) => (
            <span key={t.position}>{t.label}</span>
          ))}
        </span>
      </div>
    </div>
  );
}

/** A point event as a dot, a span as a bar, an undated one as a dashed rule. */
function Mark({ lane, selected }: { lane: Lane; selected: boolean }) {
  const solid = selected
    ? "var(--color-accent-700)"
    : "color-mix(in srgb, var(--color-accent) 34%, transparent)";

  if (lane.isPoint) {
    return (
      <i
        aria-hidden
        style={{
          position: "absolute",
          left: `${lane.left}%`,
          top: 1,
          width: 8,
          height: 8,
          marginLeft: -4,
          borderRadius: "50%",
          background: selected ? "var(--color-accent-900)" : solid,
        }}
      />
    );
  }

  // Dashed where no date is knowable: the bar spans the horizon because the
  // trigger is a balance crossing, not because the event runs throughout.
  if (lane.undated) {
    return (
      <i
        aria-hidden
        style={{
          position: "absolute",
          left: `${lane.left}%`,
          width: `${lane.width}%`,
          top: 4,
          borderTop: `3px dashed ${solid}`,
        }}
      />
    );
  }

  return (
    <i
      aria-hidden
      style={{
        position: "absolute",
        left: `${lane.left}%`,
        width: `${lane.width}%`,
        minWidth: 2,
        top: 2,
        height: 6,
        background: solid,
      }}
    />
  );
}
