/**
 * `api::accounts::Account` → the Portfolio screen's `Account`.
 *
 * Four things the API deliberately does not compute are worked out here:
 * a signed balance (positions are lots, not a total), the profile *name* behind
 * a profile id, a one-line summary of what the account holds, and the reverse
 * edge from an account to the events that touch it — which only exists once the
 * event trees are walked.
 */
import type {
  Account as ApiAccount,
  Asset,
  Event as ApiEvent,
  Profile,
} from "@/lib/api/types";
import type {
  Account,
  AccountId,
  AssetLot,
  HoldingsLine,
  LinkedAccount,
  TaxStatus,
} from "@/lib/types";
import { fmtCurrency } from "@/lib/format";
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

  const mapped: Account[] = accounts.map((account) => {
    const positions = account.positions.map((lot): AssetLot => {
      const asset = assetById.get(lot.asset_id);
      return {
        positionId: lot.id,
        assetId: asset?.name ?? `asset ${lot.asset_id}`,
        assetServerId: lot.asset_id,
        purchaseDate: lot.purchase_date,
        units: lot.units,
        costBasis: lot.cost_basis,
        value: lot.units * (asset?.initial_price ?? 0),
      };
    });

    const profilesHeld = profileLabels(account, positions, assetById, profileName);
    return {
      accountId: String(account.id),
      serverId: account.id,
      name: account.name,
      flavor: account.flavor,
      taxStatus: taxStatusOf(account),
      balance: balanceOf(account, positions),
      returnProfileId: profilesHeld.join(", "),
      returnProfiles: profilesHeld,
      returnProfileServerId: ownProfileOf(account),
      cashValue: cashValueOf(account),
      holdings: holdingsLabel(account, positions, assetById),
      contributionLimit:
        account.flavor === "Investment" &&
        account.contribution_limit != null &&
        account.contribution_period != null
          ? {
              amount: account.contribution_limit,
              period: account.contribution_period,
              catchUp: account.catch_up,
            }
          : undefined,
      planType: (account.flavor === "Investment" && account.plan_type) || undefined,
      assetServerId: account.flavor === "Property" ? account.asset_id : undefined,
      interestRate: account.flavor === "Liability" ? account.interest_rate : undefined,
      repayment:
        account.flavor === "Liability" && account.repayment
          ? {
              fromAccountId: account.repayment.from_account_id,
              termMonths: account.repayment.term_months,
            }
          : undefined,
      linked: [],
      positions,
      referencedBy: referencedBy.get(account.id) ?? [],
    };
  });

  const links = buildLinks(mapped, events);
  for (const account of mapped) account.linked = links.get(account.accountId) ?? [];
  return mapped;
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
function balanceOf(account: ApiAccount, positions: AssetLot[]): number {
  switch (account.flavor) {
    case "Bank":
      return account.cash_value;
    case "Investment":
      return positions.reduce((total, lot) => total + lot.value, account.cash_value);
    case "Property":
      return account.value;
    case "Liability":
      // Stored as a positive amount owed; the portfolio shows it as a debt.
      return -account.principal;
  }
}

/** Cash the account holds outright, as opposed to marked holdings. */
function cashValueOf(account: ApiAccount): number {
  switch (account.flavor) {
    case "Bank":
    case "Investment":
      return account.cash_value;
    default:
      return 0;
  }
}

/**
 * The profile row the account itself points at — the only one a PATCH on the
 * account can move. A property grows by its asset's profile and a liability by
 * its own interest rate, so neither has one.
 */
function ownProfileOf(account: ApiAccount): number | undefined {
  switch (account.flavor) {
    case "Bank":
      return account.return_profile_id;
    case "Investment":
      return account.cash_return_profile_id;
    default:
      return undefined;
  }
}

/**
 * The profiles an account grows by, one per entry. An investment account's
 * are its holdings' profiles, largest holding's first, so a list that shows
 * only the first shows the one that moves the balance most.
 */
function profileLabels(
  account: ApiAccount,
  positions: AssetLot[],
  assetById: Map<number, Asset>,
  profileName: Map<number, string>,
): string[] {
  // Null as well as undefined: an unmapped asset has no profile to name, and
  // the dash is the honest answer for both.
  const name = (id: number | null | undefined) =>
    id == null ? "—" : (profileName.get(id) ?? `profile ${id}`);

  switch (account.flavor) {
    case "Bank":
      return [name(account.return_profile_id)];
    case "Investment": {
      // What the account grows by is its holdings' profiles; the cash profile
      // only applies to an idle balance.
      const byProfile = new Map<string, number>();
      for (const lot of positions) {
        const label = name(assetById.get(lot.assetServerId)?.return_profile_id);
        byProfile.set(label, (byProfile.get(label) ?? 0) + lot.value);
      }
      const held = [...byProfile].sort((a, b) => b[1] - a[1]).map(([label]) => label);
      return held.length > 0 ? held : [`${name(account.cash_return_profile_id)} (cash)`];
    }
    case "Property":
      return [name(assetById.get(account.asset_id)?.return_profile_id)];
    case "Liability":
      // A liability compounds at its own interest rate, not a market profile.
      return [`${(account.interest_rate * 100).toFixed(2)}% interest`];
  }
}

/** How many holdings the list names before it starts counting the rest. */
const HOLDINGS_SHOWN = 2;

/**
 * `VFIAX 50 · VGPMX 17 · +6` beside `8 lots` — the thing you would otherwise
 * open the drawer to check, which is the whole reason the column earns its
 * width. The weights drop their % sign so the line fits a fixed-height row;
 * the title carries the full breakdown.
 */
function holdingsLabel(
  account: ApiAccount,
  positions: AssetLot[],
  assetById: Map<number, Asset>,
): HoldingsLine {
  const only = (held: string): HoldingsLine => ({ held, detail: held });
  switch (account.flavor) {
    case "Bank":
      return only("Cash — untracked");
    case "Property":
      return only(assetById.get(account.asset_id)?.name ?? `asset ${account.asset_id}`);
    case "Liability":
      return only(`Owes ${fmtCurrency(account.principal)}`);
    case "Investment":
      break;
  }

  if (positions.length === 0) return only("Cash — untracked");

  // Lots of the same ticker are one holding on this line; which lot a sale
  // comes out of is the liquidation strategy's business, not the list's.
  const byAsset = new Map<string, number>();
  for (const lot of positions) {
    byAsset.set(lot.assetId, (byAsset.get(lot.assetId) ?? 0) + lot.value);
  }
  const held = [...byAsset].sort((a, b) => b[1] - a[1]);
  const total = held.reduce((sum, [, value]) => sum + value, 0);
  const weight = (value: number) => Math.round((value / total) * 100);

  const named = held
    .slice(0, HOLDINGS_SHOWN)
    .map(([name, value]) => (total > 0 ? `${name} ${weight(value)}` : name));
  const rest = held.length - named.length;
  if (rest > 0) named.push(`+${rest}`);

  return {
    held: named.join(" · "),
    lots: `${positions.length} lot${positions.length === 1 ? "" : "s"}`,
    detail: held
      .map(([name, value]) => (total > 0 ? `${name} ${weight(value)}%` : name))
      .join(" · "),
  };
}

/**
 * Property ↔ debt pairs, read back off the plan.
 *
 * Nothing in the schema says a mortgage encumbers a house: what ties them is an
 * event that settles both sides at once. So two accounts are paired when some
 * event names them together and they sit on opposite ends of the pair — which
 * is also the only relation a sale event could act on.
 */
function buildLinks(
  accounts: Account[],
  events: ApiEvent[],
): Map<AccountId, LinkedAccount[]> {
  const byServerId = new Map(accounts.map((a) => [a.serverId, a]));
  const out = new Map<AccountId, LinkedAccount[]>();

  const pair = (from: Account, to: Account, event: string) => {
    const list = out.get(from.accountId) ?? [];
    const seen = list.find((l) => l.accountId === to.accountId);
    if (seen) {
      if (!seen.through.includes(event)) seen.through.push(event);
      return;
    }
    list.push({
      accountId: to.accountId,
      name: to.name,
      flavor: to.flavor,
      balance: to.balance,
      interestRate: to.interestRate,
      through: [event],
    });
    out.set(from.accountId, list);
  };

  for (const event of events) {
    const named = [...accountRefs(event)]
      .map((id) => byServerId.get(id))
      .filter((a): a is Account => a != null);
    const properties = named.filter((a) => a.flavor === "Property");
    const debts = named.filter((a) => a.flavor === "Liability");
    for (const property of properties) {
      for (const debt of debts) {
        pair(property, debt, event.name);
        pair(debt, property, event.name);
      }
    }
  }
  return out;
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

// ── portfolio summary ───────────────────────────────────────────────────────

/**
 * The categorical series, in their validated order, shared by the composition
 * bar, its legend and the share column — so a colour means the same account
 * wherever it appears. Hues rather than one ramp's steps: eight accounts in
 * eight shades of one blue cannot be told apart.
 */
const ACCOUNT_COLORS = [
  "var(--color-series-1)",
  "var(--color-series-2)",
  "var(--color-series-3)",
  "var(--color-series-4)",
  "var(--color-series-5)",
  "var(--color-series-6)",
  "var(--color-series-7)",
  "var(--color-series-8)",
];

/** account id → its swatch, assigned by position in the list. */
export function accountColors(accounts: Account[]): Map<AccountId, string> {
  return new Map(
    accounts.map((a, i) => [a.accountId, ACCOUNT_COLORS[i % ACCOUNT_COLORS.length]]),
  );
}

export interface CompositionSlice {
  accountId: AccountId;
  label: string;
  color: string;
  /** Fraction of gross assets. */
  share: number;
}

export interface TaxBand {
  label: string;
  value: number;
  /** Fraction of the investable pool, so the bands stack into one bar. */
  share: number;
  color: string;
}

export interface PortfolioSummary {
  netWorth: number;
  /** Everything with a positive balance. */
  assets: number;
  /** Everything with a negative one, as a positive amount owed. */
  debt: number;
  slices: CompositionSlice[];
  taxBands: TaxBand[];
  /** Untracked cash inside the investable pool, and what it holds marked. */
  cash: number;
  invested: number;
}

/**
 * Three steps of one blue, dark to light, so tax treatment reads as an ordered
 * split of one pool rather than three unrelated identities. The Tax column's
 * pills (`.tag-tax-*`) fill with the same tokens, so a band and its accounts
 * share a swatch. The ramp reverses with the ground, so the order holds in both.
 */
const TAX_BANDS: ReadonlyArray<{ label: string; status: TaxStatus; color: string }> = [
  { label: "Deferred", status: "TaxDeferred", color: "var(--color-accent-700)" },
  { label: "Taxable", status: "Taxable", color: "var(--color-accent-500)" },
  { label: "Tax-free", status: "TaxFree", color: "var(--color-accent-300)" },
];

/**
 * The two figures above the list: what the portfolio is worth and how it is
 * taxed. Both are read off the same view models the table shows, so a balance
 * cannot disagree with the total it is part of.
 */
export function portfolioSummary(accounts: Account[]): PortfolioSummary {
  const colors = accountColors(accounts);
  const assets = accounts.reduce((sum, a) => sum + Math.max(a.balance, 0), 0);
  const debt = accounts.reduce((sum, a) => sum - Math.min(a.balance, 0), 0);

  const slices = accounts
    .filter((a) => a.balance > 0)
    .map((a) => ({
      accountId: a.accountId,
      label: a.name,
      color: colors.get(a.accountId) ?? ACCOUNT_COLORS[0],
      share: assets > 0 ? a.balance / assets : 0,
    }));

  // Property and debt are not investable: nothing chooses to hold them for a
  // return, so they would only dilute the treatment they are not part of.
  const investable = accounts.filter(
    (a) => a.flavor === "Bank" || a.flavor === "Investment",
  );
  const totals = TAX_BANDS.map((band) =>
    investable
      .filter((a) => a.taxStatus === band.status)
      .reduce((sum, a) => sum + a.balance, 0),
  );
  // Clamped at zero: an overdrawn cash balance can push a band negative, and
  // a stacked bar has no way to draw less than nothing.
  const pool = totals.reduce((sum, v) => sum + Math.max(v, 0), 0);
  const taxBands = TAX_BANDS.map((band, i) => ({
    label: band.label,
    value: totals[i],
    share: pool > 0 ? Math.max(totals[i], 0) / pool : 0,
    color: band.color,
  }));

  const cash = investable.reduce((sum, a) => sum + a.cashValue, 0);
  const invested = investable.reduce((sum, a) => sum + (a.balance - a.cashValue), 0);

  return { netWorth: assets - debt, assets, debt, slices, taxBands, cash, invested };
}
