"use client";

import type { ReactNode } from "react";
import { Blueprint } from "@/components/ui";
import { FAN, type FanView } from "@/lib/view/whatIf";

const TICK = {
  fontSize: 10.5,
  fontFamily: "Barlow, sans-serif",
  fill: "var(--color-text)",
  fillOpacity: 0.55,
} as const;

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

/** Net worth in today's dollars: the plan ghosted, the what-if over it. */
export function WhatIfFan({ view, byAge }: { view: FanView; byAge: boolean }) {
  const G = FAN;
  return (
    <div>
      <Blueprint style={{ padding: "6px 8px" }}>
        <svg
          viewBox={`0 0 ${G.w} ${G.h}`}
          style={{ width: "100%", display: "block" }}
          role="img"
          aria-label="Net worth in today's dollars, plan against what-if: P25 to P75 band around the P50"
        >
          {view.yTicks.map((tick) => (
            <g key={tick.label}>
              <line
                x1={G.left}
                x2={G.w - G.right}
                y1={tick.y}
                y2={tick.y}
                stroke="var(--color-text)"
                strokeOpacity={tick.zero ? 0.3 : 0.07}
              />
              <text {...TICK} x={G.left - 6} y={tick.y + 3.5} textAnchor="end">
                {tick.label}
              </text>
            </g>
          ))}
          {view.xTicks.map((tick) => (
            <text key={tick.label} {...TICK} x={tick.x} y={G.h - G.bottom + 14} textAnchor="middle">
              {tick.label}
            </text>
          ))}
          <path d={view.planBand} fill="var(--color-text)" fillOpacity={0.07} />
          <path
            d={view.planMedian}
            fill="none"
            stroke="var(--color-text)"
            strokeOpacity={0.5}
            strokeWidth={1.5}
            strokeDasharray="4 3"
          />
          <path d={view.whatIfBand} fill="var(--color-accent)" fillOpacity={0.2} />
          <path d={view.whatIfMedian} fill="none" stroke="var(--color-accent-700)" strokeWidth={2} />
          {view.planRetire != null && (
            <line
              x1={view.planRetire}
              x2={view.planRetire}
              y1={view.top}
              y2={view.bottom}
              stroke="var(--color-text)"
              strokeOpacity={0.35}
              strokeDasharray="2 3"
            />
          )}
          {view.whatIfRetire != null && (
            <line
              x1={view.whatIfRetire}
              x2={view.whatIfRetire}
              y1={view.top}
              y2={view.bottom}
              stroke="var(--color-accent-700)"
              strokeWidth={1.2}
            />
          )}
        </svg>
      </Blueprint>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: "6px 22px",
          marginTop: 8,
          fontSize: 11.5,
          color: MUTED,
          alignItems: "center",
        }}
      >
        <Key swatch={<Swatch
              fill="color-mix(in srgb, var(--color-text) 10%, transparent)"
              line="1.5px dashed color-mix(in srgb, var(--color-text) 50%, transparent)"
            />}>Plan</Key>
        <Key swatch={<Swatch
              fill="color-mix(in srgb, var(--color-accent) 24%, transparent)"
              line="2px solid var(--color-accent-700)"
            />}>What-if</Key>
        <span>band P25–P75 · line P50</span>
        <span style={{ marginLeft: "auto" }}>
          {view.planRetire != null || view.whatIfRetire != null
            ? "vertical rules mark each retirement age"
            : byAge
              ? "by age"
              : "by calendar year — add a birth date to plot by age"}
        </span>
      </div>
    </div>
  );
}

function Key({ swatch, children }: { swatch: ReactNode; children: ReactNode }) {
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
      {swatch}
      {children}
    </span>
  );
}

/** One series' band with its centreline along the top, as in the chart. */
function Swatch({ fill, line }: { fill: string; line: string }) {
  return <i style={{ width: 18, height: 9, background: fill, borderTop: line }} />;
}
