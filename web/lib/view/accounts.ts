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

    return {
      accountId: String(account.id),
      serverId: account.id,
      name: account.name,
      flavor: account.flavor,
      taxStatus: taxStatusOf(account),
      balance: balanceOf(account, positions),
      returnProfileId: profileLabel(account, assetById, profileName),
      returnProfileServerId: ownProfileOf(account),
      cashValue: cashValueOf(account),
      holdings: holdingsLabel(account, positions, assetById),
      contributionLimit:
        account.flavor === "Investment" &&
        account.contribution_limit != null &&
        account.contribution_period != null
          ? { amount: account.contribution_limit, period: account.contribution_period }
          : undefined,
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

function profileLabel(
  account: ApiAccount,
  assetById: Map<number, Asset>,
  profileName: Map<number, string>,
): string {
  // Null as well as undefined: an unmapped asset has no profile to name, and
  // the dash is the honest answer for both.
  const name = (id: number | null | undefined) =>
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

/** How many holdings the list names before it starts counting the rest. */
const HOLDINGS_SHOWN = 2;

/**
 * `SPY 78% · BND 22% · 3 lots` — the thing you would otherwise open the drawer
 * to check, which is the whole reason the column earns its width.
 */
function holdingsLabel(
  account: ApiAccount,
  positions: AssetLot[],
  assetById: Map<number, Asset>,
): string {
  switch (account.flavor) {
    case "Bank":
      return "Cash — untracked";
    case "Property":
      return assetById.get(account.asset_id)?.name ?? `asset ${account.asset_id}`;
    case "Liability":
      return `Owes ${fmtCurrency(account.principal)}`;
    case "Investment":
      break;
  }

  if (positions.length === 0) return "Cash — untracked";

  // Lots of the same ticker are one holding on this line; which lot a sale
  // comes out of is the liquidation strategy's business, not the list's.
  const byAsset = new Map<string, number>();
  for (const lot of positions) {
    byAsset.set(lot.assetId, (byAsset.get(lot.assetId) ?? 0) + lot.value);
  }
  const held = [...byAsset].sort((a, b) => b[1] - a[1]);
  const total = held.reduce((sum, [, value]) => sum + value, 0);

  const named = held
    .slice(0, HOLDINGS_SHOWN)
    .map(([name, value]) =>
      total > 0 ? `${name} ${Math.round((value / total) * 100)}%` : name,
    );
  const rest = held.length - named.length;
  if (rest > 0) named.push(`+${rest} more`);
  named.push(`${positions.length} lot${positions.length === 1 ? "" : "s"}`);
  return named.join(" · ");
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
 * One accent ramp, darkest first, shared by the composition bar, its legend and
 * the share column — so a colour means the same account wherever it appears.
 */
const ACCOUNT_COLORS = [
  "var(--color-accent-900)",
  "var(--color-accent-700)",
  "var(--color-accent)",
  "var(--color-accent-500)",
  "var(--color-accent-300)",
  "var(--color-accent-800)",
  "var(--color-accent-400)",
  "var(--color-accent-200)",
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
  /** Fraction of the largest band, so the bars are read against each other. */
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

const TAX_BANDS: ReadonlyArray<{ label: string; status: TaxStatus; color: string }> = [
  { label: "Deferred", status: "TaxDeferred", color: "var(--color-accent-900)" },
  { label: "Taxable", status: "Taxable", color: "var(--color-accent-700)" },
  { label: "Tax-free", status: "TaxFree", color: "var(--color-accent-500)" },
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
  const largest = Math.max(0, ...totals);
  const taxBands = TAX_BANDS.map((band, i) => ({
    label: band.label,
    value: totals[i],
    share: largest > 0 ? totals[i] / largest : 0,
    color: band.color,
  }));

  const cash = investable.reduce((sum, a) => sum + a.cashValue, 0);
  const invested = investable.reduce((sum, a) => sum + (a.balance - a.cashValue), 0);

  return { netWorth: assets - debt, assets, debt, slices, taxBands, cash, invested };
}
