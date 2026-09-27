import type { ReactNode } from "react";
import { StatLabel } from "@/components/ui";
import { fmtInt } from "@/lib/format";

/** Funding across the path and terminal wealth are different measurements. */
export function SuccessRate({
  fundingSuccessRate,
  iterations,
  action,
}: {
  /** Absent on runs that predate checkpoint funding checks. */
  fundingSuccessRate?: number;
  iterations: number;
  /**
   * Set beside the label, right-aligned. The phone layout puts the iteration
   * menu here, since the rail it lives in on desktop is below the fold.
   */
  action?: ReactNode;
}) {
  const measured = fundingSuccessRate != null;
  const fraction = fundingSuccessRate ?? 0;
  const pct = fraction * 100;
  const passed = Math.round(iterations * fraction);
  const other = iterations - passed;
  const label = "Cash funding check";

  return (
    <section aria-label="Simulation outcome definitions" style={{ marginBottom: 20 }}>
      <div
        className="success-rate"
        style={{ display: "flex", alignItems: "flex-end", gap: 32, flexWrap: "wrap" }}
      >
        <div className="success-figure">
          {action ? (
            <div className="success-head">
              <StatLabel>{label}</StatLabel>
              {action}
            </div>
          ) : (
            <StatLabel>{label}</StatLabel>
          )}
          <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
            <span
              className="success-pct"
              style={{
                fontFamily: "var(--font-heading)",
                fontWeight: 600,
                fontSize: 64,
                lineHeight: 1,
              }}
            >
              {measured ? pct.toFixed(1) : "—"}
            </span>
            <span className="success-pct-unit" style={{ fontSize: 24 }}>
              %
            </span>
          </div>
          <div style={{ fontSize: 12 }}>{fmtInt(iterations)} Monte Carlo iterations</div>
        </div>
        {measured && <div className="success-bar" style={{ flex: 1, minWidth: 200, paddingBottom: 6 }}>
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
            {/* The phone keeps the counts and the first word; the rest of
                each phrase is the desktop's room to explain itself. */}
            <span>
              {fmtInt(passed)} passed<span className="success-long"> the funding check</span>
            </span>
            <span>
              {fmtInt(other)} <span className="success-long">had a </span>shortfall
              <span className="success-long"> or event warning</span>
              <span className="success-short">…</span>
            </span>
          </div>
        </div>}
      </div>
      {!measured && (
        <p style={{ fontSize: 13, margin: "10px 0 0", maxWidth: 850 }}>
          Not measured — rerun to measure cash funding.
        </p>
      )}
    </section>
  );
}
