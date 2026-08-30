"use client";

import {
  type Scale,
  areaPath,
  bandPath,
  barLayout,
  linePath,
} from "./geometry";
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
  const running = series.length ? series[0].values.map(() => 0) : [];
  const bands = series.map((s) => {
    const bottom = [...running];
    s.values.forEach((v, i) => {
      running[i] += v;
    });
    return { color: s.color, d: areaPath([...running], bottom, scale) };
  });

  // Painted back to front so the darkest band (the base of the stack) sits on
  // top of its own fill edge rather than under the one above it.
  return (
    <g>
      {bands
        .slice()
        .reverse()
        .map((b, i) => (
          <path key={i} d={b.d} fill={b.color} />
        ))}
    </g>
  );
}

/** One bar per year for the selected percentile run. */
export function BarSeries({ values, scale }: { values: number[]; scale: Scale }) {
  return (
    <g>
      {barLayout(values, scale).map((b, i) => (
        <rect
          key={i}
          x={b.x.toFixed(1)}
          y={b.y.toFixed(1)}
          width={b.w.toFixed(1)}
          height={b.h.toFixed(1)}
          fill="#5980a6"
          fillOpacity={0.75}
        />
      ))}
    </g>
  );
}
