"use client";

import { type Scale, areaPath, bandPath, barColumn, linePath } from "./geometry";
import type { AccountSeries, NetWorthBands } from "@/lib/types";
import { stackSeries } from "./stack";

/**
 * Pointwise real envelope: the outer band (P10–P90) light, the inner band
 * (P25–P75) darker over it, and the P50. None of them is a coherent path.
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
      <path d={bandPath(bands.high, bands.low, scale)} fill="var(--color-accent)" fillOpacity={0.12}>
        <title>{`Pointwise real P${lo}–P${hi}`}</title>
      </path>
      {bands.upperQuartile.length > 0 && (
        <path
          d={bandPath(bands.upperQuartile, bands.lowerQuartile, scale)}
          fill="var(--color-accent)"
          fillOpacity={0.22}
        >
          <title>Pointwise real P25–P75</title>
        </path>
      )}
      <path d={linePath(bands.p50, scale)} fill="none" stroke="var(--color-accent-700)" strokeWidth={2} strokeDasharray="6 4">
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
