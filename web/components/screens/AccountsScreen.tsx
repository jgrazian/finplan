"use client";

import { useMemo, useState } from "react";
import { SplitPane } from "@/components/layout";
import { AccountInspector, AccountsTable } from "@/components/portfolio";
import { Button } from "@/components/ui";
import { MOCK_ACCOUNTS, accountShares } from "@/lib/mock/accounts";
import type { AccountId } from "@/lib/types";

/** Artboard 1d — editing model C, a persistent inspector drawer. */
export function AccountsScreen() {
  const [accounts] = useState(MOCK_ACCOUNTS);
  const [selectedId, setSelectedId] = useState<AccountId>(accounts[0].accountId);
  const [dirty, setDirty] = useState(false);

  const shares = useMemo(() => accountShares(accounts), [accounts]);
  const selected =
    accounts.find((a) => a.accountId === selectedId) ?? accounts[0];

  return (
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
            <h4 style={{ margin: 0 }}>Accounts</h4>
            <Button shortcut="a">Add account</Button>
          </div>

          <AccountsTable
            accounts={accounts}
            shares={shares}
            selectedId={selectedId}
            onSelect={(id) => {
              setSelectedId(id);
              setDirty(false);
            }}
          />

          <p
            style={{
              fontSize: 12,
              margin: "14px 0 0",
              color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
            }}
          >
            Selection drives the drawer. Nothing is ever covered up, and holdings sit in
            the same surface as the account.
          </p>
        </div>
      }
      rail={
        <AccountInspector
          account={selected}
          dirty={dirty}
          onFieldChange={() => setDirty(true)}
          onRevert={() => setDirty(false)}
          onApply={() => setDirty(false)}
        />
      }
    />
  );
}
