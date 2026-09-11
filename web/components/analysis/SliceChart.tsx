"use client";

import { Blueprint } from "@/components/ui";
import { fmtPercent } from "@/lib/format";
import { paramTick, type Slice } from "@/lib/view/analysis";

const GEO = { w: 300, h: 108, left: 34, top: 8, right: 10, bottom: 22 } as const;

const TICK = {
  fontSize: 9.5,
  fontFamily: "Barlow, sans-serif",
  fill: "#1d1f20",
  fillOpacity: 0.55,
} as const;

/**
 * One line through the grid: success against a single axis, everything else
 * held where the pinned cell holds it.
 *
 * The threshold is on the chart as a rule, so the crossing the frontier marks
 * on the heatmap can be read as a number here.
 */
export function SliceChart({
  slice,
  threshold,
  /** The step the pinned cell sits on, marked so the two charts agree. */
  marked,
}: {
  slice: Slice;
  threshold: number;
  marked?: number;
}) {
  const plotW = GEO.w - GEO.left - GEO.right;
  const plotH = GEO.h - GEO.top - GEO.bottom;
  const count = slice.points.length;

  const x = (i: number) =>
    GEO.left + (count <= 1 ? plotW / 2 : (i / (count - 1)) * plotW);
  const y = (success: number) => GEO.top + (1 - Math.min(1, Math.max(0, success))) * plotH;

  const path = slice.points.map((p, i) => `${i === 0 ? "M" : "L"} ${x(i)} ${y(p.success)}`).join(" ");
  const baseline = GEO.top + plotH;

  return (
    <Blueprint style={{ padding: "6px 8px" }}>
      <svg
        viewBox={`0 0 ${GEO.w} ${GEO.h}`}
        style={{ width: "100%", display: "block" }}
        role="img"
        aria-label={`Success against ${slice.axis.label}`}
      >
        {[0, 0.5, 1].map((tick) => (
          <g key={tick}>
            <line
              x1={GEO.left}
              x2={GEO.w - GEO.right}
              y1={y(tick)}
              y2={y(tick)}
              stroke="#1d1f20"
              strokeOpacity={tick === 0 ? 0.3 : 0.12}
            />
            <text {...TICK} x={GEO.left - 6} y={y(tick) + 3} textAnchor="end">
              {Math.round(tick * 100)}
            </text>
          </g>
        ))}

        <line
          x1={GEO.left}
          x2={GEO.w - GEO.right}
          y1={y(threshold)}
          y2={y(threshold)}
          stroke="#1d1f20"
          strokeOpacity={0.5}
          strokeDasharray="3 3"
        />

        {marked != null && marked < count && (
          <line
            x1={x(marked)}
            x2={x(marked)}
            y1={GEO.top}
            y2={baseline}
            stroke="#1d1f20"
            strokeOpacity={0.35}
            strokeDasharray="2 3"
          />
        )}

        <path d={path} fill="none" stroke="#41617f" strokeWidth={2} />
        {slice.points.map((point, i) => (
          <circle key={i} cx={x(i)} cy={y(point.success)} r={2.8} fill="#1d2d3d">
            <title>{`${paramTick(slice.axis.kind, point.value)} — ${fmtPercent(point.success)}`}</title>
          </circle>
        ))}

        {/* Only the ends are labelled: three ticks on a 300-wide chart is a
            legible axis, six is a smear. */}
        {[0, count - 1].map((i) =>
          i >= 0 && i < count ? (
            <text
              key={`t-${i}`}
              {...TICK}
              x={x(i)}
              y={GEO.h - 8}
              textAnchor={i === 0 ? "start" : "end"}
            >
              {paramTick(slice.axis.kind, slice.points[i].value)}
            </text>
          ) : null,
        )}
      </svg>
    </Blueprint>
  );
}
