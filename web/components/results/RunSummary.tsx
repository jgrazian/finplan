import { SectionHeading, Stat } from "@/components/ui";
import { fmtCompact, fmtInt } from "@/lib/format";
import type { MonteCarloStats, NetWorthBands } from "@/lib/types";

/** The rail's run statistics. */
export function RunSummary({
  stats,
  bands,
}: {
  stats: MonteCarloStats;
  bands: NetWorthBands;
}) {
  const last = bands.p50.length - 1;
  return (
    <div>
      <SectionHeading className="mb-[10px]">Run</SectionHeading>
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
      </div>
    </div>
  );
}
