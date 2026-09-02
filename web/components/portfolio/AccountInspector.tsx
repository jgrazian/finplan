"use client";

import { type ReactNode, useState } from "react";
import { Blueprint, Button, Dropdown, Tag } from "@/components/ui";
import type { Asset, Profile } from "@/lib/api/types";
import type { Account, AccountId } from "@/lib/types";
import { AccountTerms } from "./AccountTerms";
import { AccountValuation } from "./AccountValuation";
import { LinkedAccounts } from "./LinkedAccounts";
import { PositionsTable } from "./PositionsTable";
import { ReferencedBy } from "./ReferencedBy";
import {
  type AccountDraft,
  changedFields,
  draftOf,
  pendingCount,
} from "./accountDraft";
import {
  type AccountKind,
  KINDS,
  KIND_LABEL,
  conversionWarning,
  isReachable,
  taxStatusFor,
} from "./accountKind";
import { DirtyField } from "./DirtyField";
import { taxBadge } from "./taxStatus";

export type { AccountDraft } from "./accountDraft";

const MUTED = "color-mix(in srgb, var(--color-text) 55%, transparent)";

/**
 * The account drawer — one skeleton, five kinds.
 *
 * Six blocks in a fixed order: identity, terms, valuation, positions, linked,
 * referenced by. A block is present or absent, never moved, so Referenced by is
 * the last thing above the footer whatever you selected, and the eye learns one
 * shape instead of five. Only Terms and Linked vary in content; everything else
 * is the same component with different data.
 *
 * Kind is a field in block 1, so a conversion happens in place — and when the
 * new kind abandons data the old one held, the drawer says so before it
 * commits.
 *
 * Mount with a `key` of the account id so switching rows starts a fresh draft
 * rather than carrying edits across.
 */
export function AccountInspector({
  account,
  profiles,
  assets,
  onApply,
  onAddLot,
  addLotForm,
  onSelectAccount,
  onDelete,
  busy,
  error,
  savedAt,
  offline,
}: {
  account: Account;
  /** Every profile the account's own could be pointed at. */
  profiles: Profile[];
  /** Every asset a property could be marked against. */
  assets: Asset[];
  onApply: (draft: AccountDraft) => void;
  /** Omitted where the kind cannot hold lots. */
  onAddLot?: () => void;
  /** The add-position form, rendered under the positions table when open. */
  addLotForm?: ReactNode;
  /** Opening the other end of a property/debt pair. */
  onSelectAccount: (id: AccountId) => void;
  onDelete?: () => void;
  busy?: boolean;
  error?: string;
  /** When this account was last saved from this session, for the resting line. */
  savedAt?: number;
  /** Writes are being refused, so nothing here can be saved. */
  offline?: boolean;
}) {
  const [draft, setDraft] = useState<AccountDraft>(() => draftOf(account));
  const set = <K extends keyof AccountDraft>(key: K, value: AccountDraft[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const pristine = draftOf(account);
  const changed = changedFields(draft, pristine);
  const pending = pendingCount(changed);
  const dirty = pending > 0;
  const canApply = dirty && !busy && !offline;
  const badge = taxBadge(account);
  const warning = changed.kind ? conversionWarning(account, draft.kind) : undefined;

  const revert = () => setDraft(draftOf(account));

  const holdsLots = draft.kind === "investment" || draft.kind === "retirement";

  return (
    <div
      style={{
        padding: "18px 18px 22px",
        display: "flex",
        flexDirection: "column",
        gap: 13,
        height: "100%",
      }}
      onKeyDown={(e) => {
        if (!dirty) return;
        if (e.key === "Escape") {
          e.preventDefault();
          revert();
        } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
          e.preventDefault();
          if (canApply) onApply(draft);
        }
      }}
    >
      {/* 1 · Identity */}
      <div>
        <div
          style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}
        >
          <h5 style={{ margin: 0 }}>{account.name}</h5>
          <Tag tone={badge.tone}>{badge.label}</Tag>
        </div>
        <DirtyField label="Kind" changed={changed.kind} style={{ marginTop: 8 }}>
          <Dropdown
            className="dd-field"
            options={KINDS.map((kind) => ({
              value: kind,
              label: KIND_LABEL[kind],
              // Locked rather than hidden: a menu that silently omits three of
              // five kinds does not explain why this one cannot become them.
              disabled: !isReachable(account, kind),
              detail: isReachable(account, kind) ? undefined : "separate flavor",
            }))}
            value={draft.kind}
            disabled={offline}
            onChange={(kind: AccountKind) => {
              setDraft((d) => ({
                ...d,
                kind,
                taxStatus: taxStatusFor(kind, pristine.taxStatus),
                // A brokerage has no contribution limit for the engine to read.
                contributionLimit: kind === "retirement" ? d.contributionLimit : null,
              }));
            }}
            ariaLabel="Kind"
          />
        </DirtyField>
      </div>

      {warning && <ConversionNote>{warning}</ConversionNote>}

      {/* 2 · Terms */}
      <AccountTerms
        account={account}
        draft={draft}
        pristine={pristine}
        changed={changed}
        set={set}
        profiles={profiles}
        assets={assets}
        offline={offline}
      />

      {/* 3 · Valuation */}
      <AccountValuation
        account={account}
        draft={draft}
        pristine={pristine}
        changed={changed}
        set={set}
        offline={offline}
      />

      {/* 4 · Positions — and, where the kind has none, a line saying so, rather
          than a drawer that just looks shorter for no reason. */}
      {holdsLots ? (
        <div>
          <PositionsTable
            lots={account.positions}
            onAddLot={addLotForm ? undefined : onAddLot}
            addDisabled={offline}
            basisTracked={draft.kind === "investment"}
            note={
              account.positions.length > 0 &&
              (draft.kind === "investment"
                ? "Cost basis tracked — a sale here realises a capital gain."
                : "No cost basis — gains are not taxed on sale here.")
            }
          />
          {addLotForm}
        </div>
      ) : (
        draft.kind === "cash" && (
          <div
            style={{
              fontSize: 11.5,
              padding: "9px 0",
              borderTop: "1px solid var(--color-divider)",
              borderBottom: "1px solid var(--color-divider)",
              color: "color-mix(in srgb, var(--color-text) 50%, transparent)",
            }}
          >
            No positions — cash is a single balance.
          </div>
        )
      )}

      {/* 5 · Linked */}
      {(account.flavor === "Property" || account.flavor === "Liability") && (
        <LinkedAccounts account={account} onSelect={onSelectAccount} />
      )}

      {/* 6 · Referenced by */}
      <ReferencedBy eventIds={account.referencedBy} />

      {error && (
        <p style={{ margin: 0, fontSize: 12, color: "var(--color-accent-900)" }}>{error}</p>
      )}

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          marginTop: "auto",
          fontSize: 11.5,
          color: MUTED,
        }}
      >
        {dirty ? (
          <>
            <span>{pending} unsaved</span>
            <Button style={{ marginLeft: "auto" }} shortcut="esc" onClick={revert}>
              Revert
            </Button>
            <Button
              variant="primary"
              shortcut="⌘⏎"
              disabled={!canApply}
              title={offline ? "No connection to the server." : undefined}
              onClick={() => onApply(draft)}
            >
              Apply
            </Button>
          </>
        ) : (
          <>
            {savedAt != null && (
              <>
                <svg
                  width="13"
                  height="13"
                  viewBox="0 0 16 16"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden
                >
                  <path d="M13.5 4.5 6.4 11.6 2.9 8.1" />
                </svg>
                <span>Saved {clockOf(savedAt)} · results marked stale</span>
              </>
            )}
            {onDelete && (
              <Button
                variant="ghost"
                style={{ marginLeft: "auto" }}
                onClick={onDelete}
                disabled={offline}
              >
                Delete
              </Button>
            )}
          </>
        )}
      </div>
    </div>
  );
}

/** What a conversion abandons, said before it commits. */
function ConversionNote({ children }: { children: ReactNode }) {
  return (
    <Blueprint
      corners={false}
      style={{
        padding: "10px 12px",
        fontSize: 11.5,
        lineHeight: 1.5,
        background: "color-mix(in srgb, var(--color-accent) 6%, transparent)",
      }}
    >
      <div style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
        <svg
          width="14"
          height="14"
          viewBox="0 0 16 16"
          fill="none"
          stroke="var(--color-accent-800)"
          strokeWidth="1.5"
          strokeLinecap="round"
          style={{ flex: "none", marginTop: 2 }}
          aria-hidden
        >
          <path d="M8 2.4 14.4 13.4H1.6z" />
          <line x1="8" y1="6.4" x2="8" y2="9.4" />
          <circle cx="8" cy="11.4" r="0.6" fill="var(--color-accent-800)" />
        </svg>
        <span>{children}</span>
      </div>
    </Blueprint>
  );
}

/** `14:06` — the clock the status bar and this line both quote. */
function clockOf(at: number): string {
  return new Date(at).toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}
