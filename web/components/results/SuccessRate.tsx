import { StatLabel } from "@/components/ui";
import { fmtInt } from "@/lib/format";

/**
 * The payoff figure. One large number rather than a dial, so it does not
 * compete with the chart below it, paired with a bar that shows the same
 * fraction as area.
 */
export function SuccessRate({
  successRate,
  iterations,
  converged,
  finalAge,
}: {
  successRate: number;
  iterations: number;
  converged?: boolean;
  finalAge: number;
}) {
  const pct = successRate * 100;
  const lasted = Math.round(iterations * successRate);
  const ranDry = iterations - lasted;

  return (
    <div style={{ display: "flex", alignItems: "flex-end", gap: 40, marginBottom: 20 }}>
      <div>
        <StatLabel>probability of success</StatLabel>
        <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
          <span
            style={{
              fontFamily: "var(--font-heading)",
              fontWeight: 600,
              fontSize: 64,
              lineHeight: 0.9,
            }}
          >
            {pct.toFixed(1)}
          </span>
          <span
            style={{
              fontFamily: "var(--font-heading)",
              fontSize: 24,
              color: "var(--color-accent-700)",
            }}
          >
            %
          </span>
        </div>
        <div
          style={{
            fontSize: 12,
            color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
          }}
        >
          {fmtInt(iterations)} Monte Carlo iterations
          {converged ? " · converged" : ""}
        </div>
      </div>

      <div style={{ flex: 1, paddingBottom: 6 }}>
        <div
          style={{ display: "flex", height: 10, border: "1px solid var(--color-divider)" }}
          role="img"
          aria-label={`${pct.toFixed(1)}% of runs succeeded`}
        >
          <div style={{ width: `${pct}%`, background: "var(--color-accent)" }} />
          <div
            style={{
              flex: 1,
              background:
                "repeating-linear-gradient(135deg, transparent 0 3px, color-mix(in srgb, var(--color-text) 22%, transparent) 3px 4px)",
            }}
          />
        </div>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            fontSize: 11,
            marginTop: 6,
            color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
          }}
        >
          <span>
            {fmtInt(lasted)} plans lasted through age {finalAge}
          </span>
          <span>{fmtInt(ranDry)} ran dry</span>
        </div>
      </div>
    </div>
  );
}
