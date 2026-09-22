import { StatLabel } from "@/components/ui";
import { fmtInt } from "@/lib/format";

/** Funding across the path and terminal wealth are different measurements. */
export function SuccessRate({
  successRate,
  fundingSuccessRate,
  iterations,
}: {
  /** The legacy metric: fraction with positive final net worth. */
  successRate: number;
  /** Absent on runs that predate checkpoint funding checks. */
  fundingSuccessRate?: number;
  iterations: number;
}) {
  const measured = fundingSuccessRate != null;
  const fraction = fundingSuccessRate ?? 0;
  const pct = fraction * 100;
  const passed = Math.round(iterations * fraction);
  const other = iterations - passed;
  const label = "Cash funding check";

  return (
    <section aria-label="Simulation outcome definitions" style={{ marginBottom: 20 }}>
      <div style={{ display: "flex", alignItems: "flex-end", gap: 32, flexWrap: "wrap" }}>
        <div>
          <StatLabel>{label}</StatLabel>
          <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
            <span
              style={{
                fontFamily: "var(--font-heading)",
                fontWeight: 600,
                fontSize: 64,
                lineHeight: 1,
              }}
            >
              {measured ? pct.toFixed(1) : "—"}
            </span>
            <span style={{ fontSize: 24 }}>%</span>
          </div>
          <div style={{ fontSize: 12 }}>{fmtInt(iterations)} Monte Carlo iterations</div>
        </div>
        {measured && <div style={{ flex: 1, minWidth: 200, paddingBottom: 6 }}>
          <div
            style={{ display: "flex", height: 10, border: "1px solid var(--color-divider)" }}
            role="img"
            aria-label={`${label}: ${measured ? pct.toFixed(1) : "—"}%`}
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
              gap: 12,
              fontSize: 12,
              marginTop: 6,
            }}
          >
            <span>
              {fmtInt(passed)} {measured ? "passed the funding check" : "ended above zero"}
            </span>
            <span>
              {fmtInt(other)} {measured ? "had a shortfall or event warning" : "ended at or below zero"}
            </span>
          </div>
        </div>}
      </div>
      <p style={{ fontSize: 13 }}>Positive ending net worth: {(successRate * 100).toFixed(1)}%. Cash funding checks modeled cash balances and event-processing warnings. It does not detect omitted spending or guarantee future outcomes.</p>
      {!measured && (
        <p style={{ fontSize: 13, margin: "10px 0 0", maxWidth: 850 }}>
          Not measured — rerun to measure cash funding.
        </p>
      )}
    </section>
  );
}
