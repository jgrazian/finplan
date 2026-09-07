"use client";

import { useMemo, useState } from "react";
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

/**
 * The single chart frame, in whichever of the three treatments is selected.
 * Owns only its hover index; view, percentile and scale kind are lifted so the
 * toolbar and the rail can read them.
 */
export function NetWorthChart({
  bands,
  accountSeries,
  view,
  percentile,
  scaleKind,
}: {
  bands: NetWorthBands;
  accountSeries: AccountSeries[];
  view: ChartView;
  percentile: Percentile;
  scaleKind: ScaleKind;
}) {
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);

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

  const index = hoverIndex ?? bands.years.length - 1;
  const cursorValue = view === "fan" ? active[index] : (totals[index] ?? 0);

  return (
    <>
      <ChartCanvas
        scale={scale}
        years={bands.years}
        format={fmtAxis}
        hoverIndex={hoverIndex}
        onHoverChange={setHoverIndex}
        hoverY={scale.y(cursorValue)}
      >
        {view === "fan" && (
          <FanSeries p5={bands.p5} p50={bands.p50} p95={bands.p95} scale={scale} />
        )}
        {view === "stack" && <StackedSeries series={accountSeries} scale={scale} />}
        {view === "bar" && (
          <StackedBarSeries
            series={accountSeries}
            scale={scale}
            highlight={hoverIndex}
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
