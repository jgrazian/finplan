import { SectionHeading, Stat } from "@/components/ui";
import { fmtCompact, fmtInt } from "@/lib/format";
import type { MonteCarloStats, NetWorthBands } from "@/lib/types";

/**
 * The rail's run statistics.
 *
 * Every dollar here is real, in the plan's first-year money. The heading says
 * which year that is, and the inflation stat says how far the nominal figures
 * the engine produced were from these — without it, a reader who knows what
 * the engine reports has no way to reconcile the two.
 */
export function RunSummary({
  stats,
  bands,
  baseYear,
  totalInflation,
}: {
  stats: MonteCarloStats;
  bands: NetWorthBands;
  baseYear: number;
  /** Cumulative inflation over the horizon, e.g. 2.4 for prices multiplying by 2.4. */
  totalInflation: number;
}) {
  const last = bands.p50.length - 1;
  return (
    <div>
      <SectionHeading className="mb-[10px]">Run · {baseYear} dollars</SectionHeading>
      <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        <Stat label="iterations" value={fmtInt(stats.numIterations)} />
        {stats.convergenceMetric && (
          <Stat
            label={`converged on ${stats.convergenceMetric}`}
            value={
              stats.convergenceValue == null
                ? (stats.converged ? "yes" : "no")
                : stats.convergenceValue.toPrecision(3)
            }
          />
        )}
        <Stat label="median final" value={fmtCompact(bands.p50[last])} />
        <Stat label="p5 final" value={fmtCompact(bands.p5[last])} />
        <Stat label="lifetime taxes" value={fmtCompact(stats.lifetimeTaxes)} />
        <Stat label="prices over the plan" value={`×${totalInflation.toFixed(2)}`} />
      </div>
    </div>
  );
}
