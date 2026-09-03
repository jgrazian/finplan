"use client";

import type { KeyboardEvent } from "react";
import { RETURN_SCALE, ShapeAxis, ShapeSpark, pct } from "@/components/profiles";
import { rowStyle } from "@/components/ui";
import type { DistributionSpec } from "@/lib/api/types";
import { fmtCurrency, fmtUnits } from "@/lib/format";
import type { ReturnProfile } from "@/lib/types";
import type { AssetRow } from "@/lib/view/assets";

const MUTED = "color-mix(in srgb, var(--color-text) 62%, transparent)";
const FAINT = "color-mix(in srgb, var(--color-text) 40%, transparent)";
const MONO = { fontFamily: "ui-monospace, Menlo, monospace" } as const;

/** The shape column, and the axis above it, are the same box. */
const SHAPE_WIDTH = 140;

/**
 * Declared once so the header and its rows cannot drift apart. The name is the
 * only column that flexes: everything else is a figure whose width is its own.
 */
const COLUMNS = "26px 64px minmax(110px, 1fr) 88px 96px 124px 140px 140px 54px";

/**
 * An unmapped asset is not a hole in the table — the engine really does hold
 * it flat, which is exactly what a None profile draws.
 */
const HELD_FLAT: DistributionSpec = { kind: "None" };

/**
 * The holdings, flat.
 *
 * The profile is a column rather than the level above the row, because that is
 * what it is: a thing the asset points at. What the column carries is the
 * profile's shape, drawn on the scale in the header — so eleven rows can be
 * read against each other without opening any of them.
 */
export function AssetsTable({
  rows,
  profiles,
  selectedId,
  checked,
  onSelect,
  onCheck,
}: {
  rows: AssetRow[];
  profiles: ReturnProfile[];
  /** Server id of the asset driving the drawer, or undefined. */
  selectedId: number | undefined;
  /** Server ids ticked for a bulk remap. */
  checked: ReadonlySet<number>;
  onSelect: (row: AssetRow) => void;
  /** Omitted while a remap is in flight, or when writes are refused. */
  onCheck?: (row: AssetRow, on: boolean) => void;
}) {
  const byId = new Map(profiles.map((p) => [p.serverId, p]));

  return (
    <div role="listbox" aria-label="Assets">
      <div
        style={{
          display: "grid",
          gridTemplateColumns: COLUMNS,
          gap: "0 12px",
          alignItems: "end",
          padding: "0 10px 5px",
          borderBottom: "1px solid var(--color-text)",
        }}
      >
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
        <ShapeAxis
          label="Shape"
          width={SHAPE_WIDTH}
          scale={RETURN_SCALE}
          ticks={[-20, 0, 20]}
          fontSize={7.5}
        />
        <span className="stat-l" style={{ textAlign: "right" }}>
          Mean
        </span>
      </div>

      {rows.map((row) => {
        const profile = row.profileServerId == null ? undefined : byId.get(row.profileServerId);
        const spec = profile?.distribution ?? HELD_FLAT;
        const selected = row.serverId === selectedId;
        const ticked = checked.has(row.serverId);
        return (
          <div
            key={row.serverId}
            className="rowsel"
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
                gap: "0 12px",
                alignItems: "center",
                padding: "7px 10px",
                borderBottom: "1px solid var(--color-divider)",
              }}
            >
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
                  ...(profile ? null : { color: FAINT }),
                }}
                title={profile?.id ?? "Unmapped"}
              >
                {profile?.id ?? "Unmapped"}
              </span>
              <ShapeSpark
                spec={spec}
                width={SHAPE_WIDTH}
                height={20}
                scale={RETURN_SCALE}
                history={profile?.history}
              />
              <span
                style={{
                  textAlign: "right",
                  fontFamily: "var(--font-heading)",
                  fontWeight: 600,
                  fontSize: 14,
                  ...(profile ? null : { color: FAINT }),
                }}
              >
                {pct(profile ? profile.mean : 0)}
              </span>
            </div>
          </div>
        );
      })}
    </div>
  );
}
