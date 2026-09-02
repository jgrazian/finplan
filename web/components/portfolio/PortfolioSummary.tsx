import { Blueprint, StatLabel } from "@/components/ui";
import { fmtCompactOrExact, fmtCurrency, fmtShare } from "@/lib/format";
import type { PortfolioSummary as SummaryData } from "@/lib/view/accounts";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";
const TRACK = "color-mix(in srgb, var(--color-text) 8%, transparent)";

/**
 * The two figures the accounts list is an itemisation of: what the portfolio is
 * worth, and how the investable half of it is taxed.
 *
 * They sit above the table rather than in the rail because they describe every
 * row at once — the rail is about whichever one is selected.
 */
export function PortfolioSummary({ summary }: { summary: SummaryData }) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "1fr 1fr",
        gap: 14,
        marginBottom: 18,
      }}
    >
      <NetWorthCard summary={summary} />
      <TaxTreatmentCard summary={summary} />
    </div>
  );
}

function NetWorthCard({ summary }: { summary: SummaryData }) {
  return (
    <Blueprint
      style={{
        padding: "14px 16px 15px",
        display: "flex",
        flexDirection: "column",
        gap: 10,
      }}
    >
      <div>
        <StatLabel>Net worth</StatLabel>
        <div
          style={{
            fontFamily: "var(--font-heading)",
            fontWeight: 600,
            fontSize: 30,
            lineHeight: 1.05,
            marginTop: 2,
          }}
        >
          {fmtCurrency(summary.netWorth)}
        </div>
      </div>

      {summary.slices.length > 0 && (
        <>
          <div style={{ display: "flex", height: 8, gap: 1 }} aria-hidden>
            {summary.slices.map((slice) => (
              <i
                key={slice.accountId}
                style={{
                  width: `${slice.share * 100}%`,
                  background: slice.color,
                  display: "block",
                }}
              />
            ))}
          </div>
          <div
            style={{
              display: "flex",
              flexWrap: "wrap",
              gap: "4px 12px",
              fontSize: 10.5,
              color: MUTED,
            }}
          >
            {summary.slices.map((slice) => (
              <span
                key={slice.accountId}
                style={{ display: "flex", alignItems: "center", gap: 4 }}
              >
                <i
                  style={{ width: 8, height: 8, background: slice.color, display: "block" }}
                  aria-hidden
                />
                {slice.label}
              </span>
            ))}
          </div>
        </>
      )}

      <div
        style={{
          display: "flex",
          gap: 18,
          fontSize: 11.5,
          paddingTop: 2,
          marginTop: "auto",
          borderTop: "1px solid var(--color-divider)",
        }}
      >
        <span>
          Assets <strong style={{ fontWeight: 500 }}>{fmtCurrency(summary.assets)}</strong>
        </span>
        <span style={{ color: MUTED }}>Debt {fmtCurrency(-summary.debt)}</span>
      </div>
    </Blueprint>
  );
}

function TaxTreatmentCard({ summary }: { summary: SummaryData }) {
  const pool = summary.cash + summary.invested;
  return (
    <Blueprint
      style={{
        padding: "14px 16px 15px",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      }}
    >
      <StatLabel>Tax treatment of investable assets</StatLabel>

      <div style={{ display: "flex", flexDirection: "column", gap: 9 }}>
        {summary.taxBands.map((band) => (
          <div
            key={band.label}
            style={{
              display: "grid",
              gridTemplateColumns: "74px 1fr 54px",
              gap: 8,
              alignItems: "center",
              fontSize: 11.5,
            }}
          >
            <span>{band.label}</span>
            <span style={{ display: "block", height: 10, background: TRACK }} aria-hidden>
              <i
                style={{
                  display: "block",
                  height: "100%",
                  width: `${band.share * 100}%`,
                  background: band.color,
                }}
              />
            </span>
            <span style={{ textAlign: "right", color: MUTED }}>
              {fmtCompactOrExact(band.value)}
            </span>
          </div>
        ))}
      </div>

      {/* The engine has no asset classes, so an equity/bond split would be a
          guess. What it does know is which of the pool is marked to holdings
          and which is idle cash. */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "baseline",
          fontSize: 11.5,
          paddingTop: 8,
          marginTop: "auto",
          borderTop: "1px solid var(--color-divider)",
        }}
      >
        <span style={{ color: MUTED }}>Cash / invested</span>
        <span>
          {pool > 0
            ? `${fmtShare(summary.cash / pool)} / ${fmtShare(summary.invested / pool)}`
            : "—"}
        </span>
      </div>
    </Blueprint>
  );
}
