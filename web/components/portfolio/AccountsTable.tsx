"use client";

import { Table, Tag, Td, Th, rowStyle } from "@/components/ui";
import { fmtCurrency, fmtShare } from "@/lib/format";
import type { Account, AccountId } from "@/lib/types";
import { taxBadge } from "./taxStatus";

/** The account list. Selection drives the inspector; nothing is covered up. */
export function AccountsTable({
  accounts,
  shares,
  selectedId,
  onSelect,
}: {
  accounts: Account[];
  shares: Map<AccountId, number | null>;
  selectedId: AccountId;
  onSelect: (id: AccountId) => void;
}) {
  return (
    <Table>
      <thead>
        <tr>
          <Th>Account</Th>
          <Th>Tax status</Th>
          <Th align="right">Balance</Th>
          <Th align="right">Share</Th>
        </tr>
      </thead>
      <tbody>
        {accounts.map((a) => {
          const badge = taxBadge(a);
          const share = shares.get(a.accountId);
          const selected = a.accountId === selectedId;
          return (
            <tr
              key={a.accountId}
              className="rowsel"
              style={rowStyle(selected)}
              aria-selected={selected}
              onClick={() => onSelect(a.accountId)}
            >
              <Td>
                <strong style={{ fontWeight: 500 }}>{a.name}</strong>{" "}
                <span className="text-muted" style={{ fontSize: 11 }}>
                  {a.flavor}
                </span>
              </Td>
              <Td>
                <Tag tone={badge.tone}>{badge.label}</Tag>
              </Td>
              <Td align="right">{fmtCurrency(a.balance)}</Td>
              <Td align="right" muted>
                {share == null ? "—" : fmtShare(share)}
              </Td>
            </tr>
          );
        })}
      </tbody>
    </Table>
  );
}
