"use client";

import { SectionHeading } from "@/components/ui";
import type { EventId, PlanEvent } from "@/lib/types";
import { layoutMarks, makeTimelineScale, milestoneTicks } from "./timeline";

const VIEW_W = 660;
const VIEW_H = 68;
const AXIS_Y = 50;

/**
 * Artboard 3b — the plan at a glance, pinned to the footer of the event list.
 * Spans draw as bars and one-shot triggers as points, both on the age axis;
 * the selected event is the only one at full opacity. Clicking a mark selects
 * its event, so the strip is a second way into the same selection the table
 * drives.
 */
export function MiniTimeline({
  events,
  ageRange,
  selectedId,
  onSelect,
}: {
  events: PlanEvent[];
  ageRange: [number, number];
  selectedId: EventId;
  onSelect: (id: EventId) => void;
}) {
  const scale = makeTimelineScale(ageRange);
  const marks = layoutMarks(events, scale);
  const ticks = milestoneTicks(scale);

  return (
    <div
      style={{
        borderTop: "1px solid var(--color-divider)",
        padding: "8px 20px 12px",
        background: "color-mix(in srgb, var(--color-text) 3%, transparent)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          justifyContent: "space-between",
          marginBottom: 2,
        }}
      >
        <SectionHeading>Plan at a glance</SectionHeading>
        <span
          style={{
            fontSize: 11,
            color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
          }}
        >
          selected event highlighted
        </span>
      </div>

      <svg viewBox={`0 0 ${VIEW_W} ${VIEW_H}`} style={{ width: "100%", display: "block" }}>
        {ticks.map((t) => (
          <line
            key={`tick-${t.age}`}
            x1={t.x}
            x2={t.x}
            y1={8}
            y2={AXIS_Y}
            stroke="#1d1f20"
            strokeOpacity={0.1}
          />
        ))}

        {marks.map((m, i) => {
          const selected = events[i].id === selectedId;
          const opacity = selected ? 0.95 : 0.35;
          return (
            <g
              key={m.id}
              onClick={() => onSelect(m.id)}
              style={{ cursor: "pointer" }}
              role="button"
              aria-label={`Select ${m.id}`}
            >
              {/* Transparent hit area — a 7px bar is a hard click target. */}
              <rect
                x={m.x - 4}
                y={m.y - 3}
                width={m.w + 8}
                height={13}
                fill="transparent"
              />
              {m.isPoint ? (
                <circle cx={m.x} cy={m.y + 3} r={3.5} fill="#1d2d3d" fillOpacity={opacity} />
              ) : (
                <rect
                  x={m.x}
                  y={m.y}
                  width={m.w}
                  height={7}
                  fill="#5980a6"
                  fillOpacity={opacity}
                />
              )}
            </g>
          );
        })}

        <line
          x1={scale.x0}
          x2={scale.x1}
          y1={AXIS_Y}
          y2={AXIS_Y}
          stroke="#1d1f20"
          strokeOpacity={0.3}
        />
        {ticks.map((t) => (
          <text
            key={`label-${t.age}`}
            x={t.x}
            y={60}
            textAnchor="middle"
            fontSize={9}
            fontFamily="Barlow, sans-serif"
            fill="#1d1f20"
            fillOpacity={0.55}
          >
            {t.label}
          </text>
        ))}
      </svg>
    </div>
  );
}
