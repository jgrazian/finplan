"use client";

import { Button, SegmentedControl } from "@/components/ui";
import type { ScaleKind } from "@/components/charts";
import type { ChartView } from "./types";
import type { Percentile } from "@/lib/types";

const VIEW_OPTIONS = [
  { value: "fan" as const, label: "Fan" },
  { value: "stack" as const, label: "By account" },
  { value: "bar" as const, label: "Bars" },
];

const SCALE_OPTIONS = [
  { value: "linear" as const, label: "Linear" },
  { value: "log" as const, label: "Log" },
];

/** Title, subtitle, view and axis switches, and the `v` percentile cycle. */
export function ChartToolbar({
  title,
  subtitle,
  view,
  onViewChange,
  scaleKind,
  onScaleKindChange,
  percentile,
  onCyclePercentile,
}: {
  title: string;
  subtitle: string;
  view: ChartView;
  onViewChange: (v: ChartView) => void;
  scaleKind: ScaleKind;
  onScaleKindChange: (kind: ScaleKind) => void;
  percentile: Percentile;
  onCyclePercentile: () => void;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 16,
        marginBottom: 14,
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 12 }}>
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
      <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
        <SegmentedControl
          ariaLabel="Chart view"
          options={VIEW_OPTIONS}
          value={view}
          onChange={onViewChange}
        />
        <SegmentedControl
          ariaLabel="Value axis"
          options={SCALE_OPTIONS}
          value={scaleKind}
          onChange={onScaleKindChange}
        />
        <Button shortcut="v" onClick={onCyclePercentile}>
          {percentile.toUpperCase()}
        </Button>
      </div>
    </div>
  );
}
