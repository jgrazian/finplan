/**
 * What guided setup and the example plan share: the default assumptions a
 * new plan starts from, its default name, and the year-of-birth shorthand.
 *
 * Plain imports only, so the Node test runner can load it.
 */
import type { SetupPlan } from "@/lib/api/generated/SetupPlan";
import type { Profile, TaxConfig } from "@/lib/api/types";
import { defaultTaxConfig } from "./assumptions.ts";
import { guardrails } from "./spending.ts";

/** The seeded return profiles a new plan starts from, by name; 0 where the library lacks one. */
export function defaultReturnProfileIds(profiles: readonly Profile[]) {
  const id = (name: string) => profiles.find((profile) => profile.name === name)?.id ?? 0;
  return { cash: id("Cash / T-Bills"), stocks: id("US Total Market"), bonds: id("US Aggregate Bonds") };
}

/** A name not yet taken: `base`, then `base 2`, `base 3`… Names are unique per account. */
export function freshName(taken: readonly string[], base = "My plan"): string {
  const names = new Set(taken.map((name) => name.trim().toLowerCase()));
  for (let n = 1; ; n++) {
    const name = n === 1 ? base : `${base} ${n}`;
    if (!names.has(name.toLowerCase())) return name;
  }
}

/**
 * The day a plan counts age from, given only a year and maybe a month. Without
 * a month it is mid-year, which halves the worst case of guessing January.
 * `exact` is a full date already known (the account's), kept when it agrees.
 */
export function birthDateOf(year: number, month: number | null, exact?: string | null): string {
  if (!Number.isInteger(year) || year < 1900 || year > 9999) return "";
  if (month == null) return `${year}-07-01`;
  const prefix = `${year}-${String(month).padStart(2, "0")}`;
  return exact?.startsWith(`${prefix}-`) ? exact : `${prefix}-01`;
}

/** The year and month a birth date names, for the inputs that edit it. */
export function birthParts(date: string | null | undefined): { year: number; month: number | null } {
  const match = /^(\d{4})-(\d{2})-\d{2}$/.exec(date ?? "");
  return match ? { year: Number(match[1]), month: Number(match[2]) } : { year: 0, month: null };
}

/**
 * The example plan's household. Invented, round numbers: a 40-year-old
 * saving into a 401(k), planning to retire at 65. Never anyone's real plan.
 */
export const EXAMPLE = {
  age: 40,
  retirementAge: 65,
  years: 50,
  cash: 20_000,
  savings: 30_000,
  retirement401k: 120_000,
  investments: 60_000,
  salary: 95_000,
  contributionPercent: 8,
  monthlySpending: 4_500,
  retirementSpendingPercent: 90,
  stockPercent: 70,
} as const;

/** The example household as setup answers, against one home's library. */
export function examplePlan({
  name,
  returnProfiles,
  inflationProfiles,
  taxConfigs,
  today = new Date(),
}: {
  name: string;
  returnProfiles: readonly Profile[];
  inflationProfiles: readonly Profile[];
  taxConfigs: TaxConfig[];
  today?: Date;
}): SetupPlan {
  const ids = defaultReturnProfileIds(returnProfiles);
  // A library missing a seeded profile still has some profile to stand in.
  const fallback = returnProfiles[0]?.id;
  if (fallback == null) throw new Error("The example needs a return profile in your library.");
  const start = today.toISOString().slice(0, 10);
  const retirementSpending = Math.round(
    (EXAMPLE.monthlySpending * 12 * EXAMPLE.retirementSpendingPercent) / 100,
  );
  return {
    request_id: crypto.randomUUID(),
    name,
    start_date: start,
    birth_date: birthDateOf(today.getUTCFullYear() - EXAMPLE.age, null),
    duration_years: EXAMPLE.years,
    retirement_age: EXAMPLE.retirementAge,
    cash: EXAMPLE.cash,
    high_yield_savings: EXAMPLE.savings,
    savings_profile_id: ids.cash || fallback,
    retirement_401k: EXAMPLE.retirement401k,
    investments: EXAMPLE.investments,
    stock_percent: EXAMPLE.stockPercent,
    cash_profile_id: ids.cash || fallback,
    stock_profile_id: ids.stocks || fallback,
    bond_profile_id: ids.bonds || fallback,
    investment_tax_status: "Taxable",
    annual_income: EXAMPLE.salary,
    retirement_401k_contribution_percent: EXAMPLE.contributionPercent,
    annual_spending: EXAMPLE.monthlySpending * 12,
    retirement_spending: retirementSpending,
    retirement_spending_rule: guardrails(retirementSpending),
    inflation_profile_id: inflationProfiles[0]?.id ?? null,
    tax_config_id: defaultTaxConfig(taxConfigs)?.id ?? null,
    fund_from_investments: true,
    assumptions_confirmed: true,
  };
}
