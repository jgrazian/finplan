"use client";

import { useCallback, useEffect, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import type { Account } from "@/lib/types";
import { ACCOUNT_TYPE_DISPLAY } from "@/lib/types";
import * as api from "@/lib/api";
import { formatCurrency } from "@/lib/utils";
import { AccountForm } from "@/components/accounts/AccountForm";
import { DeleteDialog } from "@/components/accounts/DeleteDialog";
import { HoldingsList } from "@/components/holdings/HoldingsList";
import { AppShell } from "@/components/layout/AppShell";

export default function AccountDetailPage() {
  const params = useParams();
  const router = useRouter();
  const id = Number(params.id);

  const [account, setAccount] = useState<Account | null>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState(false);
  const [showDelete, setShowDelete] = useState(false);

  const loadAccount = useCallback(async () => {
    try {
      const data = await api.getAccount(id);
      setAccount(data);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    loadAccount();
  }, [loadAccount]);

  async function handleUpdate(data: {
    name: string;
    description?: string;
    account_type: string;
    value?: number;
    balance?: number;
    interest_rate?: number;
  }) {
    await api.updateAccount(id, data);
    setEditing(false);
    await loadAccount();
  }

  async function handleDelete() {
    await api.deleteAccount(id);
    router.push("/accounts");
  }

  if (loading) {
    return <AppShell><div className="text-gray-500">Loading...</div></AppShell>;
  }

  if (!account) {
    return <AppShell><div className="text-red-600">Account not found</div></AppShell>;
  }

  if (editing) {
    return (
      <AppShell><div className="max-w-lg">
        <AccountForm
          initial={account}
          onSubmit={handleUpdate}
          onCancel={() => setEditing(false)}
        />
      </div></AppShell>
    );
  }

  return (
    <AppShell><div>
      {showDelete && (
        <DeleteDialog
          name={account.name}
          onConfirm={handleDelete}
          onCancel={() => setShowDelete(false)}
        />
      )}

      <div className="mb-6">
        <button
          onClick={() => router.push("/accounts")}
          className="text-sm text-blue-600 hover:text-blue-800 mb-4 inline-block"
        >
          &larr; Back to Accounts
        </button>

        <div className="flex items-start justify-between">
          <div>
            <h1 className="text-2xl font-bold text-gray-900">{account.name}</h1>
            <div className="flex items-center gap-3 mt-1">
              <span className="px-2 py-0.5 text-xs rounded-full bg-gray-100 text-gray-600">
                {ACCOUNT_TYPE_DISPLAY[account.account_type]}
              </span>
              <span className="text-gray-500">{account.category}</span>
            </div>
            {account.description && (
              <p className="mt-2 text-gray-600">{account.description}</p>
            )}
          </div>
          <div className="flex gap-2">
            <button
              onClick={() => setEditing(true)}
              className="px-3 py-1.5 text-sm border border-gray-300 rounded-md hover:bg-gray-50 transition-colors"
            >
              Edit
            </button>
            <button
              onClick={() => setShowDelete(true)}
              className="px-3 py-1.5 text-sm text-red-600 border border-red-200 rounded-md hover:bg-red-50 transition-colors"
            >
              Delete
            </button>
          </div>
        </div>
      </div>

      {/* Account details */}
      <div className="bg-white border border-gray-200 rounded-lg p-6 mb-6">
        <div className="grid grid-cols-2 gap-4">
          <div>
            <span className="text-sm text-gray-500">Total Value</span>
            <p
              className={`text-2xl font-bold ${
                account.total_value < 0 ? "text-red-600" : "text-gray-900"
              }`}
            >
              {formatCurrency(account.total_value)}
            </p>
          </div>

          {account.category === "Cash" && account.value !== undefined && (
            <div>
              <span className="text-sm text-gray-500">Balance</span>
              <p className="text-lg font-semibold text-gray-900">
                {formatCurrency(account.value)}
              </p>
            </div>
          )}

          {account.category === "Debt" && (
            <>
              {account.balance !== undefined && (
                <div>
                  <span className="text-sm text-gray-500">Balance Owed</span>
                  <p className="text-lg font-semibold text-red-600">
                    {formatCurrency(account.balance)}
                  </p>
                </div>
              )}
              {account.interest_rate !== undefined && (
                <div>
                  <span className="text-sm text-gray-500">Interest Rate</span>
                  <p className="text-lg font-semibold text-gray-900">
                    {account.interest_rate}%
                  </p>
                </div>
              )}
            </>
          )}
        </div>
      </div>

      {/* Holdings for investment accounts */}
      {account.category === "Investment" && account.holdings && (
        <HoldingsList
          accountId={account.id}
          holdings={account.holdings}
          onUpdate={loadAccount}
        />
      )}
    </div></AppShell>
  );
}
