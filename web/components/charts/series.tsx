"use client";

import { type Scale, areaPath, bandPath, barColumn, linePath } from "./geometry";
import type { AccountSeries, NetWorthBands } from "@/lib/types";
import { type ChartMarker, stackLabels } from "@/lib/view/outcome";
import { stackSeries } from "./stack";

/**
 * Pointwise real envelope: the outer band (P10–P90) light, the inner band
 * (P25–P75) darker over it, and the P50. None of them is a coherent path.
 * Drawn in the neutral ink rather than the accent, so the envelope means the
 * same thing whichever accent is chosen and never reads as an account series.
 * The scale is set by the inner band, so the outer one may run off the top
 * and is clipped there by the scale's own clamp.
 */
export function FanSeries({
  bands,
  scale,
}: {
  bands: NetWorthBands;
  scale: Scale;
}) {
  const [lo, hi] = bands.outer;
  return (
    <g>
      <path d={bandPath(bands.high, bands.low, scale)} fill="var(--color-envelope-outer)">
        <title>{`Pointwise real P${lo}–P${hi}`}</title>
      </path>
      {bands.upperQuartile.length > 0 && (
        <path
          d={bandPath(bands.upperQuartile, bands.lowerQuartile, scale)}
          fill="var(--color-envelope-inner)"
        >
          <title>Pointwise real P25–P75</title>
        </path>
      )}
      <path d={linePath(bands.p50, scale)} fill="none" stroke="var(--color-envelope-p50)" strokeWidth={2} strokeDasharray="6 4">
        <title>Pointwise real P50 across all iterations (not a path)</title>
      </path>
    </g>
  );
}

/** Signed account composition: assets above zero and debt below it. */
export function StackedSeries({
  series,
  scale,
}: {
  series: AccountSeries[];
  scale: Scale;
}) {
  // Painted back to front so the darkest band (the base of the stack) sits on
  // top of its own fill edge rather than under the one above it.
  return (
    <g>
      {stackSeries(series, scale.count).bands
        .reverse()
        .map((band, i) => (
          <path key={i} d={areaPath(band.top, band.bottom, scale)} fill={band.color} />
        ))}
    </g>
  );
}

/**
 * The same composition read year by year: one column per year, split into the
 * account bands the stack draws as areas.
 */
export function StackedBarSeries({
  series,
  scale,
  /** The year under the cursor; the rest of the columns step back for it. */
  highlight,
}: {
  series: AccountSeries[];
  scale: Scale;
  highlight?: number | null;
}) {
  const { bands } = stackSeries(series, scale.count);

  return (
    <g>
      {Array.from({ length: scale.count }, (_, i) => {
        const { x, w } = barColumn(i, scale);
        return (
          <g key={i} fillOpacity={highlight == null || highlight === i ? 1 : 0.75}>
            {bands.map((band, j) => {
              const top = scale.y(band.top[i]);
              const height = scale.y(band.bottom[i]) - top;
              // Debt bands have ordered edges too; only actual zero is omitted.
              if (height <= 0) return null;
              return (
                <rect
                  key={j}
                  x={x.toFixed(1)}
                  y={top.toFixed(1)}
                  width={w.toFixed(1)}
                  height={height.toFixed(1)}
                  fill={band.color}
                />
              );
            })}
          </g>
        );
      })}
    </g>
  );
}

/** Label type for the markers; the width estimate below assumes it. */
const MARKER_FONT = 10;
const MARKER_ROW = 13;

/**
 * One-off events — the home purchase, retirement — as faint rules through the
 * plot with their names along the top, so the bends in the path have causes.
 * Labels that would collide step down a row rather than overprint.
 */
export function EventMarkers({
  markers,
  scale,
}: {
  markers: ChartMarker[];
  scale: Scale;
}) {
  const geo = scale.geo;
  const right = geo.w - geo.right;
  const placed = markers.map((m) => {
    const x = scale.x(m.index);
    const width = m.label.length * MARKER_FONT * 0.56 + 10;
    // A label that would run off the right edge hangs to the left of its rule.
    const flip = x + width > right;
    return { ...m, x, width, start: flip ? x - width : x };
  });
  const rows = stackLabels(placed.map((p) => ({ x: p.start, width: p.width })));

  return (
    <g pointerEvents="none" className="event-markers">
      {placed.map((p, i) => {
        const y = geo.top + 9 + rows[i] * MARKER_ROW;
        const flip = p.start < p.x;
        return (
          <g key={`${p.index}-${p.label}`}>
            <title>{`${p.label} · ${p.year}`}</title>
            <line
              x1={p.x}
              x2={p.x}
              y1={y + 3}
              y2={scale.baseline}
              stroke="var(--color-text)"
              strokeOpacity={0.28}
              strokeDasharray="2 3"
            />
            <circle cx={p.x} cy={y - 3} r={2.5} fill="var(--color-accent-2)" />
            <text
              x={flip ? p.x - 6 : p.x + 6}
              y={y}
              textAnchor={flip ? "end" : "start"}
              fontSize={MARKER_FONT}
              fontFamily="var(--font-body)"
              fontWeight={600}
              fill="var(--color-text)"
              fillOpacity={0.72}
              stroke="var(--color-raised)"
              strokeWidth={3}
              strokeLinejoin="round"
              paintOrder="stroke"
            >
              {p.label}
            </text>
          </g>
        );
      })}
    </g>
  );
}
