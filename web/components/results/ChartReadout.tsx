import { InlineStat } from "@/components/ui";
import { Legend } from "@/components/charts";
import { fmtCompact } from "@/lib/format";
import type { AccountSeries, NetWorthBands } from "@/lib/types";

/**
 * The row under the chart: the hovered (or final) year's three percentiles on
 * the left, the account legend on the right. Fixed min-height so switching
 * views never shifts the panels below it.
 */
export function ChartReadout({
  bands,
  pathValues,
  pathLabel,
  index,
  isHovering,
  accountSeries,
}: {
  bands: NetWorthBands;
  pathValues: number[];
  pathLabel: string;
  index: number;
  isHovering: boolean;
  accountSeries: AccountSeries[];
}) {
  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 20,
        marginTop: 14,
        minHeight: 52,
      }}
    >
      <div style={{ display: "flex", flexWrap: "wrap", gap: 26 }}>
        <InlineStat
          label={isHovering ? "year / age" : "end of plan"}
          value={bands.years[index] == null ? "—" : `${bands.years[index]}${bands.ages[index] === bands.years[index] ? "" : ` · age ${bands.ages[index]}`}`}
        />
        <InlineStat label={pathLabel} value={fmtCompact(pathValues[index])} emphasis />
        <InlineStat label="pointwise p5" value={fmtCompact(bands.p5[index])} />
        <InlineStat label="pointwise p50" value={fmtCompact(bands.p50[index])} emphasis />
        <InlineStat label="pointwise p95" value={fmtCompact(bands.p95[index])} />
      </div>
      <Legend items={accountSeries.map((s) => ({ label: s.label, color: s.color }))} />
    </div>
  );
}
