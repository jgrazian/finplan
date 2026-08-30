"use client";

import { useMemo, useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  AssetInspector,
  AssetProfileList,
  NewAssetDialog,
  type AssetDraft,
} from "@/components/portfolio";
import { InflationProfilesTable, ProfileInspector } from "@/components/profiles";
import { Button } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { RawWorkspace } from "@/lib/hooks/useWorkspace";
import { useSubmit } from "@/lib/hooks/useSubmit";
import type { InflationProfile, ReturnProfile } from "@/lib/types";
import {
  type AssetRow,
  type AssetsSelection,
  findAsset,
  groupAssetsByProfile,
} from "@/lib/view/assets";
import { isReadOnlyPreset } from "@/lib/view/profiles";
import { EmptyState } from "./EmptyState";

/**
 * Portfolio › Assets & returns — one tab where there were two.
 *
 * A ticker exists only to point at a return profile, so the two tables were
 * always one relation shown twice: the profile is the group and the assets it
 * drives are its rows. The rail is polymorphic — it inspects whichever level
 * is selected — which is what lets a remap stay on one screen.
 *
 * Inflation profiles hang below the outline rather than in the rail: they
 * drive the scenario, not any asset, so they have no place in the grouping.
 */
export function AssetsReturnsScreen({
  scenarioId,
  raw,
  returnProfiles,
  inflationProfiles,
  activeInflationProfile,
  onActivateInflation,
  onChanged,
}: {
  scenarioId: number;
  raw: RawWorkspace;
  returnProfiles: ReturnProfile[];
  inflationProfiles: InflationProfile[];
  activeInflationProfile: string | undefined;
  onActivateInflation?: (profile: InflationProfile) => void;
  onChanged: () => void;
}) {
  const [picked, setPicked] = useState<AssetsSelection>();
  const [adding, setAdding] = useState(false);
  // Two trackers, because the two writes report in different places: a remap
  // is driven from the list, an edit from the rail.
  const remapping = useSubmit();
  const editing = useSubmit();

  const groups = useMemo(
    () => groupAssetsByProfile(raw.assets, raw.accounts, returnProfiles),
    [raw.assets, raw.accounts, returnProfiles],
  );

  // Derived rather than reset in an effect: switching scenarios replaces every
  // asset id, and the first profile is the right fallback for a stale pick.
  const selection = resolve(picked, groups) ?? fallback(groups);
  const selectedAsset =
    selection?.kind === "asset" ? findAsset(groups, selection.id) : undefined;
  const selectedProfile =
    selection?.kind === "profile"
      ? returnProfiles.find((p) => p.id === selection.id)
      : returnProfiles.find((p) => p.serverId === selectedAsset?.profileServerId);

  if (returnProfiles.length === 0) {
    return (
      <EmptyState
        title="No return profiles"
        detail="Registration seeds a starter library; if it is empty, every asset in every scenario has nothing to grow by."
      />
    );
  }

  const remap = (asset: AssetRow, profileServerId: number) =>
    remapping.run(
      () => api.assets.update(scenarioId, asset.serverId, { return_profile_id: profileServerId }),
      onChanged,
    );

  const apply = (asset: AssetRow, draft: AssetDraft) => {
    const ticker = draft.ticker.trim();
    const price = Number(draft.price);
    // `PATCH` coalesces nulls onto the stored row, so it cannot police these
    // the way `POST` does: a blank ticker or a zero price would be written.
    if (ticker === "") return editing.fail("An asset needs a ticker.");
    if (!Number.isFinite(price) || price <= 0) {
      return editing.fail("Opening price must be positive.");
    }
    editing.run(
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

  return (
    <>
      <SplitPane
        railWidth={344}
        main={
          <div style={{ padding: "14px 20px 18px" }}>
            <div
              style={{
                display: "flex",
                alignItems: "baseline",
                justifyContent: "space-between",
                marginBottom: 10,
              }}
            >
              <h4 style={{ margin: 0 }}>
                Assets by return profile{" "}
                <span className="text-muted" style={{ fontSize: 13 }}>
                  {raw.assets.length}
                </span>
              </h4>
              <div style={{ display: "flex", gap: 8 }}>
                <Button variant="ghost" disabled>
                  New profile
                </Button>
                <Button shortcut="a" onClick={() => setAdding(true)}>
                  Add asset
                </Button>
              </div>
            </div>

            <AssetProfileList
              groups={groups}
              selection={selection}
              onSelect={setPicked}
              onRemap={remapping.busy ? undefined : remap}
            />

            {remapping.error && (
              <p style={{ fontSize: 12, margin: "10px 0 0", color: "var(--color-accent-700)" }}>
                {remapping.error}
              </p>
            )}

            <p
              style={{
                fontSize: 12,
                margin: "14px 0 0",
                color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
              }}
            >
              Drag an asset onto another profile to remap it. Profiles with no assets
              still show, since an account can point at one directly for its cash or
              its property value.
            </p>

            <h4 style={{ margin: "24px 0 8px" }}>Inflation profiles</h4>
            {inflationProfiles.length === 0 ? (
              <p className="text-muted" style={{ fontSize: 12 }}>
                None defined — the scenario runs without inflation adjustment.
              </p>
            ) : (
              <InflationProfilesTable
                profiles={inflationProfiles}
                activeId={activeInflationProfile ?? ""}
                onActivate={onActivateInflation}
              />
            )}
          </div>
        }
        rail={
          selectedAsset ? (
            <AssetInspector
              key={selectedAsset.serverId}
              asset={selectedAsset}
              profile={selectedProfile}
              profiles={raw.returnProfiles}
              onApply={(draft) => apply(selectedAsset, draft)}
              busy={editing.busy}
              error={editing.error}
            />
          ) : selectedProfile ? (
            <ProfileInspector
              profile={selectedProfile}
              readOnly={isReadOnlyPreset(selectedProfile)}
            />
          ) : null
        }
      />

      {adding && (
        <NewAssetDialog
          scenarioId={scenarioId}
          profiles={raw.returnProfiles}
          onClose={() => setAdding(false)}
          onCreated={onChanged}
        />
      )}
    </>
  );
}

/** Drops a selection that no longer names a row — a deleted or remapped asset. */
function resolve(
  selection: AssetsSelection | undefined,
  groups: ReturnType<typeof groupAssetsByProfile>,
): AssetsSelection | undefined {
  if (!selection) return undefined;
  if (selection.kind === "asset") {
    return findAsset(groups, selection.id) ? selection : undefined;
  }
  return groups.some((g) => g.profile.id === selection.id) ? selection : undefined;
}

/** The first profile that actually drives something, else the first profile. */
function fallback(
  groups: ReturnType<typeof groupAssetsByProfile>,
): AssetsSelection | undefined {
  const group = groups.find((g) => g.assets.length > 0) ?? groups[0];
  return group ? { kind: "profile", id: group.profile.id } : undefined;
}
