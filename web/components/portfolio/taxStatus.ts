import type { TagTone } from "@/components/ui";
import type { Account, TaxStatus } from "@/lib/types";

const LABELS: Record<TaxStatus, string> = {
  Taxable: "Taxable",
  TaxDeferred: "Tax-deferred",
  TaxFree: "Tax-free",
};

const TONES: Record<TaxStatus, TagTone> = {
  Taxable: "neutral",
  TaxDeferred: "accent",
  TaxFree: "accent",
};

/** Display label + tag tone for an account's tax treatment. */
export function taxBadge(account: Account): { label: string; tone: TagTone } {
  if (!account.taxStatus) return { label: "n/a", tone: "outline" };
  return { label: LABELS[account.taxStatus], tone: TONES[account.taxStatus] };
}

export function contributionLimitLabel(account: Account): string {
  const limit = account.contributionLimit;
  if (!limit) return "—";
  const period = limit.period === "Yearly" ? "yr" : "mo";
  return `$${limit.amount.toLocaleString("en-US")} / ${period}`;
}
