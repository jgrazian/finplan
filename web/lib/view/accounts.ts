/**
 * `api::accounts::Account` → the Portfolio screen's `Account`.
 *
 * Three things the API deliberately does not compute are worked out here:
 * a signed balance (positions are lots, not a total), the profile *name* behind
 * a profile id, and the reverse edge from an account to the events that touch
 * it — which only exists once the event trees are walked.
 */
import type {
  Account as ApiAccount,
  Asset,
  Event as ApiEvent,
  Profile,
} from "@/lib/api/types";
import type { Account, AssetLot, TaxStatus } from "@/lib/types";
import { accountRefs } from "./refs";

export interface PortfolioContext {
  assets: Asset[];
  profiles: Profile[];
  events: ApiEvent[];
}

export function toViewAccounts(
  accounts: ApiAccount[],
  { assets, profiles, events }: PortfolioContext,
): Account[] {
  const assetById = new Map(assets.map((a) => [a.id, a]));
  const profileName = new Map(profiles.map((p) => [p.id, p.name]));
  const referencedBy = buildReferences(events);

  return accounts.map((account) => ({
    accountId: String(account.id),
    serverId: account.id,
    name: account.name,
    flavor: account.flavor,
    taxStatus: taxStatusOf(account),
    balance: balanceOf(account, assetById),
    returnProfileId: profileLabel(account, assetById, profileName),
    contributionLimit:
      account.flavor === "Investment" &&
      account.contribution_limit != null &&
      account.contribution_period != null
        ? { amount: account.contribution_limit, period: account.contribution_period }
        : undefined,
    positions: account.positions.map(
      (lot): AssetLot => ({
        assetId: assetById.get(lot.asset_id)?.name ?? `asset ${lot.asset_id}`,
        purchaseDate: lot.purchase_date,
        units: lot.units,
        costBasis: lot.cost_basis,
      }),
    ),
    referencedBy: referencedBy.get(account.id) ?? [],
  }));
}

function taxStatusOf(account: ApiAccount): TaxStatus | undefined {
  if (account.flavor === "Investment") return account.tax_status;
  // Interest on a bank account is ordinary income; property and debt have no
  // tax treatment of their own.
  if (account.flavor === "Bank") return "Taxable";
  return undefined;
}

/**
 * Signed present value. Lots carry units and cost basis but no price, so
 * holdings are marked at the asset's opening price — the same figure the engine
 * starts the simulation from.
 */
function balanceOf(account: ApiAccount, assetById: Map<number, Asset>): number {
  switch (account.flavor) {
    case "Bank":
      return account.cash_value;
    case "Investment":
      return account.positions.reduce(
        (total, lot) => total + lot.units * (assetById.get(lot.asset_id)?.initial_price ?? 0),
        account.cash_value,
      );
    case "Property":
      return account.value;
    case "Liability":
      // Stored as a positive amount owed; the portfolio shows it as a debt.
      return -account.principal;
  }
}

function profileLabel(
  account: ApiAccount,
  assetById: Map<number, Asset>,
  profileName: Map<number, string>,
): string {
  const name = (id: number | undefined) =>
    id == null ? "—" : (profileName.get(id) ?? `profile ${id}`);

  switch (account.flavor) {
    case "Bank":
      return name(account.return_profile_id);
    case "Investment": {
      // What the account grows by is its holdings' profiles; the cash profile
      // only applies to an idle balance.
      const held = [
        ...new Set(
          account.positions.map((lot) =>
            name(assetById.get(lot.asset_id)?.return_profile_id),
          ),
        ),
      ];
      return held.length > 0
        ? held.join(", ")
        : `${name(account.cash_return_profile_id)} (cash)`;
    }
    case "Property":
      return name(assetById.get(account.asset_id)?.return_profile_id);
    case "Liability":
      // A liability compounds at its own interest rate, not a market profile.
      return `${(account.interest_rate * 100).toFixed(2)}% interest`;
  }
}

/** account id → names of the events whose trigger or effects mention it. */
function buildReferences(events: ApiEvent[]): Map<number, string[]> {
  const out = new Map<number, string[]>();
  for (const event of events) {
    for (const accountId of accountRefs(event)) {
      const names = out.get(accountId);
      if (names) names.push(event.name);
      else out.set(accountId, [event.name]);
    }
  }
  return out;
}

/** Share of gross (positive-value) holdings; liabilities get no share. */
export function accountShares(accounts: Account[]): Map<string, number | null> {
  const gross = accounts
    .filter((a) => a.balance > 0)
    .reduce((sum, a) => sum + a.balance, 0);
  return new Map(
    accounts.map((a) => [a.accountId, a.balance > 0 && gross > 0 ? a.balance / gross : null]),
  );
}
