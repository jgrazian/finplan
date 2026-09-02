"use client";

import { CurrencyInput } from "@/components/ui";
import { fmtCurrency } from "@/lib/format";
import type { Account } from "@/lib/types";
import type { AccountDraft, ChangedFields, SetDraft } from "./accountDraft";
import { DirtyField } from "./DirtyField";

const MUTED = "color-mix(in srgb, var(--color-text) 55%, transparent)";

/** What the account's one stored figure is called, and where the rest comes from. */
function copy(draft: AccountDraft): { label: string; note: string } {
  switch (draft.kind) {
    case "cash":
      return {
        label: "Balance",
        note: "Interest is modelled as ordinary income each year.",
      };
    case "property":
      return { label: "Market value", note: "Grows by the profile of the asset backing it." };
    case "debt":
      return { label: "Principal left", note: "Owed, so the list shows it as a debt." };
    case "investment":
    case "retirement":
      // Handled by the summed branch below; an investment account's total is
      // not a figure anyone types.
      return { label: "Balance", note: "" };
  }
}

/**
 * Block 3 — the balance, and where it comes from.
 *
 * Present for every kind, but only ever one of two things: a figure the account
 * stores, which is an input, or one summed from its lots and its cash, which is
 * not. Naming the provenance is what keeps a read-only total from looking like
 * a field that stopped working.
 */
export function AccountValuation({
  account,
  draft,
  pristine,
  changed,
  set,
  offline,
}: {
  account: Account;
  draft: AccountDraft;
  pristine: AccountDraft;
  changed: ChangedFields;
  set: SetDraft;
  offline?: boolean;
}) {
  if (draft.kind === "investment" || draft.kind === "retirement") {
    const lots = account.positions.length;
    // The cash half is being edited in Terms, so the total follows the draft
    // rather than lagging a save behind it.
    const total = account.balance - pristine.amount + draft.amount;
    const from =
      lots === 0
        ? "cash only"
        : draft.amount === 0
          ? "marked from lots"
          : "marked from lots, plus cash";
    return (
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          justifyContent: "space-between",
          gap: 10,
          fontSize: 11.5,
          color: MUTED,
        }}
      >
        <span>Balance · {from}</span>
        <span
          style={{
            fontFamily: "var(--font-heading)",
            fontWeight: 600,
            fontSize: 18,
            color: "var(--color-text)",
          }}
        >
          {fmtCurrency(total)}
        </span>
      </div>
    );
  }

  const { label, note } = copy(draft);
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "1fr 1fr",
        gap: 10,
        alignItems: "end",
      }}
    >
      <DirtyField label={label} changed={changed.amount}>
        <CurrencyInput
          style={{ minHeight: 32 }}
          value={draft.amount}
          readOnly={offline}
          onValueChange={(amount) => set("amount", amount)}
          aria-label={label}
        />
        {changed.amount && (
          <div className="field-was">was {fmtCurrency(pristine.amount)}</div>
        )}
      </DirtyField>
      <div style={{ fontSize: 11, paddingBottom: 8, color: MUTED }}>{note}</div>
    </div>
  );
}
