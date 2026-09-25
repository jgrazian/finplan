"use client";

import { Blueprint } from "@/components/ui";
import { WATERFALL, type BarTone, type WaterfallView } from "@/lib/view/whatIf";

const TICK = {
  fontSize: 10.5,
  fontFamily: "Barlow, sans-serif",
  fill: "var(--color-text)",
  fillOpacity: 0.55,
} as const;

/**
 * Gains take the accent and losses the ink, so the two read apart without a
 * second hue; the totals are the solid ends the layers hang between.
 */
const FILL: Record<BarTone, { fill: string; opacity: number }> = {
  plan: { fill: "var(--color-text)", opacity: 0.22 },
  total: { fill: "var(--color-accent-700)", opacity: 0.9 },
  gain: { fill: "var(--color-accent)", opacity: 0.55 },
  loss: { fill: "var(--color-text)", opacity: 0.55 },
  flat: { fill: "var(--color-text)", opacity: 0.35 },
};

/** Where the success went: one bar per override, each on top of the ones before. */
export function SuccessWaterfall({ view }: { view: WaterfallView }) {
  const G = WATERFALL;
  return (
    <Blueprint style={{ padding: "6px 8px" }}>
      <svg
        viewBox={`0 0 ${G.w} ${G.h}`}
        style={{ width: "100%", display: "block" }}
        role="img"
        aria-label="Success rate, from the plan through each override to the what-if"
      >
        {view.grid.map((line, i) => (
          <g key={line.label}>
            <line
              x1={G.left}
              x2={G.w - G.right}
              y1={line.y}
              y2={line.y}
              stroke="var(--color-text)"
              strokeOpacity={i === 0 ? 0.3 : 0.07}
            />
            <text {...TICK} x={G.left - 6} y={line.y + 3.5} textAnchor="end">
              {line.label}
            </text>
          </g>
        ))}
        {view.bars.map((bar) => (
          <g key={bar.key}>
            <title>{`${bar.title}: ${bar.label}`}</title>
            {bar.connector && (
              <line
                x1={bar.connector.x1}
                x2={bar.connector.x2}
                y1={bar.connector.y}
                y2={bar.connector.y}
                stroke="var(--color-text)"
                strokeOpacity={0.45}
                strokeDasharray="2 3"
              />
            )}
            <rect
              x={bar.x}
              y={bar.y}
              width={bar.w}
              height={bar.h}
              fill={FILL[bar.tone].fill}
              fillOpacity={FILL[bar.tone].opacity}
            />
            <text
              x={bar.cx}
              y={bar.labelY}
              textAnchor="middle"
              fontSize={12}
              fontWeight={600}
              fill="var(--color-text)"
              fontFamily="Barlow, sans-serif"
            >
              {bar.label}
            </text>
            <text {...TICK} fontSize={11} fillOpacity={0.65} x={bar.cx} y={G.h - G.bottom + 20} textAnchor="middle">
              {bar.name}
            </text>
          </g>
        ))}
      </svg>
    </Blueprint>
  );
}
