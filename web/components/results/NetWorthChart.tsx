"use client";

import { useMemo } from "react";
import {
  ChartCanvas,
  FanSeries,
  type ScaleKind,
  StackedBarSeries,
  StackedSeries,
  domainFor,
  makeScale,
} from "@/components/charts";
import { fmtAxis } from "@/lib/format";
import type { AccountSeries, NetWorthBands, Percentile } from "@/lib/types";
import { ChartReadout } from "./ChartReadout";
import type { ChartView } from "./types";
import type { YearFocus } from "./useYearFocus";

/**
 * The single chart frame, in whichever of the three treatments is selected.
 * Owns nothing: view, percentile, scale kind and the year under the pointer are
 * all lifted, because the toolbar and the rail read them too.
 */
export function NetWorthChart({
  bands,
  accountSeries,
  view,
  percentile,
  scaleKind,
  focus,
}: {
  bands: NetWorthBands;
  accountSeries: AccountSeries[];
  view: ChartView;
  percentile: Percentile;
  scaleKind: ScaleKind;
  /** The year the chart and the rail's breakdown are both pointed at. */
  focus: YearFocus;
}) {
  const { hoverIndex, pinnedIndex } = focus;

  const active = bands[percentile];
  const totals = useMemo(
    () => stackTotals(accountSeries, bands.years.length),
    [accountSeries, bands.years.length],
  );

  // Each treatment draws a different set of numbers, so each gets the axis
  // those numbers need: the fan is scaled to P95, but the two composition views
  // draw one run and would otherwise be pressed into the bottom of a frame
  // built for a run they never plot.
  const plotted = useMemo(
    () => (view === "fan" ? [bands.p5, bands.p50, bands.p95] : [totals]),
    [view, totals, bands],
  );

  const scale = useMemo(
    () =>
      makeScale(bands.years.length, domainFor(scaleKind, plotted), {
        mode: view === "bar" ? "band" : "point",
        kind: scaleKind,
      }),
    [bands.years.length, plotted, scaleKind, view],
  );

  const index = focus.index;
  const cursorValue = view === "fan" ? active[index] : (totals[index] ?? 0);

  return (
    <>
      <ChartCanvas
        scale={scale}
        years={bands.years}
        format={fmtAxis}
        hoverIndex={hoverIndex}
        onHoverChange={focus.setHover}
        hoverY={scale.y(cursorValue)}
        pinnedIndex={pinnedIndex}
        onSelect={focus.pin}
      >
        {view === "fan" && (
          <FanSeries p5={bands.p5} p50={bands.p50} p95={bands.p95} scale={scale} />
        )}
        {view === "stack" && <StackedSeries series={accountSeries} scale={scale} />}
        {view === "bar" && (
          // Only a year the reader chose steps the others back; an unattended
          // chart is left flat rather than emphasising its last column.
          <StackedBarSeries
            series={accountSeries}
            scale={scale}
            highlight={hoverIndex ?? pinnedIndex}
          />
        )}
      </ChartCanvas>

      <ChartReadout
        bands={bands}
        index={index}
        isHovering={hoverIndex != null}
        accountSeries={accountSeries}
      />
    </>
  );
}

/** The stack's top edge — what the account bands add up to, year by year. */
function stackTotals(series: AccountSeries[], count: number): number[] {
  return Array.from({ length: count }, (_, i) =>
    series.reduce((sum, s) => sum + (s.values[i] ?? 0), 0),
  );
}
