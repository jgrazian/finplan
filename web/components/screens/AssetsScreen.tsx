"use client";

import { type ReactNode, useMemo, useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  AssetInspector,
  AssetsTable,
  NewAssetDialog,
  type AssetDraft,
} from "@/components/portfolio";
import {
  InflationProfilesTable,
  NewProfileDialog,
  ProfileInspector,
  ProfileLibraryTable,
} from "@/components/profiles";
import { Button, Select } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Profile, UpdateAsset, UpdateProfile } from "@/lib/api/types";
import { fmtCurrency } from "@/lib/format";
import { useAsync } from "@/lib/hooks/useAsync";
import { useSubmit } from "@/lib/hooks/useSubmit";
import type { RawWorkspace } from "@/lib/hooks/useWorkspace";
import { useNav } from "@/lib/nav";
import type { InflationProfile, ReturnProfile } from "@/lib/types";
import {
  type AssetRow,
  type AssetsSelection,
  assetsTotal,
  decodeAssetsSelection,
  encodeAssetsSelection,
  fillSummary,
  tickerFills,
  toAssetRows,
} from "@/lib/view/assets";
import { withHistories } from "@/lib/view/profiles";
import { EmptyState } from "./EmptyState";

/** The `<option>` value standing in for "no profile"; `null` is not a value. */
const UNMAPPED = -1;

const NOTE = {
  fontSize: 12,
  margin: "10px 0 0",
  color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
} as const;

/**
 * Portfolio › Assets — holdings first, the profile library below them.
 *
 * A return profile is what a holding points at, so it is a column on the asset
 * row and not the level above it: the list you scan is the list of things you
 * own. The library gets its own section further down, where a profile can be
 * edited without first finding an asset that happens to use it — and where the
 * profiles nothing points at are visible rather than hidden as empty groups.
 *
 * One drawer, two subjects. Clicking a holding inspects the holding and shows
 * the profile it inherits, read-only; clicking a library row edits the profile
 * itself.
 */
export function AssetsScreen({
  scenarioId,
  raw,
  returnProfiles: storedProfiles,
  inflationProfiles,
  activeInflationProfile,
  onActivateInflation,
  onChanged,
  offline,
}: {
  scenarioId: number;
  raw: RawWorkspace;
  returnProfiles: ReturnProfile[];
  inflationProfiles: InflationProfile[];
  activeInflationProfile: string | undefined;
  onActivateInflation?: (profile: InflationProfile) => void;
  onChanged: () => void;
  /** Writes are being refused: nothing here can be added, remapped or edited. */
  offline?: boolean;
}) {
  const [addingAsset, setAddingAsset] = useState(false);
  const [addingProfile, setAddingProfile] = useState(false);
  /** Server ids ticked for a bulk remap; cleared once one lands. */
  const [checked, setChecked] = useState<ReadonlySet<number>>(new Set());
  // One query token carries both subjects, since only one is ever selected:
  // `sel=profile:US equities`, or `sel=asset:12`.
  const nav = useNav();
  // Three trackers, because the three writes report in three places: a remap
  // in the bulk bar, an asset edit and a profile edit in their own drawers.
  const remapping = useSubmit();
  const editingAsset = useSubmit();
  const editingProfile = useSubmit();
  const filling = useSubmit();

  // The histories a Bootstrap profile resamples, series and all. A static
  // table the server owns, fetched once on mount rather than threaded through
  // the workspace — which reloads after every edit, and this never changes.
  const presets = useAsync(() => api.historyPresets(), []);
  const histories = useMemo(() => presets.data ?? [], [presets.data]);

  // A resampled profile arrives with no mean, no spread and no shape, because
  // the wire format carries a preset name. Attaching the years fills all three.
  const returnProfiles = useMemo(
    () => withHistories(storedProfiles, histories),
    [storedProfiles, histories],
  );

  const rows = useMemo(
    () => toAssetRows(raw.assets, raw.accounts, returnProfiles),
    [raw.assets, raw.accounts, returnProfiles],
  );
  const total = useMemo(() => assetsTotal(rows), [rows]);
  // Assets whose ticker knows something the row does not: a blank name, an
  // unmapped profile, or both. Recomputed off the reloaded rows, so the offer
  // disappears by itself once it has been taken.
  const fills = useMemo(
    () => tickerFills(rows, raw.returnProfiles),
    [rows, raw.returnProfiles],
  );

  // Derived rather than reset in an effect: switching scenarios replaces every
  // asset id, and the first row is the right fallback for a stale pick.
  const selection = resolve(decodeAssetsSelection(nav.selection), rows, returnProfiles);
  const selectedAsset =
    selection?.kind === "asset"
      ? rows.find((r) => r.serverId === selection.id)
      : undefined;
  const selectedProfile =
    selection?.kind === "profile"
      ? returnProfiles.find((p) => p.id === selection.id)
      : undefined;
  /** The profile the selected asset inherits — read-only in the asset drawer. */
  const inherited = returnProfiles.find(
    (p) => p.serverId === selectedAsset?.profileServerId,
  );

  const select = (next: AssetsSelection) => nav.setSelection(encodeAssetsSelection(next));

  if (raw.assets.length === 0 && returnProfiles.length === 0) {
    return (
      <EmptyState
        title="No assets and no return profiles"
        detail="Registration seeds a starter library; if it is empty, every asset in every scenario has nothing to grow by."
      />
    );
  }

  const bulkRemap = (profileServerId: number | null) => {
    const targets = rows.filter(
      (r) => checked.has(r.serverId) && r.profileServerId !== profileServerId,
    );
    if (targets.length === 0) return setChecked(new Set());
    remapping.run(
      () =>
        Promise.all(
          targets.map((r) =>
            api.assets.update(scenarioId, r.serverId, { return_profile_id: profileServerId }),
          ),
        ),
      () => {
        setChecked(new Set());
        onChanged();
      },
    );
  };

  /**
   * Fill every blank the tickers can fill, in one write each.
   *
   * Each PATCH carries only the halves that were missing: an absent key means
   * "unchanged" to the route, so an asset that was merely unmapped keeps the
   * name it had, and one that was merely unnamed keeps the profile it was on.
   */
  const fillFromTickers = () =>
    filling.run(
      () =>
        Promise.all(
          fills.map((fill) => {
            const body: UpdateAsset = {};
            if (fill.name != null) body.description = fill.name;
            if (fill.profileServerId != null) body.return_profile_id = fill.profileServerId;
            return api.assets.update(scenarioId, fill.serverId, body);
          }),
        ),
      onChanged,
    );

  const applyAsset = (asset: AssetRow, draft: AssetDraft) => {
    const ticker = draft.ticker.trim();
    const price = draft.price;
    // `PATCH` coalesces nulls onto the stored row, so it cannot police these
    // the way `POST` does: a blank ticker or a zero price would be written.
    if (ticker === "") return editingAsset.fail("An asset needs a ticker.");
    if (!Number.isFinite(price) || price <= 0) {
      return editingAsset.fail("Opening price must be positive.");
    }
    editingAsset.run(
      () =>
        api.assets.update(scenarioId, asset.serverId, {
          name: ticker,
          // Empty string, not null: null would coalesce back onto the old
          // description, so clearing the name would appear to work and revert.
          description: draft.name.trim(),
          initial_price: price,
          return_profile_id: draft.profileServerId,
        }),
      onChanged,
    );
  };

  /** Opens a freshly created profile, so it is the row the drawer is on. */
  const adopt = (created: Profile) => select({ kind: "profile", id: created.name });

  const applyProfile = (profile: ReturnProfile, body: UpdateProfile) =>
    editingProfile.run(() => api.returnProfiles.update(profile.serverId, body), onChanged);

  /**
   * A copy, opened in the drawer. The library is shared across scenarios, so
   * this is how a starter profile gets changed for one plan without changing
   * it for every other one that points at it.
   */
  const duplicateProfile = (profile: ReturnProfile) =>
    editingProfile.run(
      () =>
        api.returnProfiles
          .create({
            name: copyName(profile.id, returnProfiles),
            description: profile.description || null,
            // A copy is a copy, class included. Two profiles of one class is a
            // legitimate library — an optimistic and a pessimistic equity
            // assumption — and the resolver's tie-break is by name, so a
            // "… copy" never displaces the original it was made from.
            asset_class: profile.assetClass,
            distribution: profile.distribution,
          })
          .then(adopt),
      onChanged,
    );

  return (
    <>
      <SplitPane
        railWidth={360}
        main={
          <div style={{ padding: "14px 20px 18px" }}>
            <SectionBar
              title="Assets"
              count={`${rows.length}${rows.length > 0 ? ` · ${fmtCurrency(total)}` : ""}`}
              action={
                <Button
                  shortcut="a"
                  onClick={() => setAddingAsset(true)}
                  disabled={offline}
                  title={offline ? "No connection to the server." : undefined}
                >
                  Add asset
                </Button>
              }
            />

            {/* The offer stands down while rows are ticked: that is a selection
                waiting on an action, and two bars stacked in one slot read as
                two pending things rather than one. */}
            {fills.length > 0 && checked.size === 0 && !offline && (
              <div className="sbar sbar-notice" style={{ marginBottom: -1 }}>
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 16 16"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  style={{ flex: "none" }}
                  aria-hidden
                >
                  <circle cx="8" cy="8" r="6.2" />
                  <path d="M8 7.4v3.4" />
                  <circle cx="8" cy="5.2" r="0.6" fill="currentColor" stroke="none" />
                </svg>
                <span>
                  {fillSummary(fills)}
                  <span className="sbar-note" style={{ marginLeft: 8 }}>
                    Only blanks are filled; anything already set is left alone.
                  </span>
                </span>
                <div className="sact">
                  <button
                    type="button"
                    className="sbtn"
                    disabled={filling.busy}
                    onClick={fillFromTickers}
                  >
                    {filling.busy ? "Filling…" : "Fill them in"}
                  </button>
                </div>
              </div>
            )}

            {checked.size > 0 && (
              <div
                className="sbar"
                style={{
                  border: "1px solid var(--color-accent)",
                  background: "color-mix(in srgb, var(--color-accent) 10%, transparent)",
                  marginBottom: -1,
                }}
              >
                <span>
                  {checked.size} asset{checked.size === 1 ? "" : "s"} selected
                </span>
                <div className="sact">
                  <Select
                    style={{ minHeight: 28, fontSize: 12 }}
                    value=""
                    disabled={remapping.busy || offline}
                    aria-label="Set profile for the selected assets"
                    onChange={(e) => {
                      const id = Number(e.target.value);
                      bulkRemap(id === UNMAPPED ? null : id);
                    }}
                  >
                    <option value="">{remapping.busy ? "Remapping…" : "Set profile…"}</option>
                    {raw.returnProfiles.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                    <option value={UNMAPPED}>Unmapped — held flat at 0%</option>
                  </Select>
                  <button type="button" className="sbtn" onClick={() => setChecked(new Set())}>
                    Clear
                  </button>
                </div>
              </div>
            )}

            {rows.length === 0 ? (
              <p style={{ ...NOTE, margin: "4px 0 0", maxWidth: 460, lineHeight: 1.5 }}>
                No assets yet. An asset is a price series — a fund, a house, the
                cash in a savings account — and the profile it points at is what
                makes it move.
              </p>
            ) : (
              <AssetsTable
                rows={rows}
                profiles={returnProfiles}
                selectedId={selectedAsset?.serverId}
                checked={checked}
                onSelect={(row) => select({ kind: "asset", id: row.serverId })}
                onCheck={
                  remapping.busy || offline
                    ? undefined
                    : (row, on) =>
                        setChecked((current) => {
                          const next = new Set(current);
                          if (on) next.add(row.serverId);
                          else next.delete(row.serverId);
                          return next;
                        })
                }
              />
            )}

            {(remapping.error ?? filling.error) && (
              <p style={{ ...NOTE, color: "var(--color-accent-700)" }}>
                {remapping.error ?? filling.error}
              </p>
            )}

            <p style={NOTE}>
              Select rows to remap several holdings at once. Cash and property
              carry a profile the same way a fund does, so nothing needs a
              special case.
            </p>

            <div style={{ marginTop: 26 }}>
              <SectionBar
                title="Return profile library"
                count={String(returnProfiles.length)}
                action={
                  <Button
                    onClick={() => setAddingProfile(true)}
                    disabled={offline}
                    title={offline ? "No connection to the server." : undefined}
                  >
                    New profile
                  </Button>
                }
              />
              <ProfileLibraryTable
                profiles={returnProfiles}
                selectedId={selectedProfile?.id}
                onSelect={(p) => select({ kind: "profile", id: p.id })}
              />
              <p style={NOTE}>
                A profile with no assets is unremarkable here — an account can
                point at one directly for its cash or its property value.
              </p>
            </div>

            <div style={{ marginTop: 26 }}>
              <SectionBar
                title="Inflation"
                count={activeInflationProfile ? "one is in use" : "none in use"}
              />
              {inflationProfiles.length === 0 ? (
                <p style={{ ...NOTE, margin: 0 }}>
                  None defined — the scenario runs without inflation adjustment.
                </p>
              ) : (
                <InflationProfilesTable
                  profiles={inflationProfiles}
                  activeId={activeInflationProfile ?? ""}
                  onActivate={offline ? undefined : onActivateInflation}
                />
              )}
            </div>
          </div>
        }
        rail={
          selectedAsset ? (
            <AssetInspector
              key={selectedAsset.serverId}
              asset={selectedAsset}
              profile={inherited}
              profiles={raw.returnProfiles}
              onApply={(draft) => applyAsset(selectedAsset, draft)}
              onEditProfile={
                inherited && (() => select({ kind: "profile", id: inherited.id }))
              }
              busy={editingAsset.busy}
              error={editingAsset.error}
              offline={offline}
            />
          ) : selectedProfile ? (
            <ProfileInspector
              key={selectedProfile.id}
              profile={selectedProfile}
              presets={histories}
              onApply={(body) => applyProfile(selectedProfile, body)}
              onDuplicate={() => duplicateProfile(selectedProfile)}
              busy={editingProfile.busy}
              error={editingProfile.error}
              offline={offline}
            />
          ) : null
        }
      />

      {addingAsset && (
        <NewAssetDialog
          scenarioId={scenarioId}
          profiles={raw.returnProfiles}
          onClose={() => setAddingAsset(false)}
          onCreated={onChanged}
        />
      )}
      {addingProfile && (
        <NewProfileDialog
          presets={histories}
          onClose={() => setAddingProfile(false)}
          onCreated={(created) => {
            adopt(created);
            onChanged();
          }}
        />
      )}
    </>
  );
}

/** A section's heading, its count and whatever acts on it, on one line. */
function SectionBar({
  title,
  count,
  action,
}: {
  title: string;
  count: string;
  action?: ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "baseline",
        justifyContent: "space-between",
        marginBottom: 8,
        minHeight: 30,
      }}
    >
      <h4 style={{ margin: 0 }}>
        {title}{" "}
        <span
          style={{
            fontFamily: "var(--font-body)",
            fontWeight: 400,
            fontSize: 12,
            color: "color-mix(in srgb, var(--color-text) 50%, transparent)",
          }}
        >
          {count}
        </span>
      </h4>
      {action}
    </div>
  );
}

/** Drops a selection that no longer names a row, then falls back to the first. */
function resolve(
  selection: AssetsSelection | undefined,
  rows: AssetRow[],
  profiles: ReturnProfile[],
): AssetsSelection | undefined {
  if (selection?.kind === "asset" && rows.some((r) => r.serverId === selection.id)) {
    return selection;
  }
  if (selection?.kind === "profile" && profiles.some((p) => p.id === selection.id)) {
    return selection;
  }
  if (rows.length > 0) return { kind: "asset", id: rows[0].serverId };
  return profiles[0] ? { kind: "profile", id: profiles[0].id } : undefined;
}

/** `US Total Market copy 2` — the first name the library does not already hold. */
function copyName(name: string, profiles: ReturnProfile[]): string {
  const taken = new Set(profiles.map((p) => p.id));
  const base = `${name} copy`;
  if (!taken.has(base)) return base;
  for (let n = 2; ; n++) {
    const next = `${base} ${n}`;
    if (!taken.has(next)) return next;
  }
}
