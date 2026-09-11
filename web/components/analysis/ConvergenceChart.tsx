"use client";

import { Blueprint } from "@/components/ui";
import { fmtPercent } from "@/lib/format";
import { paramTick, paramValue } from "@/lib/view/analysis";
import type { AnalysisParameter, SolveStep } from "@/lib/api/types";

const GEO = { w: 360, h: 124, left: 46, top: 10, right: 10, bottom: 24 } as const;

const TICK = {
  fontSize: 9.5,
  fontFamily: "Barlow, sans-serif",
  fill: "#1d1f20",
  fillOpacity: 0.55,
} as const;

/**
 * The search, one probe per column: the bracket closing in, and where in it
 * each probe was taken.
 *
 * Whether a probe cleared the constraint is drawn as a filled dot against a
 * hollow one, not as a second colour — the distinction survives a greyscale
 * print and a colourblind reader, and the legend names both.
 */
export function ConvergenceChart({
  steps,
  parameter,
}: {
  steps: SolveStep[];
  /** The varied parameter, for the value axis. Absent under a grid search. */
  parameter: AnalysisParameter | undefined;
}) {
  const probes = steps.filter((step) => step.values.length > 0);
  if (probes.length === 0 || !parameter) return null;

  const values = probes.flatMap((step) => [
    step.values[0],
    step.bracket_low ?? step.values[0],
    step.bracket_high ?? step.values[0],
  ]);
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  const span = hi - lo || 1;

  const plotW = GEO.w - GEO.left - GEO.right;
  const plotH = GEO.h - GEO.top - GEO.bottom;
  const x = (i: number) =>
    GEO.left + (probes.length <= 1 ? plotW / 2 : (i / (probes.length - 1)) * plotW);
  const y = (value: number) => GEO.top + (1 - (value - lo) / span) * plotH;

  const bracket = (pick: (step: SolveStep) => number | null) =>
    probes
      .map((step, i) => {
        const value = pick(step);
        return value == null ? null : `${i === 0 ? "M" : "L"} ${x(i)} ${y(value)}`;
      })
      .filter(Boolean)
      .join(" ");

  const high = bracket((s) => s.bracket_high);
  const low = bracket((s) => s.bracket_low);

  return (
    <div>
      <Blueprint style={{ padding: "6px 8px", maxWidth: 620 }}>
        <svg
          viewBox={`0 0 ${GEO.w} ${GEO.h}`}
          style={{ width: "100%", display: "block" }}
          role="img"
          aria-label="The bracket narrowing, one probe per iteration"
        >
          {[hi, (hi + lo) / 2, lo].map((value) => (
            <g key={value}>
              <line
                x1={GEO.left}
                x2={GEO.w - GEO.right}
                y1={y(value)}
                y2={y(value)}
                stroke="#1d1f20"
                strokeOpacity={0.12}
              />
              <text {...TICK} x={GEO.left - 6} y={y(value) + 3} textAnchor="end">
                {paramTick(parameter.kind, value)}
              </text>
            </g>
          ))}

          {high && <path d={high} fill="none" stroke="#1d1f20" strokeOpacity={0.35} />}
          {low && <path d={low} fill="none" stroke="#1d1f20" strokeOpacity={0.35} />}

          {probes.map((step, i) => (
            <circle
              key={i}
              cx={x(i)}
              cy={y(step.values[0])}
              r={4}
              fill={step.feasible ? "#41617f" : "#f2f2f3"}
              stroke="#41617f"
              strokeWidth={1.5}
            >
              <title>
                {`${paramValue(parameter.kind, step.values[0])} — ${fmtPercent(
                  step.success_rate,
                )} success, ${step.feasible ? "clears" : "misses"} the constraint`}
              </title>
            </circle>
          ))}

          <text {...TICK} x={GEO.left} y={GEO.h - 8}>
            probe 1
          </text>
          <text {...TICK} x={GEO.w - GEO.right} y={GEO.h - 8} textAnchor="end">
            probe {probes.length}
          </text>
        </svg>
      </Blueprint>
      <div
        style={{
          display: "flex",
          gap: 14,
          maxWidth: 620,
          marginTop: 6,
          fontSize: 11,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
        }}
      >
        <Key filled>clears the constraint</Key>
        <Key>misses it</Key>
        <span style={{ marginLeft: "auto" }}>hairlines are the bracket</span>
      </div>
    </div>
  );
}

function Key({ filled, children }: { filled?: boolean; children: React.ReactNode }) {
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
      <i
        style={{
          width: 9,
          height: 9,
          borderRadius: "50%",
          border: "1.5px solid #41617f",
          background: filled ? "#41617f" : "transparent",
        }}
      />
      {children}
    </span>
  );
}
