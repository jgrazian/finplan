"use client";

import { useMemo, useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  AccountInspector,
  AccountsTable,
  AddLotDialog,
  NewAccountDialog,
} from "@/components/portfolio";
import { Button } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { RawWorkspace } from "@/lib/hooks/useWorkspace";
import { useServerStatus } from "@/lib/status/useServerStatus";
import { accountShares } from "@/lib/view/accounts";
import type { Account, AccountId } from "@/lib/types";

/** `14:02` — the clock the status bar and this note both quote. */
function clockOf(at: number): string {
  return new Date(at).toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
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
  /** Writes are being refused, so add and delete cannot be offered. */
  offline?: boolean;
}) {
  const [picked, setPicked] = useState<AccountId>();
  const [dialog, setDialog] = useState<"account" | "lot">();
  const { lastContact } = useServerStatus();

  const shares = useMemo(() => accountShares(accounts), [accounts]);
  // Derived rather than reset in an effect: switching scenarios replaces every
  // id, and the first row is the right fallback whenever the pick is stale.
  const selected = accounts.find((a) => a.accountId === picked) ?? accounts[0];

  const remove = async (account: Account) => {
    if (!confirm(`Delete ${account.name}? Its positions go with it.`)) return;
    await api.accounts.remove(scenarioId, account.serverId);
    onChanged();
  };

  return (
    <>
      <SplitPane
        railWidth={344}
        main={
          <div style={{ padding: "18px 20px" }}>
            <div
              style={{
                display: "flex",
                alignItems: "baseline",
                justifyContent: "space-between",
                marginBottom: 10,
              }}
            >
              <h4 style={{ margin: 0 }}>
                Accounts{" "}
                <span className="text-muted" style={{ fontSize: 13 }}>
                  {accounts.length}
                </span>
              </h4>
              <Button
                shortcut="a"
                onClick={() => setDialog("account")}
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
                  selectedId={selected?.accountId ?? ""}
                  onSelect={setPicked}
                />
                <p
                  style={{
                    fontSize: 12,
                    margin: "14px 0 0",
                    color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
                  }}
                >
                  {offline ? (
                    <>
                      Reading is untouched — this is the last state the server
                      confirmed{lastContact ? `, at ${clockOf(lastContact)}` : ""}. Add
                      and delete are disabled because they cannot be held locally.
                    </>
                  ) : (
                    <>
                      Balances mark investment holdings at each asset&rsquo;s opening
                      price — the same figure the simulation starts from.
                    </>
                  )}
                </p>
              </>
            )}
          </div>
        }
        rail={
          selected ? (
            <AccountInspector
              account={selected}
              onAddLot={
                selected.flavor === "Investment" ? () => setDialog("lot") : undefined
              }
              onDelete={() => remove(selected)}
              offline={offline}
            />
          ) : null
        }
      />

      {dialog === "account" && (
        <NewAccountDialog
          scenarioId={scenarioId}
          profiles={raw.returnProfiles}
          assets={raw.assets}
          onClose={() => setDialog(undefined)}
          onCreated={onChanged}
        />
      )}
      {dialog === "lot" && selected && (
        <AddLotDialog
          scenarioId={scenarioId}
          accountId={selected.serverId}
          assets={raw.assets}
          onClose={() => setDialog(undefined)}
          onCreated={onChanged}
        />
      )}
    </>
  );
}
