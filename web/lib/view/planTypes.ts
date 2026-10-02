/**
 * Retirement plan types and the statutory limits each one starts with.
 *
 * Picking a plan fills in the account's contribution limit and its age
 * catch-ups; every figure stays editable afterwards. The engine reads only
 * those figures, never the plan type, so a plan whose limit was edited
 * simulates with the edit.
 *
 * Figures are the IRS's 2026 limits, the same table the review AI cites
 * (`suggest/ai/tools/facts.rs`). They are nominal: the engine does not index
 * them to inflation.
 */
import type { CatchUpSpec } from "../api/generated/CatchUpSpec.ts";
import type { PlanType } from "../api/generated/PlanType.ts";
import type { TaxStatus } from "../api/generated/TaxStatus.ts";

export const LIMITS_YEAR = 2026;

export interface PlanInfo {
  label: string;
  /** Plans that share these limits, for the menu's right-hand column. */
  detail?: string;
  taxStatus: Exclude<TaxStatus, "Taxable">;
  /** Yearly contribution limit. */
  limit: number;
  catchUp: CatchUpSpec[];
}

const DEFERRAL = 24_500;
const DEFERRAL_CATCH_UP: CatchUpSpec[] = [
  { from_age: 50, through_age: null, amount: 8_000 },
  // SECURE 2.0: a larger amount at 60 to 63, in place of the one at 50.
  { from_age: 60, through_age: 63, amount: 11_250 },
];
const IRA = 7_500;
const IRA_CATCH_UP: CatchUpSpec[] = [{ from_age: 50, through_age: null, amount: 1_100 }];

export const PLANS: Record<PlanType, PlanInfo> = {
  Traditional401k: {
    label: "Traditional 401(k)",
    detail: "403(b) · 457(b) · TSP",
    taxStatus: "TaxDeferred",
    limit: DEFERRAL,
    catchUp: DEFERRAL_CATCH_UP,
  },
  Roth401k: {
    label: "Roth 401(k)",
    detail: "Roth 403(b) · 457(b)",
    taxStatus: "TaxFree",
    limit: DEFERRAL,
    catchUp: DEFERRAL_CATCH_UP,
  },
  TraditionalIra: {
    label: "Traditional IRA",
    taxStatus: "TaxDeferred",
    limit: IRA,
    catchUp: IRA_CATCH_UP,
  },
  RothIra: {
    label: "Roth IRA",
    taxStatus: "TaxFree",
    limit: IRA,
    catchUp: IRA_CATCH_UP,
  },
  Hsa: {
    label: "HSA",
    // Self-only coverage; family coverage is $8,750.
    detail: "self-only",
    taxStatus: "TaxFree",
    limit: 4_400,
    catchUp: [{ from_age: 55, through_age: null, amount: 1_000 }],
  },
};

export const PLAN_TYPES = Object.keys(PLANS) as PlanType[];

/**
 * One menu value for both halves of a retirement account's treatment: a named
 * plan, which implies its tax status, or "other" with the status spelled out.
 */
export type PlanChoice = PlanType | "OtherTaxDeferred" | "OtherTaxFree";

export function planChoiceOf(
  plan: PlanType | null | undefined,
  taxStatus: TaxStatus | undefined,
): PlanChoice {
  if (plan) return plan;
  return taxStatus === "TaxFree" ? "OtherTaxFree" : "OtherTaxDeferred";
}

export const PLAN_CHOICES: ReadonlyArray<{ value: PlanChoice; label: string; detail?: string }> = [
  ...PLAN_TYPES.map((plan) => ({
    value: plan,
    label: PLANS[plan].label,
    detail: PLANS[plan].detail,
  })),
  { value: "OtherTaxDeferred", label: "Other plan", detail: "tax-deferred" },
  { value: "OtherTaxFree", label: "Other plan", detail: "tax-free" },
];

/** What a choice sets: the plan, its tax status, and — for a named plan — its limits. */
export function applyPlanChoice(choice: PlanChoice): {
  planType: PlanType | null;
  taxStatus: Exclude<TaxStatus, "Taxable">;
  defaults?: { limit: number; catchUp: CatchUpSpec[] };
} {
  if (choice === "OtherTaxDeferred") return { planType: null, taxStatus: "TaxDeferred" };
  if (choice === "OtherTaxFree") return { planType: null, taxStatus: "TaxFree" };
  const plan = PLANS[choice];
  return {
    planType: choice,
    taxStatus: plan.taxStatus,
    defaults: { limit: plan.limit, catchUp: plan.catchUp.map((tier) => ({ ...tier })) },
  };
}

function dollars(n: number): string {
  return `$${n.toLocaleString("en-US", { maximumFractionDigits: 2 })}`;
}

/** `50+` or `60–63`. */
export function ageBand(tier: CatchUpSpec): string {
  return tier.through_age == null ? `${tier.from_age}+` : `${tier.from_age}–${tier.through_age}`;
}

/** `+$8,000 at 50+ · +$11,250 at 60–63`, or empty when there are none. */
export function catchUpLabel(tiers: readonly CatchUpSpec[]): string {
  return tiers.map((tier) => `+${dollars(tier.amount)} at ${ageBand(tier)}`).join(" · ");
}

export function sameCatchUp(a: readonly CatchUpSpec[], b: readonly CatchUpSpec[]): boolean {
  return (
    a.length === b.length &&
    a.every(
      (tier, i) =>
        tier.from_age === b[i].from_age &&
        tier.through_age === b[i].through_age &&
        tier.amount === b[i].amount,
    )
  );
}
