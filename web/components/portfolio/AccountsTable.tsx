"use client";

import { DragHandle, DropLine, Table, Tag, Td, Th, rowStyle } from "@/components/ui";
import { fmtCurrency, fmtShareFine } from "@/lib/format";
import { useReorder } from "@/lib/hooks/useReorder";
import type { Account, AccountId } from "@/lib/types";
import { KIND_LABEL, kindOf } from "./accountKind";
import { taxBadge } from "./taxStatus";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";
const TRACK = "color-mix(in srgb, var(--color-text) 8%, transparent)";

/**
 * The account list. Selection drives the inspector; nothing is covered up.
 *
 * Widths are fixed rather than content-sized: left to itself a table this wide
 * pushes every number to the far margin, away from the label it belongs to. The
 * width that buys is spent on two columns worth having — the profile driving the
 * account, and what it actually holds, which is the thing you would otherwise
 * open the drawer to check.
 *
 * The order is the user's own, dragged by the grip in the left gutter — the
 * same gutter the selected row's accent rule runs down.
 */
export function AccountsTable({
  accounts,
  shares,
  colors,
  selectedId,
  onSelect,
  onReorder,
}: {
  accounts: Account[];
  shares: Map<AccountId, number | null>;
  colors: Map<AccountId, string>;
  selectedId: AccountId;
  onSelect: (id: AccountId) => void;
  /** Server ids in their new order. Omitted where writes are refused. */
  onReorder?: (ids: number[]) => void | Promise<unknown>;
}) {
  const byServerId = new Map(accounts.map((a) => [a.serverId, a]));
  // Destructured rather than kept as one object: a `ref` prop taken off a
  // value marks the whole value as a ref to the React compiler, and the rest of
  // what the hook returns is ordinary render state.
  const { order, attachList, attachRow, dragging, indicator, handleProps, listStyle } =
    useReorder({ keys: accounts.map((a) => a.serverId), onReorder });

  return (
    <div ref={attachList} style={listStyle}>
      <DropLine at={indicator} />
      <Table fixed>
        <thead>
          <tr>
            <Th style={{ width: 22, padding: 0 }} aria-label="Order" />
            <Th style={{ width: 210 }}>Account</Th>
            <Th style={{ width: 104 }}>Tax</Th>
            <Th style={{ width: 150 }}>Return profile</Th>
            <Th>Holdings</Th>
            <Th align="right" style={{ width: 120 }}>
              Balance
            </Th>
            <Th style={{ width: 132 }}>Share</Th>
          </tr>
        </thead>
        <tbody>
          {order.map((serverId) => {
            const a = byServerId.get(serverId);
            if (!a) return null;
            const badge = taxBadge(a);
            const share = shares.get(a.accountId);
            const selected = a.accountId === selectedId;
            return (
              <tr
                key={a.accountId}
                ref={attachRow(serverId)}
                className={dragging === serverId ? "rowsel dragging" : "rowsel"}
                style={rowStyle(selected)}
                aria-selected={selected}
                onClick={() => onSelect(a.accountId)}
              >
                <Td style={{ padding: 0 }}>
                  <DragHandle label={a.name} props={handleProps(serverId)} />
                </Td>
                <Td style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
                  <strong style={{ fontWeight: 500 }}>{a.name}</strong>{" "}
                  <span className="text-muted" style={{ fontSize: 11 }}>
                    {KIND_LABEL[kindOf(a)]}
                  </span>
                </Td>
                <Td>
                  <Tag tone={badge.tone}>{badge.label}</Tag>
                </Td>
                <Td
                  style={{ fontSize: 12, overflow: "hidden", textOverflow: "ellipsis" }}
                  title={a.returnProfileId}
                >
                  {a.returnProfileId}
                </Td>
                <Td
                  style={{
                    fontSize: 12,
                    color: "color-mix(in srgb, var(--color-text) 62%, transparent)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                  title={a.holdings}
                >
                  {a.holdings}
                </Td>
                <Td align="right">{fmtCurrency(a.balance)}</Td>
                <Td>
                  {share == null ? (
                    <span style={{ fontSize: 11.5, color: MUTED }}>—</span>
                  ) : (
                    <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span style={{ flex: 1, height: 7, background: TRACK }} aria-hidden>
                        <i
                          style={{
                            display: "block",
                            height: "100%",
                            width: `${share * 100}%`,
                            background: colors.get(a.accountId),
                          }}
                        />
                      </span>
                      <span
                        style={{ fontSize: 11.5, width: 36, textAlign: "right", color: MUTED }}
                      >
                        {fmtShareFine(share)}
                      </span>
                    </span>
                  )}
                </Td>
              </tr>
            );
          })}
        </tbody>
      </Table>
    </div>
  );
}
