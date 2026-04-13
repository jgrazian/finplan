"use client";

import type { Account, AccountCategory } from "@/lib/types";
import { AccountCard } from "./AccountCard";
import { formatCurrency } from "@/lib/utils";

const CATEGORIES: { key: AccountCategory; label: string }[] = [
  { key: "Investment", label: "Investment" },
  { key: "Cash", label: "Cash / Property" },
  { key: "Debt", label: "Debt" },
];

export function AccountList({ accounts }: { accounts: Account[] }) {
  if (accounts.length === 0) {
    return (
      <div className="text-center py-12 text-gray-500">
        <p className="text-lg">No accounts yet</p>
        <p className="mt-1 text-sm">Create your first account to get started.</p>
      </div>
    );
  }

  return (
    <div className="space-y-8">
      {CATEGORIES.map(({ key, label }) => {
        const categoryAccounts = accounts.filter((a) => a.category === key);
        if (categoryAccounts.length === 0) return null;
        const categoryTotal = categoryAccounts.reduce(
          (sum, a) => sum + a.total_value,
          0
        );
        return (
          <section key={key}>
            <div className="flex items-center justify-between mb-3">
              <h2 className="text-lg font-semibold text-gray-700">{label}</h2>
              <span className="text-sm text-gray-500">
                {formatCurrency(categoryTotal)}
              </span>
            </div>
            <div className="space-y-2">
              {categoryAccounts.map((account) => (
                <AccountCard key={account.id} account={account} />
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );
}
