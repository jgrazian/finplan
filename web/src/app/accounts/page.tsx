"use client";

import { useCallback, useEffect, useState } from "react";
import type { Account, AllocationData } from "@/lib/types";
import * as api from "@/lib/api";
import { AccountList } from "@/components/accounts/AccountList";
import { AccountForm } from "@/components/accounts/AccountForm";
import { AppShell } from "@/components/layout/AppShell";
import { AllocationPieChart } from "@/components/charts/AllocationPieChart";
import { formatCurrency, CATEGORY_COLORS } from "@/lib/utils";

export default function AccountsPage() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [allocation, setAllocation] = useState<AllocationData | null>(null);
  const [loading, setLoading] = useState(true);
  const [showForm, setShowForm] = useState(false);

  const loadAccounts = useCallback(async () => {
    try {
      const [accountsData, allocationData] = await Promise.all([
        api.getAccounts(),
        api.getAllocation(),
      ]);
      setAccounts(accountsData);
      setAllocation(allocationData);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadAccounts();
  }, [loadAccounts]);

  async function handleCreate(data: {
    name: string;
    description?: string;
    account_type: string;
    value?: number;
    balance?: number;
    interest_rate?: number;
  }) {
    await api.createAccount(data);
    setShowForm(false);
    await loadAccounts();
  }

  const netWorth = accounts.reduce((sum, a) => sum + a.total_value, 0);

  if (loading) {
    return <AppShell><div className="text-gray-500">Loading accounts...</div></AppShell>;
  }

  return (
    <AppShell><div>
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold text-gray-900">Accounts</h1>
          {accounts.length > 0 && (
            <p className="text-gray-500 mt-1">
              Net Worth: <span className="font-semibold text-gray-900">{formatCurrency(netWorth)}</span>
            </p>
          )}
        </div>
        {!showForm && (
          <button
            onClick={() => setShowForm(true)}
            className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 transition-colors"
          >
            Add Account
          </button>
        )}
      </div>

      {showForm && (
        <div className="mb-6 p-6 bg-white border border-gray-200 rounded-lg">
          <AccountForm
            onSubmit={handleCreate}
            onCancel={() => setShowForm(false)}
          />
        </div>
      )}

      <AccountList accounts={accounts} />

      {allocation &&
        (allocation.by_account.length > 0 || allocation.by_asset.length > 0) && (
          <div className="mt-10">
            <h2 className="text-xl font-semibold text-gray-900 mb-4">
              Allocation
            </h2>
            <div className="grid grid-cols-1 lg:grid-cols-2 xl:grid-cols-3 gap-6">
              <AllocationPieChart
                title="By Category"
                data={allocation.by_category}
                colorMap={CATEGORY_COLORS}
              />
              <AllocationPieChart
                title="By Account"
                data={allocation.by_account}
              />
              <AllocationPieChart
                title="By Asset"
                data={allocation.by_asset}
              />
            </div>
          </div>
        )}
    </div></AppShell>
  );
}
