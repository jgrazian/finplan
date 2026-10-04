"use client";

import { fmtCompact, fmtPercent } from "@/lib/format";
import type { DrawdownComparison } from "@/lib/api/types";
import { comparisonLines, type DrawdownBasis } from "@/lib/view/drawdown";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/** Success rate, median ending balance and median-path tax per strategy; best of each marked. */
export function StrategyComparison({
  comparison,
  loading,
  error,
  selectedKey,
  onSelect,
  basis,
}: {
  comparison: DrawdownComparison | undefined;
  loading: boolean;
  error?: string;
  selectedKey: string;
  onSelect: (key: string) => void;
  basis: DrawdownBasis;
}) {
  const lines = comparison ? comparisonLines(comparison, basis) : [];
  const best = (on: boolean) =>
    on ? { fontWeight: 700, color: "var(--color-accent-800)", background: "color-mix(in srgb, var(--color-accent) 12%, transparent)" } : {};
  return (
    <section aria-label="Strategy comparison" style={{ fontSize: 12.5 }}>
      <div style={{ color: MUTED, marginBottom: 6 }}>
        {comparison
          ? `${comparison.iterations} markets each, same markets for every strategy`
          : loading
            ? "Comparing strategies across simulated markets…"
            : (error ?? "")}
      </div>
      {comparison ? (
        <div style={{ overflowX: "auto", opacity: loading ? 0.6 : 1 }}>
          <table style={{ borderCollapse: "collapse", minWidth: "100%" }}>
            <thead>
              <tr style={{ color: MUTED, textAlign: "right" }}>
                <th style={{ textAlign: "left", fontWeight: 500, padding: "2px 10px 4px 0" }}>Strategy</th>
                <th style={{ fontWeight: 500, padding: "2px 10px 4px" }}>Plan succeeds</th>
                <th style={{ fontWeight: 500, padding: "2px 10px 4px" }}>
                  Median ending balance{basis === "real" ? " (today’s $)" : ""}
                </th>
                <th style={{ fontWeight: 500, padding: "2px 0 4px 10px" }}>Tax, median path</th>
              </tr>
            </thead>
            <tbody>
              {lines.map((l) => (
                <tr
                  key={l.key}
                  onClick={() => onSelect(l.key)}
                  style={{
                    cursor: "pointer",
                    borderTop: "1px solid var(--color-divider)",
                    fontWeight: l.key === selectedKey ? 600 : 400,
                  }}
                >
                  <td style={{ padding: "4px 10px 4px 0", whiteSpace: "nowrap" }}>{l.label}</td>
                  <td style={{ textAlign: "right", padding: "4px 10px", ...best(l.bestSuccess) }}>
                    {fmtPercent(l.success, 1)}
                  </td>
                  <td style={{ textAlign: "right", padding: "4px 10px", ...best(l.bestEnding) }}>
                    {fmtCompact(l.endingBalance)}
                  </td>
                  <td style={{ textAlign: "right", padding: "4px 0 4px 10px", ...best(l.bestTax) }}>
                    {l.tax == null ? "—" : fmtCompact(l.tax)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : loading ? (
        <div aria-hidden style={{ height: 120, background: "color-mix(in srgb, var(--color-text) 5%, transparent)" }} />
      ) : null}
    </section>
  );
}
