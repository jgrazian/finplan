"use client";

import { fmtCurrency } from "@/lib/format";
import type { Account, AccountId } from "@/lib/types";
import { KIND_LABEL, kindOf } from "./accountKind";
import { taxBadge } from "./taxStatus";

/**
 * The account list on a phone: one two-line row per account instead of the
 * seven-column table. The colour bar on the left stands in for the legend the
 * net-worth card drops. No drag handle — reordering stays a desktop affair.
 */
export function AccountsList({
  accounts,
  colors,
  selectedId,
  onSelect,
}: {
  accounts: Account[];
  colors: Map<AccountId, string>;
  selectedId: AccountId | undefined;
  onSelect: (id: AccountId) => void;
}) {
  return (
    <ul className="portfolio-list" aria-label="Accounts">
      {accounts.map((a) => (
        <li key={a.accountId}>
          <button
            type="button"
            className="portfolio-list-row"
            aria-current={a.accountId === selectedId || undefined}
            onClick={() => onSelect(a.accountId)}
          >
            <i
              className="portfolio-list-swatch"
              style={{ background: colors.get(a.accountId) }}
              aria-hidden
            />
            <span className="portfolio-list-text">
              <span className="portfolio-list-name">{a.name}</span>
              <span className="portfolio-list-meta">
                {KIND_LABEL[kindOf(a)]}
                {a.taxStatus && ` · ${taxBadge(a).label}`}
              </span>
            </span>
            <span className="portfolio-list-figure">{fmtCurrency(a.balance)}</span>
            <span className="portfolio-list-chevron" aria-hidden>
              ›
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}
