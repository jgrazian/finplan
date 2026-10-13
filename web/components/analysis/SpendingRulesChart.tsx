"use client";

import { useMemo, useState } from "react";
import { ChartCanvas, makeScale, useChartGeometry } from "@/components/charts";
import type { ChartGeometry, Scale } from "@/components/charts/geometry";
import { fmtAxis, fmtCompact } from "@/lib/format";
import { useIsMobile } from "@/lib/hooks/useIsMobile";
import { type RuleChart, ruleChartMax } from "@/lib/view/spending";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

const GEOMETRY: ChartGeometry = { w: 920, h: 300, left: 64, right: 12, top: 8, bottom: 28 };
/** The phone frame `SpendingCard` uses. */
const PHONE_GEOMETRY: ChartGeometry = { w: 360, h: 250, left: 46, right: 8, top: 8, bottom: 26 };

/** A polyline through the points that have a value, broken where one has none. */
function gappedPath(values: (number | null)[], scale: Scale): string {
  let open = false;
  return values
    .map((v, i) => {
      if (v == null) {
        open = false;
        return "";
      }
      const step = `${open ? "L" : "M"}${scale.x(i).toFixed(1)} ${scale.y(v).toFixed(1)}`;
      open = true;
      return step;
    })
    .join(" ");
}

/** The band between two series, one closed shape per unbroken run of years. */
function gappedBand(upper: (number | null)[], lower: (number | null)[], scale: Scale): string {
  const runs: number[][] = [];
  upper.forEach((v, i) => {
    if (v == null || lower[i] == null) return;
    const run = runs[runs.length - 1];
    if (run && run[run.length - 1] === i - 1) run.push(i);
    else runs.push([i]);
  });
  const at = (i: number, v: number) => `${scale.x(i).toFixed(1)} ${scale.y(v).toFixed(1)}`;
  return runs
    .map((run) => {
      const top = run.map((i, k) => `${k ? "L" : "M"}${at(i, upper[i]!)}`).join(" ");
      const bottom = [...run].reverse().map((i) => `L${at(i, lower[i]!)}`).join(" ");
      return `${top} ${bottom} Z`;
    })
    .join(" ");
}

/**
 * Analysis > Spending: each rule's draw by year, its pointwise median as a
 * line, on the same markets. The selected rule (a row of the table, or a
 * name in the readout) also shows its P10–P90 band. Hover reads every rule
 * at a year; click pins it.
 */
export function SpendingRulesChart({
  chart,
  selected,
  onSelect,
}: {
  chart: RuleChart;
  selected: number;
  onSelect: (index: number) => void;
}) {
  const mobile = useIsMobile();
  const [hover, setHover] = useState<number | null>(null);
  const [pinned, setPinned] = useState<number | null>(null);
  const [measure, geo] = useChartGeometry(mobile ? PHONE_GEOMETRY : GEOMETRY);
  const scale = useMemo(
    () =>
      // The axis takes in the selected band whole, so its upper tail is never cut flat.
      makeScale(chart.years.length, { min: 0, max: ruleChartMax(chart, selected) * 1.05 }, {
        mode: "point",
        geo,
      }),
    [chart, geo, selected],
  );

  if (chart.years.length === 0) return null;

  const focus = hover ?? pinned;
  const index = focus ?? 0;
  const chosen = chart.series[selected] ?? chart.series[0];
  // The selected rule's band sits under every line; its own line goes last, on top.
  const order = chart.series.map((_, i) => i).filter((i) => i !== selected).concat(selected);

  return (
    <section aria-label="Spending by year under each rule" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        <h4 style={{ margin: 0 }}>The draw by year</h4>
        <span style={{ fontSize: 12, color: MUTED }}>
          Each rule&apos;s yearly draw, pointwise median · the shading is {chosen.label}&apos;s P10–P90 ·
          today&apos;s dollars
        </span>
      </div>

      <ChartCanvas
        measureRef={measure}
        scale={scale}
        years={chart.years}
        format={fmtAxis}
        hoverIndex={hover}
        onHoverChange={setHover}
        hoverY={chosen.p50[index] == null ? undefined : scale.y(chosen.p50[index]!)}
        pinnedIndex={pinned}
        onSelect={(i) => setPinned((at) => (at === i ? null : i))}
        zeroLabel="No draw"
      >
        <path d={gappedBand(chosen.p90, chosen.p10, scale)} fill={chosen.color} fillOpacity={0.14}>
          <title>{`${chosen.label}: pointwise P10–P90`}</title>
        </path>
        {order.map((i) => {
          const s = chart.series[i];
          const active = i === selected;
          return (
            <path
              key={s.key}
              d={gappedPath(s.p50, scale)}
              fill="none"
              stroke={s.color}
              strokeWidth={active ? 2.5 : 2}
              strokeOpacity={active ? 1 : 0.55}
              strokeDasharray={s.dashed ? "6 4" : undefined}
              strokeLinejoin="round"
            >
              <title>{`${s.label}: pointwise P50 draw`}</title>
            </path>
          );
        })}
      </ChartCanvas>

      {/* The legend, read at the year in focus. A name selects that rule. */}
      <div style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12 }}>
        <div style={{ color: MUTED }}>
          {focus == null ? `${chart.years[0]}, the first year` : chart.years[index]} · P50, then P10–P90
        </div>
        {/* Equal cells with fixed number columns, so moving along the years
            changes the figures without moving or rewrapping anything. */}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(290px, 1fr))", gap: "2px 6px" }}>
          {chart.series.map((s, i) => {
            const active = i === selected;
            const p50 = s.p50[index];
            const p10 = s.p10[index];
            const p90 = s.p90[index];
            return (
              <button
                key={s.key}
                type="button"
                aria-pressed={active}
                onClick={() => onSelect(i)}
                style={{
                  display: "grid",
                  gridTemplateColumns: "16px minmax(0, 1fr) 6ch 14ch",
                  alignItems: "center",
                  columnGap: 8,
                  padding: "3px 8px",
                  textAlign: "left",
                  border: "1px solid",
                  borderColor: active ? "var(--color-divider)" : "transparent",
                  borderRadius: 4,
                  background: active ? "color-mix(in srgb, var(--color-text) 5%, transparent)" : "transparent",
                  color: "var(--color-text)",
                  font: "inherit",
                  cursor: "pointer",
                  whiteSpace: "nowrap",
                }}
              >
                <svg width={16} height={8} aria-hidden style={{ flex: "none" }}>
                  <line
                    x1={0}
                    x2={16}
                    y1={4}
                    y2={4}
                    stroke={s.color}
                    strokeWidth={2}
                    strokeDasharray={s.dashed ? "4 3" : undefined}
                  />
                </svg>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{s.label}</span>
                <span style={{ fontVariantNumeric: "tabular-nums", fontWeight: 600, textAlign: "right" }}>
                  {p50 == null ? "—" : fmtCompact(p50)}
                </span>
                <span style={{ color: MUTED, fontVariantNumeric: "tabular-nums", textAlign: "right" }}>
                  {p10 != null && p90 != null && p90 > p10 ? `${fmtCompact(p10)}–${fmtCompact(p90)}` : ""}
                </span>
              </button>
            );
          })}
        </div>
      </div>
    </section>
  );
}
