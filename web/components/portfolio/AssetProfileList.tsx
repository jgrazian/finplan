"use client";

import { type KeyboardEvent, useState } from "react";
import { pct } from "@/components/profiles";
import { Tag, rowStyle } from "@/components/ui";
import { fmtCurrency, fmtUnits } from "@/lib/format";
import type { AssetRow, AssetsSelection, ProfileGroup } from "@/lib/view/assets";
import { flatten } from "@/lib/view/assets";

const MONO = { fontFamily: "ui-monospace, Menlo, monospace", fontSize: 12 } as const;
const MUTED = "color-mix(in srgb, var(--color-text) 62%, transparent)";
const ASSET_RULE = "1px solid color-mix(in srgb, var(--color-text) 7%, transparent)";

/**
 * The outline: return profiles as group rows, the assets they drive nested
 * under them. Dragging an asset onto another group remaps it — the one edit
 * the old two-tab split made awkward, since it meant holding a profile's name
 * in your head while switching tabs.
 */
export function AssetProfileList({
  groups,
  selection,
  onSelect,
  onRemap,
}: {
  groups: ProfileGroup[];
  selection: AssetsSelection | undefined;
  onSelect: (selection: AssetsSelection) => void;
  /** Omitted while a remap is already in flight. */
  onRemap?: (asset: AssetRow, profileServerId: number) => void;
}) {
  const [dropTarget, setDropTarget] = useState<number>();

  const isSelected = (row: { group: ProfileGroup; asset?: AssetRow }) =>
    row.asset
      ? selection?.kind === "asset" && selection.id === row.asset.serverId
      : selection?.kind === "profile" && selection.id === row.group.profile.id;

  return (
    <div
      role="listbox"
      aria-label="Assets by return profile"
      style={{ display: "flex", flexDirection: "column" }}
    >
      {flatten(groups).map((row) =>
        row.asset ? (
          <AssetLine
            key={`a${row.asset.serverId}`}
            asset={row.asset}
            selected={isSelected(row)}
            draggable={onRemap != null}
            onSelect={() => onSelect({ kind: "asset", id: row.asset!.serverId })}
          />
        ) : (
          <GroupLine
            key={`p${row.group.profile.id}`}
            group={row.group}
            selected={isSelected(row)}
            dropping={dropTarget === row.group.profile.serverId}
            onSelect={() => onSelect({ kind: "profile", id: row.group.profile.id })}
            onDropAsset={
              onRemap &&
              ((assetId) => {
                setDropTarget(undefined);
                const asset = groups
                  .flatMap((g) => g.assets)
                  .find((a) => a.serverId === assetId);
                if (asset && asset.profileServerId !== row.group.profile.serverId) {
                  onRemap(asset, row.group.profile.serverId);
                }
              })
            }
            onDropTarget={(over) =>
              setDropTarget(over ? row.group.profile.serverId : undefined)
            }
          />
        ),
      )}
    </div>
  );
}

function GroupLine({
  group,
  selected,
  dropping,
  onSelect,
  onDropAsset,
  onDropTarget,
}: {
  group: ProfileGroup;
  selected: boolean;
  dropping: boolean;
  onSelect: () => void;
  onDropAsset?: (assetId: number) => void;
  onDropTarget: (over: boolean) => void;
}) {
  const { profile, assets } = group;
  return (
    <div
      {...selectable(selected, onSelect)}
      style={{
        ...rowStyle(selected),
        ...(dropping
          ? { outline: "1px dashed var(--color-accent)", outlineOffset: -1 }
          : null),
      }}
      onDragOver={
        onDropAsset &&
        ((e) => {
          e.preventDefault();
          e.dataTransfer.dropEffect = "move";
          onDropTarget(true);
        })
      }
      onDragLeave={
        onDropAsset &&
        ((e) => {
          // dragleave also fires on the way into a child, which would flicker
          // the drop outline off under the cursor.
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) onDropTarget(false);
        })
      }
      onDrop={
        onDropAsset &&
        ((e) => {
          e.preventDefault();
          const id = Number(e.dataTransfer.getData("text/plain"));
          if (Number.isFinite(id)) onDropAsset(id);
        })
      }
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "9px 10px",
          borderTop: "1px solid var(--color-divider)",
        }}
      >
        <span style={MONO}>{profile.id}</span>
        <Tag tone="accent">{profile.kind}</Tag>
        <span style={{ fontSize: 12, color: MUTED }}>
          {assets.length === 0
            ? "no assets"
            : `${assets.length} asset${assets.length === 1 ? "" : "s"}`}
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 16, fontSize: 13 }}>
          <span>{pct(profile.mean)}</span>
          <span style={{ color: "color-mix(in srgb, var(--color-text) 55%, transparent)" }}>
            {profile.sd === 0 ? "—" : `± ${pct(profile.sd)}`}
          </span>
        </span>
      </div>
    </div>
  );
}

function AssetLine({
  asset,
  selected,
  draggable,
  onSelect,
}: {
  asset: AssetRow;
  selected: boolean;
  draggable: boolean;
  onSelect: () => void;
}) {
  return (
    <div
      {...selectable(selected, onSelect)}
      style={rowStyle(selected)}
      draggable={draggable}
      onDragStart={(e) => {
        e.dataTransfer.setData("text/plain", String(asset.serverId));
        e.dataTransfer.effectAllowed = "move";
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "7px 10px 7px 26px",
          borderTop: ASSET_RULE,
        }}
      >
        <span style={{ ...MONO, width: 56 }}>{asset.ticker}</span>
        <span style={{ fontSize: 13 }}>{asset.name}</span>
        <span
          style={{
            marginLeft: "auto",
            display: "flex",
            gap: 20,
            fontSize: 12.5,
            color: MUTED,
          }}
        >
          <span>{fmtCurrency(asset.price)}</span>
          <span>{asset.units === 0 ? "unheld" : `${fmtUnits(asset.units)} sh`}</span>
          <span>
            {asset.holdings.length === 0
              ? "—"
              : `${asset.holdings.length} acct${asset.holdings.length === 1 ? "" : "s"}`}
          </span>
        </span>
      </div>
    </div>
  );
}

/**
 * The row's selection affordances. These are divs rather than table rows —
 * the outline interleaves two shapes — so the listbox semantics and the keys
 * a row would otherwise get for free are spelled out here.
 */
function selectable(selected: boolean, onSelect: () => void) {
  return {
    className: "rowsel",
    role: "option" as const,
    "aria-selected": selected,
    tabIndex: 0,
    onClick: onSelect,
    onKeyDown: (e: KeyboardEvent<HTMLDivElement>) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        onSelect();
      }
    },
  };
}
