"use client";

import { type Scale, areaPath, bandPath, barColumn, linePath } from "./geometry";
import type { AccountSeries } from "@/lib/types";

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

/** Per-account composition of the selected run, stacked to the total. */
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
      {stackBands(series, scale.count)
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
  const bands = stackBands(series, scale.count);

  return (
    <g>
      {Array.from({ length: scale.count }, (_, i) => {
        const { x, w } = barColumn(i, scale);
        return (
          <g key={i} fillOpacity={highlight == null || highlight === i ? 1 : 0.75}>
            {bands.map((band, j) => {
              const top = scale.y(band.top[i]);
              const height = scale.y(band.bottom[i]) - top;
              // An account worth nothing this year — or, on a log axis, worth
              // less than the floor — has no segment to draw.
              if (height < 0.2) return null;
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

interface StackBand {
  color: string;
  /** The running total under this account, at every index. */
  bottom: number[];
  /** …and with it, which is where the next one starts. */
  top: number[];
}

/** The stack's edges, in the order the accounts are laid down: bottom first. */
function stackBands(series: AccountSeries[], count: number): StackBand[] {
  const running = new Array<number>(count).fill(0);
  return series.map((s) => {
    const bottom = [...running];
    const top = bottom.map((v, i) => v + (s.values[i] ?? 0));
    top.forEach((v, i) => {
      running[i] = v;
    });
    return { color: s.color, bottom, top };
  });
}
