import { InlineStat } from "@/components/ui";
import { Legend } from "@/components/charts";
import { fmtCompact } from "@/lib/format";
import type { AccountSeries, NetWorthBands } from "@/lib/types";

/**
 * The row under the chart: the hovered (or final) year's bands and median on
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
        <InlineStat
          label={`pointwise p${bands.outer[0]}–p${bands.outer[1]}`}
          value={range(bands.low[index], bands.high[index])}
        />
        {bands.upperQuartile.length > 0 && (
          <InlineStat
            label="pointwise p25–p75"
            value={range(bands.lowerQuartile[index], bands.upperQuartile[index])}
          />
        )}
        <InlineStat label="pointwise p50" value={fmtCompact(bands.p50[index])} emphasis />
      </div>
      <Legend items={accountSeries.map((s) => ({ label: s.label, color: s.color }))} />
    </div>
  );
}

/** A band's two edges as one value: `$1.2M – $3.4M`. */
function range(low: number | undefined, high: number | undefined): string {
  return low == null || high == null ? "—" : `${fmtCompact(low)} – ${fmtCompact(high)}`;
}
