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
    <div className="chart-readout">
      <div className="chart-readout-stats">
        <InlineStat
          label={isHovering ? "year / age" : "end of plan"}
          value={bands.years[index] == null ? "—" : `${bands.years[index]}${bands.ages[index] === bands.years[index] ? "" : ` · age ${bands.ages[index]}`}`}
        />
        <InlineStat label={pathLabel} value={fmtCompact(pathValues[index])} emphasis />
        <InlineStat
          label={`pointwise p${bands.outer[0]}–p${bands.outer[1]}`}
          value={range(bands.low[index], bands.high[index])}
        />
        {/* The phone keeps three cells — year, path, outer band — and drops
            the two below, which the envelope itself already draws. */}
        {bands.upperQuartile.length > 0 && (
          <div className="chart-readout-extra">
            <InlineStat
              label="pointwise p25–p75"
              value={range(bands.lowerQuartile[index], bands.upperQuartile[index])}
            />
          </div>
        )}
        <div className="chart-readout-extra">
          <InlineStat label="pointwise p50" value={fmtCompact(bands.p50[index])} emphasis />
        </div>
      </div>
      <Legend items={accountSeries.map((s) => ({ label: s.label, color: s.color }))} />
    </div>
  );
}

/** A band's two edges as one value: `$1.2M – $3.4M`. */
function range(low: number | undefined, high: number | undefined): string {
  return low == null || high == null ? "—" : `${fmtCompact(low)} – ${fmtCompact(high)}`;
}
