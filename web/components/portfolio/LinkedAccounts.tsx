"use client";

import { Blueprint, SectionHeading } from "@/components/ui";
import { fmtCurrency, fmtPercent } from "@/lib/format";
import type { Account, AccountId } from "@/lib/types";

const MUTED = "color-mix(in srgb, var(--color-text) 55%, transparent)";

/**
 * Block 5 — the other side of the pair.
 *
 * A property and the debt encumbering it are one object seen from two ends, so
 * each end carries a compact card for the other rather than a second full
 * drawer, and either one opens the other.
 *
 * Nothing in the schema draws that edge: what pairs them is an event that
 * settles both, which is also the only thing that could act on the pair. So the
 * card names the event it read the link from.
 */
export function LinkedAccounts({
  account,
  onSelect,
}: {
  account: Account;
  onSelect: (id: AccountId) => void;
}) {
  const fromProperty = account.flavor === "Property";
  const owed = account.linked.reduce((sum, l) => sum - Math.min(l.balance, 0), 0);

  return (
    <div>
      <SectionHeading className="mb-[6px]">
        {fromProperty ? "Linked liability" : "Linked property"}
      </SectionHeading>

      {account.linked.length === 0 ? (
        <span className="text-muted" style={{ fontSize: 12 }}>
          Not paired — an event that settles this and a{" "}
          {fromProperty ? "debt" : "property"} together would link them.
        </span>
      ) : (
        <>
          {account.linked.map((link) => (
            <Blueprint
              key={link.accountId}
              corners={false}
              style={{ padding: "11px 12px 12px", marginBottom: 6 }}
            >
              <div
                style={{
                  display: "flex",
                  alignItems: "baseline",
                  justifyContent: "space-between",
                  gap: 10,
                  fontSize: 12.5,
                }}
              >
                <button
                  type="button"
                  className="linkbtn"
                  onClick={() => onSelect(link.accountId)}
                >
                  {link.name}
                </button>
                <span>{fmtCurrency(link.balance)}</span>
              </div>
              <div style={{ fontSize: 11, marginTop: 5, color: MUTED }}>
                {link.interestRate != null && `${fmtPercent(link.interestRate, 2)} · `}
                through {link.through.join(", ")}
              </div>
            </Blueprint>
          ))}
          {fromProperty && (
            <div style={{ fontSize: 11.5, marginTop: 6, color: MUTED }}>
              Equity {fmtCurrency(account.balance - owed)} — a sale event settles both
              sides.
            </div>
          )}
        </>
      )}
    </div>
  );
}
