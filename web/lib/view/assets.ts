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
import type { Account as ApiAccount, Asset, Profile } from "@/lib/api/types";
import { tickerDefaults } from "@/lib/tickers";
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

/** One asset with a blank the ticker table can fill, and what it would write. */
export interface TickerFill {
  serverId: number;
  ticker: string;
  /** The name it would be given; absent where it already has one. */
  name?: string;
  /** The profile it would be mapped to; absent where it is already mapped. */
  profileServerId?: number;
}

/**
 * The assets whose ticker says more than the row does.
 *
 * Only blanks count. An asset someone has named "the house fund" is not
 * improved by being renamed to what a table thinks `VNQ` is, and an asset
 * pointed at a deliberately pessimistic profile must not be moved off it — so a
 * filled field is never a candidate, and the two halves are decided separately.
 *
 * This is the same lookup the drawer offers one asset at a time. It is worth
 * having twice because the case that matters is a library of assets created
 * before any of this existed, where doing it one drawer at a time is the tedium
 * that stops it being done at all.
 */
export function tickerFills(rows: AssetRow[], profiles: Profile[]): TickerFill[] {
  const out: TickerFill[] = [];
  for (const row of rows) {
    const known = tickerDefaults(row.ticker, profiles);
    if (!known) continue;
    const name = row.name.trim() === "" ? known.name : undefined;
    const profileServerId = row.profileServerId == null ? known.profile?.id : undefined;
    if (name == null && profileServerId == null) continue;
    out.push({ serverId: row.serverId, ticker: row.ticker, name, profileServerId });
  }
  return out;
}

/**
 * `3 assets can be named and mapped from their tickers — VTI, BND, VNQ`.
 *
 * The verb is whichever halves are actually missing, and the tickers are named
 * rather than counted up to the point where naming them is longer than the row
 * they would send you to.
 */
export function fillSummary(fills: TickerFill[]): string {
  const naming = fills.some((f) => f.name != null);
  const mapping = fills.some((f) => f.profileServerId != null);
  const verb = naming && mapping ? "named and mapped" : naming ? "named" : "mapped";
  const shown = fills.slice(0, 4).map((f) => f.ticker);
  const rest = fills.length - shown.length;
  const list = rest > 0 ? `${shown.join(", ")} +${rest} more` : shown.join(", ");
  return `${fills.length} asset${fills.length === 1 ? "" : "s"} can be ${verb} from ${
    fills.length === 1 ? "its ticker" : "their tickers"
  } — ${list}`;
}
