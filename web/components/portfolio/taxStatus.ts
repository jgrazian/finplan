import type { TagTone } from "@/components/ui";
import type { Account, TaxStatus } from "@/lib/types";

const LABELS: Record<TaxStatus, string> = {
  Taxable: "Taxable",
  TaxDeferred: "Tax-deferred",
  TaxFree: "Tax-free",
};

/** Matches the portfolio strip's tax-treatment bands (`TAX_BANDS`). */
const TONES: Record<TaxStatus, TagTone> = {
  Taxable: "tax-taxable",
  TaxDeferred: "tax-deferred",
  TaxFree: "tax-free",
};

/** Display label + tag tone for an account's tax treatment. */
export function taxBadge(account: Account): { label: string; tone: TagTone } {
  if (!account.taxStatus) return { label: "n/a", tone: "quiet" };
  return { label: LABELS[account.taxStatus], tone: TONES[account.taxStatus] };
}

export function contributionLimitLabel(account: Account): string {
  const limit = account.contributionLimit;
  if (!limit) return "—";
  const period = limit.period === "Yearly" ? "yr" : "mo";
  const catchUp = limit.catchUp.length > 0 ? " + catch-up" : "";
  return `$${limit.amount.toLocaleString("en-US")} / ${period}${catchUp}`;
}
