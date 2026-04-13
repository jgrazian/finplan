"use client";

import { useCallback, useEffect, useState } from "react";
import type { Account } from "@/lib/types";
import * as api from "@/lib/api";
import { AccountList } from "@/components/accounts/AccountList";
import { AccountForm } from "@/components/accounts/AccountForm";
import { AppShell } from "@/components/layout/AppShell";
import { formatCurrency } from "@/lib/utils";

export default function AccountsPage() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [loading, setLoading] = useState(true);
  const [showForm, setShowForm] = useState(false);

  const loadAccounts = useCallback(async () => {
    try {
      const data = await api.getAccounts();
      setAccounts(data);
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
    </div></AppShell>
  );
}
