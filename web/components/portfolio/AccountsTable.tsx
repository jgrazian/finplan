"use client";

import { DragHandle, DropLine, Table, Tag, Td, Th, rowStyle } from "@/components/ui";
import { fmtCurrency, fmtShareFine } from "@/lib/format";
import { useReorder } from "@/lib/hooks/useReorder";
import type { Account, AccountId } from "@/lib/types";
import { kindLabel } from "./accountKind";
import { taxBadge } from "./taxStatus";

const MUTED = "color-mix(in srgb, var(--color-text) 60%, transparent)";

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
      <Table fixed className="ledger-table">
        <thead>
          <tr>
            <Th style={{ width: 22, padding: 0 }} aria-label="Order" />
            <Th style={{ width: "25%" }}>Account</Th>
            <Th style={{ width: 98 }}>Tax</Th>
            <Th style={{ width: "19%" }}>Return profile</Th>
            <Th>Holdings %</Th>
            <Th align="right" style={{ width: 112 }}>
              Balance
            </Th>
            <Th style={{ width: 120 }}>Share</Th>
          </tr>
        </thead>
        <tbody>
          {order.map((serverId) => {
            const a = byServerId.get(serverId);
            if (!a) return null;
            const badge = taxBadge(a);
            const share = shares.get(a.accountId);
            const selected = a.accountId === selectedId;
            const color = colors.get(a.accountId);
            const [profile, ...moreProfiles] = a.returnProfiles;
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
                <Td title={`${a.name} · ${kindLabel(a)}`}>
                  <i className="ledger-swatch" style={{ background: color }} aria-hidden />
                  <strong style={{ fontWeight: 500 }}>{a.name}</strong>{" "}
                  <span className="text-muted" style={{ fontSize: 11.5 }}>
                    {kindLabel(a)}
                  </span>
                </Td>
                <Td>
                  <Tag tone={badge.tone}>{badge.label}</Tag>
                </Td>
                <Td style={{ fontSize: 12.5 }} title={a.returnProfiles.join(", ")}>
                  {profile}
                  {moreProfiles.length > 0 && (
                    <span className="text-muted"> +{moreProfiles.length}</span>
                  )}
                </Td>
                <Td title={a.holdings.detail}>
                  {/* The tickers give way before the lot count does. */}
                  <span className="ledger-holdings">
                    <span className="ledger-mono">{a.holdings.held}</span>
                    {a.holdings.lots && <span>{a.holdings.lots}</span>}
                  </span>
                </Td>
                <Td align="right" style={{ fontWeight: 500 }}>
                  {fmtCurrency(a.balance)}
                </Td>
                <Td>
                  {share == null ? (
                    <span style={{ fontSize: 11.5, color: MUTED }}>—</span>
                  ) : (
                    <span className="ledger-share">
                      <span aria-hidden>
                        <i
                          style={{
                            // A sliver stays visible: a 0.3% account still
                            // has a share, and an empty track says it has none.
                            width: `${Math.max(share * 100, 1.5)}%`,
                            background: color,
                          }}
                        />
                      </span>
                      <span>{fmtShareFine(share)}</span>
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
