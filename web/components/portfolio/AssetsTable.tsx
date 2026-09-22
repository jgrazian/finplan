"use client";

import type { KeyboardEvent } from "react";
import { DragHandle, DropLine, rowStyle } from "@/components/ui";
import { fmtCurrency, fmtShareFine, fmtUnits } from "@/lib/format";
import { useReorder } from "@/lib/hooks/useReorder";
import type { AssetMix, AssetRow } from "@/lib/view/assets";

const MUTED = "color-mix(in srgb, var(--color-text) 62%, transparent)";
const FAINT = "color-mix(in srgb, var(--color-text) 40%, transparent)";
const TRACK = "color-mix(in srgb, var(--color-text) 8%, transparent)";
const MONO = { fontFamily: "ui-monospace, Menlo, monospace" } as const;

/**
 * Declared once so the header and its rows cannot drift apart. The name is the
 * only column that flexes: everything else is a figure whose width is its own.
 */
const COLUMNS = "14px 26px 64px minmax(110px, 1fr) 88px 96px 124px 150px 140px";
const GAP = "0 12px";

/**
 * The holdings, flat.
 *
 * The profile is a column rather than the level above the row, because that is
 * what it is: a thing the asset points at. The column names it and stops there
 * — what the profile is shaped like belongs to the library below, where each
 * one is drawn once instead of once per holding that happens to point at it.
 *
 * The row ends on its share of the portfolio, which is where a share belongs:
 * it is the last thing you want off a row and the first thing you want to scan
 * a column of, and both are easiest against the margin.
 *
 * The order is the user's, dragged by the grip in the left gutter: the point of
 * a holdings list is that the things you watch are near the top.
 */
export function AssetsTable({
  rows,
  mix,
  selectedId,
  checked,
  onSelect,
  onCheck,
  onReorder,
}: {
  rows: AssetRow[];
  /** The portfolio total each row is a share of, and the ramp above the table. */
  mix: AssetMix;
  /** Server id of the asset driving the drawer, or undefined. */
  selectedId: number | undefined;
  /** Server ids ticked for a bulk remap. */
  checked: ReadonlySet<number>;
  onSelect: (row: AssetRow) => void;
  /** Omitted while a remap is in flight, or when writes are refused. */
  onCheck?: (row: AssetRow, on: boolean) => void;
  /** Server ids in their new order. Omitted where writes are refused. */
  onReorder?: (ids: number[]) => void | Promise<unknown>;
}) {
  const byServerId = new Map(rows.map((r) => [r.serverId, r]));
  // Destructured rather than kept as one object: a `ref` prop taken off a
  // value marks the whole value as a ref to the React compiler, and the rest of
  // what the hook returns is ordinary render state.
  const { order, attachList, attachRow, dragging, indicator, handleProps, listStyle } =
    useReorder({ keys: rows.map((r) => r.serverId), onReorder });

  return (
    <div
      role="listbox"
      aria-label="Assets"
      ref={attachList}
      style={listStyle}
    >
      <DropLine at={indicator} />
      <div
        style={{
          display: "grid",
          gridTemplateColumns: COLUMNS,
          gap: GAP,
          alignItems: "end",
          padding: "0 10px 5px",
          borderBottom: "1px solid var(--color-text)",
        }}
      >
        <span />
        <span />
        <span className="stat-l">Ticker</span>
        <span className="stat-l">Name</span>
        <span className="stat-l" style={{ textAlign: "right" }}>
          Units
        </span>
        <span className="stat-l" style={{ textAlign: "right" }}>
          Value
        </span>
        <span className="stat-l">Held in</span>
        <span className="stat-l">Return profile</span>
        <span className="stat-l">Share of portfolio</span>
      </div>

      {order.map((serverId) => {
        const row = byServerId.get(serverId);
        if (!row) return null;
        const selected = row.serverId === selectedId;
        const ticked = checked.has(row.serverId);
        const share = mix.total > 0 ? row.value / mix.total : 0;
        return (
          <div
            key={row.serverId}
            ref={attachRow(serverId)}
            className={dragging === serverId ? "rowsel dragging" : "rowsel"}
            role="option"
            aria-selected={selected}
            tabIndex={0}
            style={rowStyle(selected)}
            onClick={() => onSelect(row)}
            onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect(row);
              }
            }}
          >
            <div
              style={{
                display: "grid",
                gridTemplateColumns: COLUMNS,
                gap: GAP,
                alignItems: "center",
                padding: "7px 10px",
                borderBottom: "1px solid var(--color-divider)",
              }}
            >
              <DragHandle label={row.ticker} props={handleProps(serverId)} />
              {onCheck ? (
                <label
                  className="check"
                  // The box picks rows for the bulk bar; the row around it
                  // drives the drawer. Ticking one must not do the other,
                  // by pointer or by the Space the row also answers to.
                  onClick={(e) => e.stopPropagation()}
                  onKeyDown={(e) => e.stopPropagation()}
                >
                  <input
                    type="checkbox"
                    checked={ticked}
                    onChange={(e) => onCheck(row, e.target.checked)}
                    aria-label={`Select ${row.ticker}`}
                  />
                  <span className="box" />
                </label>
              ) : (
                <span />
              )}
              <span style={{ ...MONO, fontSize: 12 }}>{row.ticker}</span>
              <span
                style={{
                  fontSize: 13,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={row.name}
              >
                {row.name || "—"}
              </span>
              <span style={{ ...MONO, fontSize: 12, textAlign: "right" }}>
                {row.units === 0 ? <span style={{ color: FAINT }}>unheld</span> : fmtUnits(row.units)}
              </span>
              <span style={{ ...MONO, fontSize: 12.5, textAlign: "right" }}>
                {row.units === 0 ? (
                  <span style={{ color: FAINT }}>{fmtCurrency(row.price)}</span>
                ) : (
                  fmtCurrency(row.value)
                )}
              </span>
              <span
                style={{
                  fontSize: 11.5,
                  color: MUTED,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={row.holdings.map((h) => h.account).join(" · ")}
              >
                {row.heldIn}
              </span>
              <span
                style={{
                  fontSize: 12.5,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  ...(row.profileId ? null : { color: FAINT }),
                }}
                title={row.profileId ?? "Unmapped"}
              >
                {row.profileId ?? "Unmapped"}
              </span>
              {/* Drawn against the whole portfolio rather than against the
                  largest holding: a bar that fills its track is a bar that
                  says 100%, and the figure beside it would be calling that a
                  lie. The fill is the profile's own colour, so the row is
                  findable in the breakdown above it. */}
              <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                {row.value <= 0 ? (
                  <span style={{ fontSize: 11.5, color: FAINT, marginLeft: "auto" }}>—</span>
                ) : (
                  <>
                    <span style={{ flex: 1, height: 7, background: TRACK }} aria-hidden>
                      <i
                        style={{
                          display: "block",
                          height: "100%",
                          width: `${share * 100}%`,
                          background: mix.colors.get(row.profileServerId) ?? FAINT,
                        }}
                      />
                    </span>
                    <span
                      style={{ ...MONO, fontSize: 11.5, width: 36, textAlign: "right", color: MUTED }}
                    >
                      {fmtShareFine(share)}
                    </span>
                  </>
                )}
              </span>
            </div>
          </div>
        );
      })}
    </div>
  );
}
