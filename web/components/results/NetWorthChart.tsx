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
import { linePath } from "@/components/charts/geometry";
import { resolveScaleKind, stackSeries } from "@/components/charts/stack";
import type { AccountSeries, NetWorthBands } from "@/lib/types";
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
  pathValues,
  pathLabel,
  scaleKind,
  focus,
}: {
  bands: NetWorthBands;
  accountSeries: AccountSeries[];
  view: ChartView;
  pathValues: number[];
  pathLabel: string;
  scaleKind: ScaleKind;
  /** The year the chart and the rail's breakdown are both pointed at. */
  focus: YearFocus;
}) {
  const { hoverIndex, pinnedIndex } = focus;

  const stack = useMemo(
    () => stackSeries(accountSeries, bands.years.length),
    [accountSeries, bands.years.length],
  );

  // The fan scales to its inner band and the path being followed, not to the
  // outer band: the upper tail of long-horizon wealth is so skewed that
  // scaling to P90 presses the median flat, so P10–P90 is clipped instead.
  // Older runs without quartiles scale to their P5–P95 as before. Account
  // composition needs gross signed extents.
  const plotted = useMemo(
    () =>
      view === "fan"
        ? bands.upperQuartile.length
          ? [bands.lowerQuartile, bands.p50, bands.upperQuartile, pathValues]
          : [bands.low, bands.p50, bands.high, pathValues]
        : [stack.positive, stack.negative, pathValues],
    [view, stack, bands, pathValues],
  );

  const scale = useMemo(() => {
    const { kind } = resolveScaleKind(scaleKind, view);
    return makeScale(bands.years.length, domainFor(kind, plotted), {
      mode: view === "bar" ? "band" : "point",
      kind,
    });
  }, [bands.years.length, plotted, scaleKind, view]);

  const index = focus.index;
  const cursorValue = pathValues[index] ?? 0;

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
          <FanSeries bands={bands} scale={scale} />
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
        <path d={linePath(pathValues, scale)} fill="none" stroke="var(--color-text)" strokeWidth={2}>
          <title>{pathLabel}: net worth, positive balances less debt</title>
        </path>
      </ChartCanvas>

      <ChartReadout
        bands={bands}
        pathValues={pathValues}
        pathLabel={pathLabel}
        index={index}
        isHovering={hoverIndex != null}
        accountSeries={accountSeries}
      />
    </>
  );
}
