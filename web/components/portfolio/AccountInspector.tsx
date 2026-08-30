"use client";

import { Button, CompactInput, Field, Hr, Tag } from "@/components/ui";
import { fmtCurrency } from "@/lib/format";
import type { Account } from "@/lib/types";
import { PositionsTable } from "./PositionsTable";
import { ReferencedBy } from "./ReferencedBy";
import { contributionLimitLabel, taxBadge } from "./taxStatus";

/**
 * Editing model C — a persistent drawer rather than a modal. It inspects the
 * selected account without covering the list, so holdings and the account
 * itself sit in one surface.
 */
export function AccountInspector({
  account,
  onFieldChange,
  onRevert,
  onApply,
  onAddLot,
  onDelete,
  dirty,
}: {
  account: Account;
  onFieldChange?: (field: "balance" | "returnProfileId", value: string) => void;
  onRevert?: () => void;
  onApply?: () => void;
  /** Omitted where the flavor cannot hold lots. */
  onAddLot?: () => void;
  onDelete?: () => void;
  dirty?: boolean;
}) {
  const badge = taxBadge(account);

  return (
    <div
      style={{
        padding: "18px 18px 22px",
        display: "flex",
        flexDirection: "column",
        gap: 14,
        height: "100%",
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
        <h5 style={{ margin: 0 }}>{account.name}</h5>
        <Tag tone={badge.tone}>{badge.label}</Tag>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
        <Field label="Flavor">
          <CompactInput value={account.flavor} readOnly />
        </Field>
        <Field label="Balance">
          <CompactInput
            value={fmtCurrency(account.balance)}
            readOnly={!onFieldChange}
            onChange={(e) => onFieldChange?.("balance", e.target.value)}
          />
        </Field>
        <Field label="Return profile">
          <CompactInput
            value={account.returnProfileId}
            readOnly={!onFieldChange}
            onChange={(e) => onFieldChange?.("returnProfileId", e.target.value)}
          />
        </Field>
        <Field label="Contribution limit">
          <CompactInput value={contributionLimitLabel(account)} readOnly />
        </Field>
      </div>

      <Hr flush />
      <PositionsTable lots={account.positions} onAddLot={onAddLot} />
      <Hr flush />
      <ReferencedBy eventIds={account.referencedBy} />

      <div style={{ display: "flex", gap: 8, marginTop: "auto" }}>
        {onDelete && (
          <Button variant="ghost" onClick={onDelete}>
            Delete
          </Button>
        )}
        <Button style={{ marginLeft: "auto" }} onClick={onRevert} disabled={!dirty}>
          Revert
        </Button>
        <Button variant="primary" onClick={onApply} disabled={!dirty}>
          Apply
        </Button>
      </div>
    </div>
  );
}
