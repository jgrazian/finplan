"use client";

import { type Scale, areaPath, bandPath, barColumn, linePath } from "./geometry";
import type { AccountSeries } from "@/lib/types";
import { stackSeries } from "./stack";

/** P5–P95 band with a solid median and dashed edges. */
export function FanSeries({
  p5,
  p50,
  p95,
  scale,
}: {
  p5: number[];
  p50: number[];
  p95: number[];
  scale: Scale;
}) {
  return (
    <g>
      <path d={bandPath(p95, p5, scale)} fill="#5980a6" fillOpacity={0.16} />
      <path
        d={linePath(p95, scale)}
        fill="none"
        stroke="#5980a6"
        strokeOpacity={0.55}
        strokeWidth={1}
        strokeDasharray="4 3"
      />
      <path
        d={linePath(p5, scale)}
        fill="none"
        stroke="#5980a6"
        strokeOpacity={0.55}
        strokeWidth={1}
        strokeDasharray="4 3"
      />
      <path d={linePath(p50, scale)} fill="none" stroke="#41617f" strokeWidth={2} />
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
