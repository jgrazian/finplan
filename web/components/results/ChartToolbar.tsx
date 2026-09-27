"use client";

import { SegmentedControl } from "@/components/ui";
import type { ScaleKind } from "@/components/charts";
import type { ChartView } from "./types";
import type { Percentile } from "@/lib/types";

const VIEW_OPTIONS = [
  { value: "fan" as const, label: "Envelope" },
  { value: "stack" as const, label: "By account" },
  { value: "bar" as const, label: "Bars" },
];

const SCALE_OPTIONS = [
  { value: "linear" as const, label: "Linear" },
  { value: "log" as const, label: "Log" },
];

/* Ranked low to high, the way the envelope is drawn, rather than in the order
   the old cycle button happened to visit them. */
const PATH_OPTIONS = [
  { value: "p10" as const, label: "P10" },
  { value: "p50" as const, label: "P50" },
  { value: "p90" as const, label: "P90" },
];

/** On a phone: no per-account stack and no log axis, which need the width. */
const COMPACT_VIEWS = new Set<ChartView>(["fan", "bar"]);

/** Title, subtitle, view and axis switches, and the selected-path picker. */
export function ChartToolbar({
  title,
  subtitle,
  view,
  onViewChange,
  scaleKind,
  onScaleKindChange,
  logDisabledReason,
  percentile,
  onPercentileChange,
  compact = false,
}: {
  title: string;
  subtitle: string;
  view: ChartView;
  onViewChange: (v: ChartView) => void;
  scaleKind: ScaleKind;
  logDisabledReason?: string;
  onScaleKindChange: (kind: ScaleKind) => void;
  percentile: Percentile;
  onPercentileChange: (percentile: Percentile) => void;
  /** Phone width: envelope and bars only, linear axis only. */
  compact?: boolean;
}) {
  return (
    <div
      className="chart-toolbar"
      style={{
        display: "flex",
        flexWrap: "wrap",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 16,
        marginBottom: 14,
      }}
    >
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-start",
          gap: 12,
          flex: "1 1 0",
          minWidth: 0,
        }}
      >
        <h4 style={{ margin: 0 }}>{title}</h4>
        <span
          style={{
            fontSize: 12,
            color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
          }}
        >
          {subtitle}
        </span>
      </div>
      {/* On a phone the switches share one row that scrolls sideways
          rather than wrapping mid-control. */}
      <div
        className="chart-toolbar-controls"
        style={{
          display: "flex",
          gap: 10,
          alignItems: "center",
          flex: "0 0 auto",
          marginLeft: "auto",
        }}
      >
        <SegmentedControl
          ariaLabel="Chart view"
          options={
            compact
              ? VIEW_OPTIONS.filter((o) => COMPACT_VIEWS.has(o.value))
              : VIEW_OPTIONS
          }
          value={view}
          onChange={onViewChange}
        />
        {!compact && (
          <SegmentedControl
            ariaLabel="Value axis"
            options={SCALE_OPTIONS.map((option) => ({
              ...option,
              disabled: option.value === "log" && logDisabledReason != null,
              title: option.value === "log" ? logDisabledReason : undefined,
            }))}
            value={scaleKind}
            onChange={onScaleKindChange}
          />
        )}
        <SegmentedControl
          ariaLabel="Selected path"
          options={PATH_OPTIONS}
          value={percentile}
          onChange={onPercentileChange}
        />
      </div>
    </div>
  );
}
