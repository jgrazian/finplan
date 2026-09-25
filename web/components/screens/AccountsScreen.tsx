"use client";

import { useMemo, useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  AccountInspector,
  AccountsTable,
  NewAccountDialog,
  PortfolioSummary,
  PositionForm,
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
  UpdatePosition,
} from "@/lib/api/types";
import { fmtClock, fmtCurrency } from "@/lib/format";
import type { RawWorkspace } from "@/lib/hooks/useWorkspace";
import { useReorderWrite } from "@/lib/hooks/useReorderWrite";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { useNav } from "@/lib/nav";
import { useServerStatus } from "@/lib/status/useServerStatus";
import { accountColors, accountShares, portfolioSummary } from "@/lib/view/accounts";
import type { Account, AccountId, AssetLot } from "@/lib/types";

/** What the one form under the positions table is open on. */
type LotEditor = { kind: "add" } | { kind: "edit"; positionId: number };

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
        repayment:
          draft.repayFrom == null
            ? null
            : { from_account_id: draft.repayFrom, term_months: draft.termMonths },
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
  const [creating, setCreating] = useState(false);
  /**
   * What the form under the positions table is doing, if anything: adding a lot
   * or editing the one whose row was clicked. One piece of state, because the
   * form is one form and only ever open on one subject.
   */
  const [lotEditor, setLotEditor] = useState<LotEditor>();
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
  // The selected account is in the query, by name, so a refresh or a shared
  // link opens the drawer on the same row.
  const nav = useNav();
  // Two trackers: an edit reports in the drawer's footer, a lot in its own form.
  const editing = useSubmit();
  const lot = useSubmit();
  const saveOrder = useReorderWrite(onChanged);

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
  const selected = accounts.find((a) => a.accountId === nav.selection) ?? accounts[0];
  const selectedRaw = raw.accounts.find((a) => a.id === selected?.serverId);

  const select = (id: AccountId) => {
    nav.setSelection(id);
    setLotEditor(undefined);
  };

  const remove = async (account: Account) => {
    if (!confirm(`Delete ${account.name}? Its positions go with it.`)) return;
    await api.accounts.remove(scenarioId, account.serverId);
    onChanged();
  };

  const apply = (account: Account, draft: AccountDraft) => {
    if (!selectedRaw) return;
    const name = draft.name.trim();
    // The server refuses a blank name, but saying so here keeps the drawer's
    // own field from being the one thing a round trip is spent on.
    if (name === "") return editing.fail("An account needs a name.");
    const body: UpdateAccountBody = { name, ...flavorOf(selectedRaw, draft) };
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
        else setLotEditor(undefined);
      },
    );

  const savePosition = (account: Account, positionId: number, body: UpdatePosition) =>
    lot.run(
      () => api.accounts.updatePosition(scenarioId, account.serverId, positionId, body),
      () => {
        onChanged();
        setLotEditor(undefined);
      },
    );

  const removePosition = (account: Account, lotRow: AssetLot) => {
    if (!confirm(`Remove the ${lotRow.assetId} lot from ${account.name}?`)) return;
    lot.run(
      () => api.accounts.removePosition(scenarioId, account.serverId, lotRow.positionId),
      () => {
        onChanged();
        setLotEditor(undefined);
      },
    );
  };

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
                  onReorder={
                    offline
                      ? undefined
                      : (ids) => saveOrder(() => api.accounts.reorder(scenarioId, ids))
                  }
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
                      confirmed{lastContact ? `, at ${fmtClock(lastContact)}` : ""}. Editing
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
              payers={raw.accounts.filter((a) => a.flavor === "Bank" || a.flavor === "Investment")}
              onApply={(draft) => apply(selected, draft)}
              onSelectAccount={select}
              onAddLot={
                selected.flavor === "Investment"
                  ? () => setLotEditor({ kind: "add" })
                  : undefined
              }
              onEditLot={
                selected.flavor === "Investment"
                  ? (row) => setLotEditor({ kind: "edit", positionId: row.positionId })
                  : undefined
              }
              editingLotId={lotEditor?.kind === "edit" ? lotEditor.positionId : undefined}
              onReorderLots={
                offline
                  ? undefined
                  : (ids) =>
                      saveOrder(() =>
                        api.accounts.reorderPositions(scenarioId, selected.serverId, ids),
                      )
              }
              lotForm={
                selected.flavor === "Investment"
                  ? lotForm(selected, lotEditor, {
                      // Keyed by what it is open on, so clicking a second lot
                      // starts a fresh draft on that lot rather than carrying
                      // the first one's figures across.
                      key: lotEditor?.kind === "edit" ? lotEditor.positionId : lotNonce,
                      scenarioId,
                      assets,
                      profiles: raw.returnProfiles,
                      busy: lot.busy,
                      error: lot.error,
                      onAssetCreated: assetCreated,
                      onCancel: () => setLotEditor(undefined),
                      onAdd: (body, again) => addPosition(selected, body, again),
                      onSave: (positionId, body) => savePosition(selected, positionId, body),
                      onDelete: (row) => removePosition(selected, row),
                    })
                  : undefined
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
          payers={raw.accounts.filter((a) => a.flavor === "Bank" || a.flavor === "Investment")}
          onClose={() => setCreating(false)}
          onCreated={onChanged}
          onAssetCreated={assetCreated}
        />
      )}
    </>
  );
}

/**
 * The add- or edit-position form, or nothing while neither is open.
 *
 * Written as a function rather than inline because the two cases differ in
 * what they submit — a create body, or a patch against one stored lot — and a
 * ternary carrying both is longer than the branch it saves. An edit whose lot
 * has since gone (deleted in another tab, say) renders as no form at all.
 */
function lotForm(
  account: Account,
  editor: LotEditor | undefined,
  props: {
    key: number;
    scenarioId: number;
    assets: Asset[];
    profiles: RawWorkspace["returnProfiles"];
    busy: boolean;
    error: string | undefined;
    onAssetCreated: (asset: Asset) => void;
    onCancel: () => void;
    onAdd: (body: CreatePosition, again: boolean) => void;
    onSave: (positionId: number, body: UpdatePosition) => void;
    onDelete: (lot: AssetLot) => void;
  },
) {
  if (!editor) return undefined;
  const { key, onAdd, onSave, onDelete, ...common } = props;

  if (editor.kind === "add") {
    return <PositionForm key={`add-${key}`} {...common} taxStatus={account.taxStatus} onSubmit={onAdd} />;
  }

  const lot = account.positions.find((p) => p.positionId === editor.positionId);
  if (!lot) return undefined;
  return (
    <PositionForm
      key={`edit-${key}`}
      {...common}
      taxStatus={account.taxStatus}
      lot={lot}
      onSubmit={(body) => onSave(lot.positionId, body)}
      onDelete={() => onDelete(lot)}
    />
  );
}
