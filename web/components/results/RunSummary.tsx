import { SectionHeading, Stat } from "@/components/ui";
import { fmtCompact, fmtInt } from "@/lib/format";
import type { MonteCarloStats } from "@/lib/types";

/** Real distribution statistics are independent of the selected path. */
export function RunSummary({
  stats, baseDate, pathLabel, dollarLabel, totalInflation,
}: {
  stats: MonteCarloStats;
  baseDate: string;
  pathLabel: string;
  dollarLabel: string;
  totalInflation: number;
}) {
  const quantile = (p: number) => stats.percentileValues.find(([rank]) => rank === p)?.[1] ?? Number.NaN;
  // P10/P90 where the run measured them; P5/P95 on runs stored before.
  const [low, high] = stats.percentileValues.some(([rank]) => rank === 0.1) ? [0.1, 0.9] : [0.05, 0.95];
  const pct = (p: number) => `P${Math.round(p * 100)}`;
  return (
    <div>
      <SectionHeading className="mb-[10px]">All paths · {baseDate} dollars</SectionHeading>
      <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        <Stat label="iterations" value={fmtInt(stats.numIterations)} />
        {stats.convergenceMetric && (
          <Stat label={`converged on nominal ${stats.convergenceMetric}`} value={
            stats.convergenceValue == null ? (stats.converged ? "yes" : "no") : stats.convergenceValue.toPrecision(3)
          } />
        )}
        <Stat label="real median final (P50)" value={fmtCompact(quantile(0.5))} />
        <Stat label={`real ${pct(low)} final`} value={fmtCompact(quantile(low))} />
        <Stat label={`real ${pct(high)} final`} value={fmtCompact(quantile(high))} />
        <Stat label="real mean final" value={fmtCompact(stats.meanFinalNetWorth)} />
        {!stats.percentileValues.length && <p>Not measured for this run. Rerun for real terminal statistics.</p>}
        <SectionHeading>Selected path</SectionHeading>
        <p style={{ margin: 0, fontSize: 12 }}>{pathLabel} · {dollarLabel}</p>
        <Stat label="path lifetime taxes" value={fmtCompact(stats.lifetimeTaxes)} />
        <Stat label="path prices over the plan" value={Number.isFinite(totalInflation) ? `×${totalInflation.toFixed(2)}` : "—"} />
      </div>
    </div>
  );
}
