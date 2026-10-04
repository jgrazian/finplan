"use client";

import { fmtCurrency, fmtPercent } from "@/lib/format";
import type { DrawdownView, YearPanel } from "@/lib/view/drawdown";
import { fmtCompact } from "@/lib/format";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";
const FAINT = "color-mix(in srgb, var(--color-text) 48%, transparent)";
const LABEL = {
  fontSize: 11,
  letterSpacing: "0.06em",
  textTransform: "uppercase",
  color: FAINT,
} as const;

/** The selected year: target, where each dollar came from, and the lifetime split. */
export function DrawdownSide({
  view,
  panel,
  pinned,
  onClear,
  fixedSweeps,
}: {
  view: DrawdownView;
  panel: YearPanel | undefined;
  pinned: boolean;
  onClear: () => void;
  fixedSweeps: Array<{ event_id: number; name: string }>;
}) {
  return (
    <aside
      aria-live="polite"
      aria-label="Selected year"
      style={{ display: "flex", flexDirection: "column", gap: 14, minWidth: 0 }}
    >
      {panel && (
        <>
          <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
            <span style={{ fontSize: 13, fontWeight: 600, textTransform: "uppercase" }}>{panel.year}</span>
            {panel.age != null && <span style={{ fontSize: 13, color: MUTED }}>age {panel.age}</span>}
            <span style={{ marginLeft: "auto", fontSize: 12, color: MUTED }}>
              {pinned ? (
                <>
                  Pinned ·{" "}
                  <button type="button" onClick={onClear} style={{ all: "unset", cursor: "pointer", textDecoration: "underline" }}>
                    clear
                  </button>
                </>
              ) : (
                "Click to pin"
              )}
            </span>
          </div>
          <div>
            <div style={LABEL}>Target spend</div>
            <div style={{ fontFamily: "var(--font-heading)", fontSize: 30, lineHeight: 1.15 }}>
              {fmtCurrency(panel.target)}
            </div>
          </div>
          <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 13 }}>
            <thead>
              <tr style={LABEL}>
                <th style={{ textAlign: "left", fontWeight: 500, padding: "0 0 6px" }}>Source</th>
                <th style={{ textAlign: "right", fontWeight: 500, padding: "0 8px 6px" }}>Share</th>
                <th style={{ textAlign: "right", fontWeight: 500, padding: "0 0 6px" }}>Amount</th>
              </tr>
            </thead>
            <tbody>
              {panel.rows.map((r) => (
                <tr key={r.key} style={{ opacity: r.dim ? 0.45 : 1, borderTop: "1px solid var(--color-divider)" }}>
                  <td style={{ padding: "6px 0" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
                      <i aria-hidden style={{ width: 10, height: 10, background: r.color, flex: "none" }} />
                      <span>{r.name}</span>
                    </div>
                    <div style={{ fontSize: 12, color: FAINT, marginLeft: 17 }}>{r.sub}</div>
                  </td>
                  <td style={{ textAlign: "right", padding: "6px 8px", fontVariantNumeric: "tabular-nums" }}>
                    {Math.round(r.share * 100)}%
                  </td>
                  <td style={{ textAlign: "right", fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>
                    {fmtCurrency(r.amount)}
                  </td>
                </tr>
              ))}
              <tr style={{ borderTop: "1px solid var(--color-text)" }}>
                <td style={{ padding: "6px 0", fontWeight: 600 }}>Funded</td>
                <td style={{ textAlign: "right", padding: "6px 8px", fontVariantNumeric: "tabular-nums" }}>
                  {Math.round(panel.funded * 100)}%
                </td>
                <td style={{ textAlign: "right", fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>
                  {fmtCurrency(panel.fundedAmount)}
                </td>
              </tr>
              {panel.taxes && (
                <tr style={{ borderTop: "1px dashed var(--color-divider)" }}>
                  <td style={{ padding: "6px 0" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
                      <i
                        aria-hidden
                        style={{
                          width: 10,
                          height: 10,
                          background: panel.taxes.color,
                          border: "1px solid color-mix(in srgb, var(--color-danger) 70%, transparent)",
                          boxSizing: "border-box",
                          flex: "none",
                        }}
                      />
                      <span>Tax on withdrawals</span>
                    </div>
                    <div style={{ fontSize: 12, color: FAINT, marginLeft: 17 }}>Withheld · share of spending</div>
                  </td>
                  <td style={{ textAlign: "right", padding: "6px 8px", fontVariantNumeric: "tabular-nums" }}>
                    {Math.round(panel.taxes.ofSpending * 100)}%
                  </td>
                  <td style={{ textAlign: "right", fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>
                    {fmtCurrency(panel.taxes.amount)}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
          {panel.rmd && (
            <div style={{ display: "flex", justifyContent: "space-between", gap: 8, fontSize: 13 }}>
              <span style={{ color: MUTED }}>
                Required minimum distribution
                <span style={{ display: "block", fontSize: 12, color: FAINT }}>
                  {panel.rmd.accounts.join(", ")} · {fmtCurrency(panel.rmd.afterTax)} after tax
                </span>
              </span>
              <b style={{ fontVariantNumeric: "tabular-nums" }}>{fmtCurrency(panel.rmd.amount)}</b>
            </div>
          )}
          <p style={{ margin: 0, fontSize: 11.5, color: FAINT }}>
            Account amounts are after the tax withheld on them. The tax is drawn below zero and is
            not part of the funded total.
          </p>
          {panel.note && (
            <p
              style={{
                margin: 0,
                fontSize: 13,
                color: panel.note.tone === "bad" ? "var(--color-danger)" : "var(--color-text)",
              }}
            >
              {panel.note.text}
            </p>
          )}
        </>
      )}

      <div style={{ borderTop: "1px solid var(--color-divider)", paddingTop: 12 }}>
        <div style={LABEL}>Over retirement</div>
        <div
          role="img"
          aria-label={view.lifetime.map((l) => `${l.name} ${fmtPercent(l.share, 0)}`).join(", ")}
          style={{ display: "flex", height: 12, gap: 1, margin: "8px 0" }}
        >
          {view.lifetime.map((l) => (
            <div key={l.key} title={l.name} style={{ flex: `${l.share} 0 0`, background: l.color }} />
          ))}
        </div>
        <ul
          style={{
            listStyle: "none",
            margin: 0,
            padding: 0,
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: "3px 12px",
            fontSize: 12,
            color: MUTED,
          }}
        >
          {view.lifetime.map((l) => (
            <li key={l.key} style={{ display: "flex", gap: 6, alignItems: "center", minWidth: 0 }}>
              <i aria-hidden style={{ width: 8, height: 8, background: l.color, flex: "none" }} />
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{l.name}</span>
              <span style={{ marginLeft: "auto", fontVariantNumeric: "tabular-nums" }}>
                {fmtPercent(l.share, 0)}
              </span>
            </li>
          ))}
        </ul>
        {view.lifetimeTax > 0.5 && (
          <div style={{ display: "flex", justifyContent: "space-between", marginTop: 10, fontSize: 13 }}>
            <span style={{ color: MUTED }}>
              Tax on withdrawals · {fmtPercent(view.lifetimeTaxShare, 0)} of spending
            </span>
            <b style={{ fontFamily: "var(--font-heading)" }}>{fmtCompact(view.lifetimeTax)}</b>
          </div>
        )}
        <div style={{ display: "flex", justifyContent: "space-between", marginTop: 6, fontSize: 13 }}>
          <span style={{ color: MUTED }}>
            Balance at {view.endAge ?? view.endYear}
          </span>
          <b
            style={{ fontFamily: "var(--font-heading)" }}
            title={`After tax on pre-tax money: ${fmtCompact(view.endBalanceAfterTax)}`}
          >
            {fmtCompact(view.endBalance)}
          </b>
        </div>
      </div>

      {fixedSweeps.length > 0 && (
        <p style={{ margin: 0, fontSize: 12, color: MUTED }}>
          {fixedSweeps.map((s) => s.name).join(", ")}{" "}
          {fixedSweeps.length === 1 ? "always sells" : "always sell"} from a fixed account; strategies
          don&apos;t change {fixedSweeps.length === 1 ? "it" : "them"}.
        </p>
      )}
    </aside>
  );
}
