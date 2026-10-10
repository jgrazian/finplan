"use client";

import { useMemo } from "react";
import { ChartCanvas, FanSeries, domainFor, makeScale } from "@/components/charts";
import { type ChartGeometry, linePath } from "@/components/charts/geometry";
import { InlineStat } from "@/components/ui";
import { fmtAxis, fmtCompact } from "@/lib/format";
import { useIsMobile } from "@/lib/hooks/useIsMobile";
import type { NetWorthBands } from "@/lib/types";
import type { SpendingView } from "@/lib/view/spending";
import type { YearFocus } from "./useYearFocus";

const MUTED = "color-mix(in srgb, var(--color-text) 55%, transparent)";

/** The phone frame `NetWorthChart` uses, so the two charts scale alike. */
const PHONE_GEOMETRY: ChartGeometry = { w: 360, h: 250, left: 46, right: 8, top: 8, bottom: 26 };

/**
 * Artboard 22 · Results — what a plan with a spending rule spent.
 *
 * Built like the net-worth chart above it, from the same parts: a title and
 * subtitle, the envelope in the same frame, and the readout row under it for
 * the year in focus. Both charts share that focus, by year, so hovering
 * either points both at it. A second readout row says what the rule did
 * across every path.
 */
export function SpendingCard({
  spending,
  years,
  focus,
  dollars,
}: {
  spending: SpendingView;
  /** The net-worth chart's years, which `focus` indexes. */
  years: number[];
  focus: YearFocus;
  dollars: string;
}) {
  const mobile = useIsMobile();
  const bands: NetWorthBands = useMemo(
    () => ({
      years: spending.years,
      ages: spending.ages,
      low: spending.low,
      high: spending.high,
      outer: [10, 90],
      lowerQuartile: spending.lowerQuartile,
      upperQuartile: spending.upperQuartile,
      p50: spending.p50,
    }),
    [spending],
  );
  // Scaled to the inner band and the median, as the net-worth envelope is:
  // the upper tail of spending under a rule that raises in good markets is
  // long enough to press everything else flat.
  const scale = useMemo(
    () =>
      makeScale(
        bands.years.length,
        domainFor("linear", [bands.lowerQuartile, bands.upperQuartile, bands.p50, [0]]),
        { mode: "point", kind: "linear", geo: mobile ? PHONE_GEOMETRY : undefined },
      ),
    [bands, mobile],
  );

  if (bands.years.length === 0) return null;

  // The net-worth focus, read and written by year.
  const toOwn = (index: number | null) => {
    if (index == null) return null;
    const at = bands.years.indexOf(years[index]);
    return at < 0 ? null : at;
  };
  const toShared = (index: number | null) => {
    if (index == null) return null;
    const at = years.indexOf(bands.years[index]);
    return at < 0 ? null : at;
  };
  const hovering = toOwn(focus.hoverIndex) != null;
  const index = toOwn(focus.index) ?? bands.years.length - 1;
  const year = bands.years[index];
  const age = bands.ages[index];
  const range = (low: number | undefined, high: number | undefined) =>
    low == null || high == null ? "—" : `${fmtCompact(low)} – ${fmtCompact(high)}`;
  const belowFirst = spending.medianYearsBelowFirst;

  return (
    <section aria-label="Spending" style={{ marginTop: 32 }}>
      <div className="chart-toolbar" style={{ marginBottom: 14 }}>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 12, minWidth: 0 }}>
          <h4 style={{ margin: 0 }}>Spending — envelope</h4>
          <span style={{ fontSize: 12, color: MUTED }}>
            Everything spent each year, the rule&apos;s draws with the plan&apos;s other expenses ·
            pointwise P10–P90 and P25–P75 shading, P50 solid · {dollars}
          </span>
        </div>
      </div>

      <ChartCanvas
        scale={scale}
        years={bands.years}
        format={fmtAxis}
        hoverIndex={toOwn(focus.hoverIndex)}
        onHoverChange={(i) => focus.setHover(toShared(i))}
        hoverY={scale.y(bands.p50[index] ?? 0)}
        pinnedIndex={toOwn(focus.pinnedIndex)}
        onSelect={(i) => {
          const at = toShared(i);
          if (at != null) focus.pin(at);
        }}
        zeroLabel="No spending"
      >
        <FanSeries bands={bands} scale={scale} />
        {/* The pointwise median, solid: there is no single path to follow
            here, so the median is the line to read. */}
        <path d={linePath(bands.p50, scale)} fill="none" stroke="var(--color-text)" strokeWidth={2}>
          <title>Pointwise P50 spending across all iterations</title>
        </path>
      </ChartCanvas>

      <div className="chart-readout">
        <div className="chart-readout-stats">
          <InlineStat
            label={hovering ? "year / age" : "last full year"}
            value={year == null ? "—" : `${year}${age === year ? "" : ` · age ${age}`}`}
          />
          <InlineStat label="pointwise p50" value={fmtCompact(bands.p50[index])} emphasis />
          <InlineStat label="pointwise p10–p90" value={range(bands.low[index], bands.high[index])} />
          <div className="chart-readout-extra">
            <InlineStat
              label="pointwise p25–p75"
              value={range(bands.lowerQuartile[index], bands.upperQuartile[index])}
            />
          </div>
        </div>
      </div>

      <div className="chart-readout" style={{ marginTop: 6 }}>
        <div className="chart-readout-stats">
          <InlineStat label="first-year draw" value={`${fmtCompact(spending.medianFirstDraw)}/yr`} />
          <InlineStat label="lowest draw, 1 in 10" value={`${fmtCompact(spending.p10LowestDraw)}/yr`} />
          {spending.cutsCanExceed10 && (
            <InlineStat label="cuts over 10%" value={`${Math.round(spending.cutRate * 100)}% of paths`} />
          )}
          <div className="chart-readout-extra">
            <InlineStat
              label="years below 90%"
              value={belowFirst === 1 ? "1 year" : `${belowFirst} years`}
            />
          </div>
          <div className="chart-readout-extra">
            <InlineStat label="lifetime draws" value={fmtCompact(spending.medianLifetime)} />
          </div>
        </div>
        <span style={{ fontSize: 12, color: MUTED }}>
          {spending.rules.length > 0 ? `${spending.rules.join(" and ")} · ` : ""}
          across every path · {dollars}
        </span>
      </div>
    </section>
  );
}
