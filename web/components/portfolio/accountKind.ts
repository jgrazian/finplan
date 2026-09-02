import type { Account, AccountFlavorKind, TaxStatus } from "@/lib/types";
import { fmtCurrency } from "@/lib/format";

/**
 * The five kinds the drawer is polymorphic over.
 *
 * The engine stores four flavors, not five: what the canvas calls Investment
 * and Retirement are the same `Investment` flavor under different tax
 * treatments, which is exactly the distinction that decides whether a sale
 * realises a gain — so the split earns its place rather than duplicating one.
 */
export type AccountKind = "investment" | "retirement" | "cash" | "property" | "debt";

export const KIND_LABEL: Record<AccountKind, string> = {
  investment: "Investment",
  retirement: "Retirement",
  cash: "Cash",
  property: "Property",
  debt: "Debt",
};

const KIND_FLAVOR: Record<AccountKind, AccountFlavorKind> = {
  investment: "Investment",
  retirement: "Investment",
  cash: "Bank",
  property: "Property",
  debt: "Liability",
};

/** Every kind, in the order the menu lists them. */
export const KINDS: readonly AccountKind[] = [
  "investment",
  "retirement",
  "cash",
  "property",
  "debt",
];

export function kindOf(account: Account): AccountKind {
  switch (account.flavor) {
    case "Bank":
      return "cash";
    case "Property":
      return "property";
    case "Liability":
      return "debt";
    case "Investment":
      return account.taxStatus === "Taxable" ? "investment" : "retirement";
  }
}

/** The tax treatment a kind implies when it is chosen, before refinement. */
export function taxStatusFor(kind: AccountKind, current: TaxStatus | undefined): TaxStatus | undefined {
  if (kind === "investment") return "Taxable";
  if (kind === "retirement") {
    // Traditional unless it already was a Roth — the refinement in Terms keeps
    // whichever the account had.
    return current === "TaxFree" ? "TaxFree" : "TaxDeferred";
  }
  return undefined;
}

/**
 * Whether a kind can be reached from this account.
 *
 * The server refuses a flavor change outright — turning a 401(k) into a
 * mortgage would invalidate every event and position pointing at it — so only
 * the kinds sharing this account's flavor are live, and the rest are shown
 * locked rather than hidden: a menu that silently omits three of five kinds
 * does not explain itself.
 */
export function isReachable(account: Account, kind: AccountKind): boolean {
  return KIND_FLAVOR[kind] === account.flavor;
}

/**
 * What a conversion drops, said before it commits.
 *
 * Only Investment ↔ Retirement is reachable, and each direction abandons
 * something: a contribution limit the engine stops enforcing, or a cost basis
 * it stops reading.
 */
export function conversionWarning(account: Account, to: AccountKind): string | undefined {
  const from = kindOf(account);
  if (from === to) return undefined;
  const lots = account.positions.length;
  const clause = (n: number) => `${n} lot${n === 1 ? "" : "s"}`;

  if (from === "retirement" && to === "investment") {
    const parts: string[] = [];
    if (account.contributionLimit) {
      parts.push(`clears the ${fmtCurrency(account.contributionLimit.amount)} contribution limit`);
    }
    if (lots > 0) parts.push(`starts taxing gains on ${clause(lots)} at sale`);
    if (parts.length === 0) return undefined;
    return `Retirement → Investment ${parts.join(", and ")}.`;
  }

  if (from === "investment" && to === "retirement") {
    if (lots === 0) return undefined;
    return `Investment → Retirement stops the cost basis on ${clause(
      lots,
    )} being read — gains are not taxed on sale there.`;
  }

  return undefined;
}
