/**
 * `api::runs::LedgerPage` → the entries the cash-flow table nests under an
 * expanded year.
 *
 * The server keeps the ledger's prose free of dollar figures precisely so this
 * step can restate the numbers without rewriting the sentence around them.
 */
import type { LedgerPage } from "@/lib/api/types";
import type { LedgerCategory, LedgerEntry } from "@/lib/types";
import { deflate } from "./results";

const CATEGORIES: readonly string[] = ["cash", "asset", "tax", "event"];

function asCategory(value: string): LedgerCategory {
  return (CATEGORIES.includes(value) ? value : "cash") as LedgerCategory;
}

/**
 * `factor` is the year's cumulative inflation. Every entry in one year shares
 * it, so a year's items still sum to the total on the row above them.
 */
export function toLedgerEntries(page: LedgerPage, factor: number): LedgerEntry[] {
  return page.entries.map((entry) => ({
    id: `${entry.year}-${entry.position}`,
    date: entry.date,
    category: asCategory(entry.category),
    kind: entry.kind,
    detail: entry.detail,
    amount: entry.amount == null ? undefined : deflate(entry.amount, factor),
    basis: entry.basis == null ? undefined : deflate(entry.basis, factor),
    basisLabel: entry.basis_label ?? undefined,
  }));
}
