"use client";

import { useMemo, useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  AccountInspector,
  AccountsTable,
  AddPositionForm,
  NewAccountDialog,
  PortfolioSummary,
  type AccountDraft,
} from "@/components/portfolio";
import { Button } from "@/components/ui";
import { api } from "@/lib/api/client";
import type {
  Account as ApiAccount,
  Asset,
  CreatePosition,
  FlavorSpec,
  UpdateAccountBody,
} from "@/lib/api/types";
import { fmtCurrency } from "@/lib/format";
import type { RawWorkspace } from "@/lib/hooks/useWorkspace";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { useServerStatus } from "@/lib/status/useServerStatus";
import { accountColors, accountShares, portfolioSummary } from "@/lib/view/accounts";
import type { Account, AccountId } from "@/lib/types";

/** `14:02` — the clock the status bar and this note both quote. */
function clockOf(at: number): string {
  return new Date(at).toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

/**
 * The flavor half of the PATCH.
 *
 * The route replaces the detail row wholesale rather than merging into it, so
 * every field of the variant has to be sent — the ones the drawer does not edit
 * come back off the row the server last returned.
 */
function flavorOf(raw: ApiAccount, draft: AccountDraft): FlavorSpec {
  switch (raw.flavor) {
    case "Bank":
      return {
        flavor: "Bank",
        cash_value: draft.amount,
        return_profile_id: draft.returnProfileServerId ?? raw.return_profile_id,
      };
    case "Investment":
      return {
        flavor: "Investment",
        // The kind carries this: Investment is taxable, Retirement deferred or
        // free, and the drawer's Tax treatment field refines the second.
        tax_status: draft.taxStatus ?? raw.tax_status,
        cash_value: draft.amount,
        cash_return_profile_id:
          draft.returnProfileServerId ?? raw.cash_return_profile_id,
        contribution_limit: draft.contributionLimit,
        // The server refuses one without the other.
        contribution_period:
          draft.contributionLimit == null ? null : draft.contributionPeriod,
      };
    case "Property":
      return {
        flavor: "Property",
        asset_id: draft.assetServerId ?? raw.asset_id,
        value: draft.amount,
      };
    case "Liability":
      // Stored as a positive amount owed; only the list signs it.
      return {
        flavor: "Liability",
        principal: draft.amount,
        interest_rate: draft.interestRate,
      };
  }
}

/** Portfolio › Accounts — the list, with a persistent inspector drawer. */
export function AccountsScreen({
  scenarioId,
  accounts,
  raw,
  onChanged,
  offline,
}: {
  scenarioId: number;
  accounts: Account[];
  raw: RawWorkspace;
  onChanged: () => void;
  /** Writes are being refused, so add, edit and delete cannot be offered. */
  offline?: boolean;
}) {
  const [picked, setPicked] = useState<AccountId>();
  const [creating, setCreating] = useState(false);
  const [addingLot, setAddingLot] = useState(false);
  /** Bumped after each save, to hand the form a clean slate for the next lot. */
  const [lotNonce, setLotNonce] = useState(0);
  const [savedAt, setSavedAt] = useState<Map<AccountId, number>>(new Map());
  /**
   * Tickers made from this screen, held until the workspace reload catches up.
   * Without them the form would select an asset that is not yet in the list it
   * was handed, and show its placeholder for a beat.
   */
  const [pending, setPending] = useState<Asset[]>([]);
  const { lastContact } = useServerStatus();
  // Two trackers: an edit reports in the drawer's footer, a lot in its own form.
  const editing = useSubmit();
  const lot = useSubmit();

  const assets = useMemo(() => {
    const known = new Set(raw.assets.map((a) => a.id));
    const fresh = pending.filter((a) => !known.has(a.id));
    return fresh.length > 0 ? [...raw.assets, ...fresh] : raw.assets;
  }, [raw.assets, pending]);

  const assetCreated = (asset: Asset) => {
    setPending((list) => [...list, asset]);
    onChanged();
  };

  const shares = useMemo(() => accountShares(accounts), [accounts]);
  const colors = useMemo(() => accountColors(accounts), [accounts]);
  const summary = useMemo(() => portfolioSummary(accounts), [accounts]);

  // Derived rather than reset in an effect: switching scenarios replaces every
  // id, and the first row is the right fallback whenever the pick is stale.
  const selected = accounts.find((a) => a.accountId === picked) ?? accounts[0];
  const selectedRaw = raw.accounts.find((a) => a.id === selected?.serverId);

  const select = (id: AccountId) => {
    setPicked(id);
    setAddingLot(false);
  };

  const remove = async (account: Account) => {
    if (!confirm(`Delete ${account.name}? Its positions go with it.`)) return;
    await api.accounts.remove(scenarioId, account.serverId);
    onChanged();
  };

  const apply = (account: Account, draft: AccountDraft) => {
    if (!selectedRaw) return;
    const body: UpdateAccountBody = flavorOf(selectedRaw, draft);
    editing.run(
      () => api.accounts.update(scenarioId, account.serverId, body),
      () => {
        setSavedAt((m) => new Map(m).set(account.accountId, Date.now()));
        onChanged();
      },
    );
  };

  const addPosition = (account: Account, body: CreatePosition, again: boolean) =>
    lot.run(
      () => api.accounts.addPosition(scenarioId, account.serverId, body),
      () => {
        onChanged();
        if (again) setLotNonce((n) => n + 1);
        else setAddingLot(false);
      },
    );

  return (
    <>
      <SplitPane
        railWidth={372}
        main={
          <div style={{ padding: "18px 22px 22px" }}>
            {accounts.length > 0 && <PortfolioSummary summary={summary} />}

            <div
              style={{
                display: "flex",
                alignItems: "baseline",
                justifyContent: "space-between",
                marginBottom: 12,
              }}
            >
              <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
                <h4 style={{ margin: 0 }}>Accounts</h4>
                <span
                  style={{
                    fontSize: 12,
                    color: "color-mix(in srgb, var(--color-text) 50%, transparent)",
                  }}
                >
                  {accounts.length}
                  {accounts.length > 0 && ` · ${fmtCurrency(summary.netWorth)} net`}
                </span>
              </div>
              <Button
                shortcut="a"
                onClick={() => setCreating(true)}
                disabled={offline}
                title={offline ? "No connection to the server." : undefined}
              >
                Add account
              </Button>
            </div>

            {accounts.length === 0 ? (
              <p style={{ fontSize: 13, maxWidth: 460, lineHeight: 1.5 }} className="text-muted">
                No accounts yet. Accounts are the balances the engine grows, spends
                from and taxes — a plan needs at least one to simulate.
              </p>
            ) : (
              <>
                <AccountsTable
                  accounts={accounts}
                  shares={shares}
                  colors={colors}
                  selectedId={selected?.accountId ?? ""}
                  onSelect={select}
                />
                <div
                  style={{
                    display: "flex",
                    justifyContent: "space-between",
                    gap: 20,
                    fontSize: 12,
                    padding: "10px 8px 0",
                    color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
                  }}
                >
                  {offline ? (
                    <span>
                      Reading is untouched — this is the last state the server
                      confirmed{lastContact ? `, at ${clockOf(lastContact)}` : ""}. Editing
                      is disabled because it cannot be held locally.
                    </span>
                  ) : (
                    <span>Balances mark holdings at each asset&rsquo;s opening price.</span>
                  )}
                  <span style={{ flex: "none" }}>
                    Total{" "}
                    <strong style={{ fontWeight: 500, color: "var(--color-text)" }}>
                      {fmtCurrency(summary.assets)}
                    </strong>{" "}
                    assets · {fmtCurrency(-summary.debt)} debt
                  </span>
                </div>
              </>
            )}
          </div>
        }
        rail={
          selected ? (
            <AccountInspector
              key={selected.accountId}
              account={selected}
              profiles={raw.returnProfiles}
              assets={assets}
              onApply={(draft) => apply(selected, draft)}
              onSelectAccount={select}
              onAddLot={
                selected.flavor === "Investment" ? () => setAddingLot(true) : undefined
              }
              addLotForm={
                addingLot && selected.flavor === "Investment" ? (
                  <AddPositionForm
                    key={lotNonce}
                    scenarioId={scenarioId}
                    assets={assets}
                    profiles={raw.returnProfiles}
                    taxStatus={selected.taxStatus}
                    busy={lot.busy}
                    error={lot.error}
                    onAssetCreated={assetCreated}
                    onCancel={() => setAddingLot(false)}
                    onSubmit={(body, again) => addPosition(selected, body, again)}
                  />
                ) : undefined
              }
              onDelete={() => remove(selected)}
              busy={editing.busy}
              error={editing.error}
              savedAt={savedAt.get(selected.accountId)}
              offline={offline}
            />
          ) : null
        }
      />

      {creating && (
        <NewAccountDialog
          scenarioId={scenarioId}
          profiles={raw.returnProfiles}
          assets={assets}
          onClose={() => setCreating(false)}
          onCreated={onChanged}
          onAssetCreated={assetCreated}
        />
      )}
    </>
  );
}
