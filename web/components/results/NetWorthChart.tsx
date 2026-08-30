"use client";

import { useMemo, useState } from "react";
import {
  BarSeries,
  ChartCanvas,
  FanSeries,
  StackedSeries,
  domainMax,
  makeScale,
} from "@/components/charts";
import { fmtCompact } from "@/lib/format";
import type { AccountSeries, NetWorthBands, Percentile } from "@/lib/types";
import { ChartReadout } from "./ChartReadout";
import type { ChartView } from "./types";

/**
 * The single chart frame, in whichever of the three treatments is selected.
 * Owns only its hover index; view and percentile are lifted so the toolbar
 * and the rail can read them.
 */
export function NetWorthChart({
  bands,
  accountSeries,
  view,
  percentile,
}: {
  bands: NetWorthBands;
  accountSeries: AccountSeries[];
  view: ChartView;
  percentile: Percentile;
}) {
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);

  const scale = useMemo(
    () => makeScale(bands.years.length, domainMax(bands.p95)),
    [bands],
  );

  const active = bands[percentile];
  const index = hoverIndex ?? bands.years.length - 1;

  return (
    <>
      <ChartCanvas
        scale={scale}
        years={bands.years}
        format={fmtCompact}
        hoverIndex={hoverIndex}
        onHoverChange={setHoverIndex}
        hoverY={scale.y(active[index])}
      >
        {view === "fan" && (
          <FanSeries p5={bands.p5} p50={bands.p50} p95={bands.p95} scale={scale} />
        )}
        {view === "stack" && <StackedSeries series={accountSeries} scale={scale} />}
        {view === "bar" && <BarSeries values={active} scale={scale} />}
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
