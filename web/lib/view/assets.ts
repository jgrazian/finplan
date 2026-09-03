/**
 * `api::assets::Asset` → the Assets screen's rows.
 *
 * Holdings are the list. A return profile is what a holding points at, so it
 * is a column on the asset rather than the level above it — a library of
 * profiles sits below the table, where a row can be edited without first
 * finding an asset that happens to use it.
 *
 * The lots behind an asset are not on the asset row at all; they are scattered
 * across accounts, so they are gathered here once rather than by the inspector
 * on every selection.
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
  /** Units marked at the opening price — what a balance counts. */
  value: number;
  costBasis: number;
  holdings: AssetHolding[];
  /** The accounts holding it, in one line: `401(k) · Brokerage · Roth IRA`. */
  heldIn: string;
}

/** Either level of the screen; the inspector switches on `kind`. */
export type AssetsSelection =
  | { kind: "profile"; id: ReturnProfileId }
  | { kind: "asset"; id: number };

/**
 * `profile:US equities` / `asset:12` — both levels as one URL token, since only
 * one of them is ever selected.
 *
 * A profile is named, an asset is not: a ticker can be renamed to one that
 * another asset already had, and the row id cannot.
 */
export function encodeAssetsSelection(selection: AssetsSelection): string {
  return `${selection.kind}:${selection.id}`;
}

/** The reverse; anything malformed reads as no selection, not as an error. */
export function decodeAssetsSelection(token: string | undefined): AssetsSelection | undefined {
  const cut = token?.indexOf(":") ?? -1;
  if (token == null || cut < 0) return undefined;
  const id = token.slice(cut + 1);
  // A profile id is its name, so only the empty one is impossible.
  if (token.slice(0, cut) === "profile") return id === "" ? undefined : { kind: "profile", id };
  if (token.slice(0, cut) !== "asset") return undefined;
  const serverId = Number(id);
  return Number.isSafeInteger(serverId) ? { kind: "asset", id: serverId } : undefined;
}

export function toAssetRows(
  assets: Asset[],
  accounts: ApiAccount[],
  profiles: ReturnProfile[],
): AssetRow[] {
  const holdings = holdingsByAsset(accounts);
  return assets.map((asset) => {
    const held = holdings.get(asset.id) ?? [];
    const mapping = asset.return_profile_id;
    const units = held.reduce((sum, h) => sum + h.units, 0);
    return {
      serverId: asset.id,
      ticker: asset.name,
      name: asset.description ?? "",
      price: asset.initial_price,
      profileId:
        mapping == null ? null : (profiles.find((p) => p.serverId === mapping)?.id ?? null),
      profileServerId: mapping,
      units,
      value: units * asset.initial_price,
      costBasis: held.reduce((sum, h) => sum + h.costBasis, 0),
      holdings: held,
      heldIn: heldInLabel(held),
    };
  });
}

/** What every holding of every asset is worth at its opening price. */
export function assetsTotal(rows: AssetRow[]): number {
  return rows.reduce((sum, row) => sum + row.value, 0);
}

/** Asset ids per profile row id, for the library's "Used by" column. */
export function assetsPerProfile(rows: AssetRow[]): Map<number, number> {
  const out = new Map<number, number>();
  for (const row of rows) {
    if (row.profileServerId == null) continue;
    out.set(row.profileServerId, (out.get(row.profileServerId) ?? 0) + 1);
  }
  return out;
}

/**
 * Account names up to three, then a count. Four names is wider than the column
 * and says less than the number does.
 */
function heldInLabel(held: AssetHolding[]): string {
  if (held.length === 0) return "—";
  if (held.length > 3) return `${held.length} accounts`;
  return held.map((h) => h.account).join(" · ");
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
