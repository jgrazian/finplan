"use client";

import { Blueprint, StatLabel } from "@/components/ui";
import { fmtCompactOrExact, fmtCurrency, fmtShare, fmtShareFine } from "@/lib/format";
import { useIsMobile } from "@/lib/hooks/useIsMobile";
import type { PortfolioSummary as SummaryData } from "@/lib/view/accounts";

/**
 * The two figures the accounts list is an itemisation of — what the portfolio
 * is worth and how the investable part of it is taxed — on one strip.
 *
 * It sits above the table rather than in the rail because it describes every
 * row at once — the rail is about whichever one is selected. Both breakdowns
 * are one stacked bar each, so the strip stays a single line of height and the
 * table starts higher on the page.
 */
export function PortfolioSummary({ summary }: { summary: SummaryData }) {
  const mobile = useIsMobile();
  if (mobile) return <CompactSummary summary={summary} />;
  const pool = summary.cash + summary.invested;
  return (
    <Blueprint className="ledger-strip" corners={false}>
      <div>
        <StatLabel>Net worth</StatLabel>
        <div className="ledger-strip-figure">{fmtCurrency(summary.netWorth)}</div>
        <div className="ledger-strip-meta">
          Assets <strong>{fmtCurrency(summary.assets)}</strong> · Debt{" "}
          {fmtCurrency(-summary.debt)}
        </div>
      </div>

      <div className="ledger-strip-block">
        <StatLabel>By account</StatLabel>
        <StackedBar segments={summary.slices.map((s) => ({ key: s.accountId, ...s }))} />
        <div className="ledger-strip-legend">
          {summary.slices.map((slice) => (
            <span key={slice.accountId}>
              <i style={{ background: slice.color }} aria-hidden />
              {slice.label}
            </span>
          ))}
        </div>
      </div>

      <div className="ledger-strip-block">
        <div className="ledger-strip-head">
          <StatLabel>Tax treatment</StatLabel>
          {/* The engine has no asset classes, so an equity/bond split would
              be a guess. What it does know is which of the pool is marked to
              holdings and which is idle cash. */}
          <span>
            Cash / invested{" "}
            <strong>
              {pool > 0
                ? `${fmtShare(summary.cash / pool)} / ${fmtShare(summary.invested / pool)}`
                : "—"}
            </strong>
          </span>
        </div>
        <StackedBar
          segments={summary.taxBands.map((b) => ({ key: b.label, ...b }))}
        />
        <div className="ledger-strip-legend">
          {summary.taxBands.map((band) => (
            <span key={band.label}>
              <i style={{ background: band.color }} aria-hidden />
              {band.label} <strong>{fmtCompactOrExact(band.value)}</strong>
            </span>
          ))}
        </div>
      </div>
    </Blueprint>
  );
}

/**
 * One bar split by share, with a hairline of the card between segments so
 * neighbouring colours never run together. Each segment names itself on hover.
 */
function StackedBar({
  segments,
}: {
  segments: { key: string; label: string; share: number; color: string; value?: number }[];
}) {
  const shown = segments.filter((s) => s.share > 0);
  return (
    <div className="ledger-bar">
      {shown.map((s) => (
        <i
          key={s.key}
          style={{ width: `${s.share * 100}%`, background: s.color }}
          title={`${s.label} · ${fmtShareFine(s.share)}${
            s.value != null ? ` · ${fmtCompactOrExact(s.value)}` : ""
          }`}
        />
      ))}
    </div>
  );
}

/**
 * The phone's version: net worth and tax treatment merged into one card. The
 * legend goes — each account row carries its own colour bar — and the tax
 * bands become three figures side by side rather than three bars.
 */
function CompactSummary({ summary }: { summary: SummaryData }) {
  const pool = summary.cash + summary.invested;
  return (
    <Blueprint className="portfolio-summary-card">
      <StatLabel>Net worth</StatLabel>
      <div className="portfolio-summary-value">{fmtCurrency(summary.netWorth)}</div>
      <div className="portfolio-summary-meta">
        Assets {fmtCurrency(summary.assets)} · Debt {fmtCurrency(-summary.debt)}
      </div>

      {summary.slices.length > 0 && (
        <div className="portfolio-summary-strip" aria-hidden>
          {summary.slices.map((slice) => (
            <i
              key={slice.accountId}
              style={{ width: `${slice.share * 100}%`, background: slice.color }}
            />
          ))}
        </div>
      )}

      <div className="portfolio-summary-bands">
        {summary.taxBands.map((band) => (
          <div key={band.label}>
            <span>{band.label}</span>
            <strong>{fmtCompactOrExact(band.value)}</strong>
          </div>
        ))}
      </div>

      <div className="portfolio-summary-foot">
        <span>Cash / invested</span>
        <span>
          {pool > 0
            ? `${fmtShare(summary.cash / pool)} / ${fmtShare(summary.invested / pool)}`
            : "—"}
        </span>
      </div>
    </Blueprint>
  );
}
