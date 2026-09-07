"use client";

import type { ReactNode } from "react";
import { Blueprint } from "@/components/ui";
import {
  DEFAULT_GEOMETRY,
  type Scale,
  indexFromPointer,
  valueTicks,
  yearTicks,
} from "./geometry";

const TICK_TEXT = {
  fontSize: 10,
  fontFamily: "Barlow, sans-serif",
  fill: "#1d1f20",
  fillOpacity: 0.5,
} as const;

/**
 * The one chart frame every Results view shares: blueprint border, value
 * gridlines, both axes and the hover crosshair. Series renderers are passed
 * as children and receive the scale, so swapping fan → stack → bars changes
 * only the marks, never the frame.
 */
export function ChartCanvas({
  scale,
  years,
  format,
  hoverIndex,
  onHoverChange,
  /** Y of the crosshair dot; omit to draw the rule alone. */
  hoverY,
  pinnedIndex,
  onSelect,
  children,
}: {
  scale: Scale;
  years: number[];
  format: (v: number) => string;
  hoverIndex: number | null;
  onHoverChange: (index: number | null) => void;
  hoverY?: number;
  /** The year held on screen after the pointer leaves; drawn as a dashed rule. */
  pinnedIndex?: number | null;
  /** Click a year to pin it. Omit to leave the plot read-only. */
  onSelect?: (index: number) => void;
  children: ReactNode;
}) {
  const geo = scale.geo ?? DEFAULT_GEOMETRY;
  const vTicks = valueTicks(scale);
  const xTicks = yearTicks(years, scale);

  return (
    <Blueprint style={{ position: "relative", padding: "10px 12px 4px" }}>
      <svg
        viewBox={`0 0 ${geo.w} ${geo.h}`}
        style={{
          width: "100%",
          display: "block",
          overflow: "visible",
          cursor: onSelect ? "crosshair" : undefined,
        }}
        onClick={() => {
          if (hoverIndex != null) onSelect?.(hoverIndex);
        }}
        onMouseMove={(e) => {
          const rect = e.currentTarget.getBoundingClientRect();
          const next = indexFromPointer(e.clientX, rect, scale);
          if (next !== hoverIndex) onHoverChange(next);
        }}
        onMouseLeave={() => onHoverChange(null)}
      >
        {vTicks.map((t, i) => (
          <line
            key={`grid-${i}`}
            x1={geo.left}
            x2={geo.w - geo.right}
            y1={t.y}
            y2={t.y}
            stroke="#1d1f20"
            strokeOpacity={0.1}
            strokeWidth={1}
          />
        ))}
        {vTicks.map((t, i) => (
          <text key={`vt-${i}`} x={geo.left - 8} y={t.y + 3} textAnchor="end" {...TICK_TEXT}>
            {format(t.value)}
          </text>
        ))}

        {children}

        <line
          x1={geo.left}
          x2={geo.w - geo.right}
          y1={scale.baseline}
          y2={scale.baseline}
          stroke="#1d1f20"
          strokeOpacity={0.3}
        />
        {xTicks.map((t) => (
          <text key={`xt-${t.year}`} x={t.x} y={geo.h - 12} textAnchor="middle" {...TICK_TEXT}>
            {t.year}
          </text>
        ))}

        {pinnedIndex != null && (
          <g pointerEvents="none">
            <line
              x1={scale.x(pinnedIndex)}
              x2={scale.x(pinnedIndex)}
              y1={geo.top}
              y2={scale.baseline}
              stroke="#5980a6"
              strokeWidth={1}
              strokeDasharray="3 3"
            />
            <rect
              x={scale.x(pinnedIndex) - 3}
              y={scale.baseline - 3}
              width={6}
              height={6}
              fill="#5980a6"
            />
          </g>
        )}

        {hoverIndex != null && (
          <g pointerEvents="none">
            <line
              x1={scale.x(hoverIndex)}
              x2={scale.x(hoverIndex)}
              y1={geo.top}
              y2={scale.baseline}
              stroke="#1d2d3d"
              strokeWidth={1}
            />
            {hoverY != null && (
              <circle cx={scale.x(hoverIndex)} cy={hoverY} r={3.5} fill="#1d2d3d" />
            )}
          </g>
        )}
      </svg>
    </Blueprint>
  );
}
