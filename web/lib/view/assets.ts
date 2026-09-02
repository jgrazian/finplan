/**
 * `api::assets::Asset` → the Assets & returns screen's grouped list.
 *
 * A ticker only exists to point at a return profile, so the profile — not the
 * asset — is the outline level: every asset hangs under the profile that makes
 * it move. Profiles with no assets still appear, because an account can point
 * at one directly for its cash or its property value.
 *
 * The holdings behind an asset are not on the asset row at all; they are lots
 * scattered across accounts, so they are gathered here once rather than by the
 * inspector on every selection.
 */
import type { Account as ApiAccount, Asset } from "@/lib/api/types";
import type { ReturnProfile, ReturnProfileId } from "@/lib/types";

/** One account's stake in an asset, its lots summed. */
export interface AssetHolding {
  account: string;
  units: number;
  costBasis: number;
}

export interface AssetRow {
  /** Database id, for the mutation endpoints. */
  serverId: number;
  /** The asset's short name — the ticker the list is keyed by. */
  ticker: string;
  /** Its longer description, blank where none was given. */
  name: string;
  price: number;
  /**
   * Name of the profile driving it, and the id a PATCH needs. Both null while
   * the asset is unmapped — it has a price and nothing moving it, which the
   * engine reads as flat zero growth.
   */
  profileId: ReturnProfileId | null;
  profileServerId: number | null;
  units: number;
  costBasis: number;
  holdings: AssetHolding[];
}

export interface ProfileGroup {
  /** Null for the unmapped bucket, which is a hole in the outline, not a row. */
  profile: ReturnProfile | null;
  assets: AssetRow[];
}

/** Either level of the outline; the inspector switches on `kind`. */
export type AssetsSelection =
  | { kind: "profile"; id: ReturnProfileId }
  | { kind: "asset"; id: number };

export function groupAssetsByProfile(
  assets: Asset[],
  accounts: ApiAccount[],
  profiles: ReturnProfile[],
): ProfileGroup[] {
  const holdings = holdingsByAsset(accounts);
  const byProfile = new Map<number, AssetRow[]>();
  const unmapped: AssetRow[] = [];

  for (const asset of assets) {
    const held = holdings.get(asset.id) ?? [];
    const mapping = asset.return_profile_id;
    const row: AssetRow = {
      serverId: asset.id,
      ticker: asset.name,
      name: asset.description ?? "",
      price: asset.initial_price,
      profileId:
        mapping == null ? null : (profiles.find((p) => p.serverId === mapping)?.id ?? null),
      profileServerId: mapping,
      units: held.reduce((sum, h) => sum + h.units, 0),
      costBasis: held.reduce((sum, h) => sum + h.costBasis, 0),
      holdings: held,
    };
    if (mapping == null) {
      unmapped.push(row);
      continue;
    }
    const rows = byProfile.get(mapping);
    if (rows) rows.push(row);
    else byProfile.set(mapping, [row]);
  }

  const mapped = profiles.map((profile) => ({
    profile,
    assets: byProfile.get(profile.serverId) ?? [],
  }));

  // The bucket leads the outline when it has anything in it: an unmapped asset
  // is a to-do, not a category, and it holds its price flat until it is cleared.
  return unmapped.length > 0 ? [{ profile: null, assets: unmapped }, ...mapped] : mapped;
}

/** Flattens the outline for rendering: a group row, then each of its assets. */
export function flatten(
  groups: ProfileGroup[],
): Array<{ group: ProfileGroup; asset?: AssetRow }> {
  return groups.flatMap((group) => [
    { group },
    ...group.assets.map((asset) => ({ group, asset })),
  ]);
}

export function findAsset(groups: ProfileGroup[], id: number): AssetRow | undefined {
  for (const group of groups) {
    const found = group.assets.find((a) => a.serverId === id);
    if (found) return found;
  }
  return undefined;
}

/** asset id → one entry per account holding it, its lots summed. */
function holdingsByAsset(accounts: ApiAccount[]): Map<number, AssetHolding[]> {
  const out = new Map<number, AssetHolding[]>();
  for (const account of accounts) {
    const perAsset = new Map<number, AssetHolding>();
    for (const lot of account.positions) {
      const held = perAsset.get(lot.asset_id);
      if (held) {
        held.units += lot.units;
        held.costBasis += lot.cost_basis;
      } else {
        perAsset.set(lot.asset_id, {
          account: account.name,
          units: lot.units,
          costBasis: lot.cost_basis,
        });
      }
    }
    for (const [assetId, held] of perAsset) {
      const rows = out.get(assetId);
      if (rows) rows.push(held);
      else out.set(assetId, [held]);
    }
  }
  return out;
}
