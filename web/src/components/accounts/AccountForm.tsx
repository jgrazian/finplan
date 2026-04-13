"use client";

import { useState } from "react";
import type { Account, AccountCategory, AccountType } from "@/lib/types";
import {
  ACCOUNT_TYPE_DISPLAY,
  CATEGORY_TYPES,
} from "@/lib/types";

interface AccountFormProps {
  initial?: Account;
  onSubmit: (data: {
    name: string;
    description?: string;
    account_type: string;
    value?: number;
    balance?: number;
    interest_rate?: number;
  }) => Promise<void>;
  onCancel: () => void;
}

const CATEGORY_LABELS: Record<AccountCategory, string> = {
  Investment: "Investment",
  Cash: "Cash / Property",
  Debt: "Debt",
};

export function AccountForm({ initial, onSubmit, onCancel }: AccountFormProps) {
  const [category, setCategory] = useState<AccountCategory | null>(
    initial?.category ?? null
  );
  const [accountType, setAccountType] = useState<AccountType | null>(
    initial?.account_type ?? null
  );
  const [name, setName] = useState(initial?.name ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [value, setValue] = useState(initial?.value?.toString() ?? "");
  const [balance, setBalance] = useState(initial?.balance?.toString() ?? "");
  const [interestRate, setInterestRate] = useState(
    initial?.interest_rate?.toString() ?? ""
  );
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");

  const isEditing = !!initial;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!accountType || !name.trim()) return;

    setSubmitting(true);
    setError("");
    try {
      await onSubmit({
        name: name.trim(),
        description: description.trim() || undefined,
        account_type: accountType,
        value: value ? parseFloat(value) : undefined,
        balance: balance ? parseFloat(balance) : undefined,
        interest_rate: interestRate ? parseFloat(interestRate) : undefined,
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Something went wrong");
    } finally {
      setSubmitting(false);
    }
  }

  // Step 1: Pick category (only when creating)
  if (!isEditing && !category) {
    return (
      <div className="space-y-4">
        <h3 className="text-lg font-semibold">Select Account Category</h3>
        <div className="grid grid-cols-3 gap-3">
          {(Object.keys(CATEGORY_LABELS) as AccountCategory[]).map((cat) => (
            <button
              key={cat}
              onClick={() => setCategory(cat)}
              className="p-4 border border-gray-200 rounded-lg hover:border-blue-400 hover:bg-blue-50 transition-colors text-center"
            >
              <div className="font-medium">{CATEGORY_LABELS[cat]}</div>
            </button>
          ))}
        </div>
        <button
          onClick={onCancel}
          className="text-sm text-gray-500 hover:text-gray-700"
        >
          Cancel
        </button>
      </div>
    );
  }

  // Step 2: Pick type (only when creating)
  if (!isEditing && category && !accountType) {
    const types = CATEGORY_TYPES[category];
    return (
      <div className="space-y-4">
        <h3 className="text-lg font-semibold">
          Select {CATEGORY_LABELS[category]} Type
        </h3>
        <div className="grid grid-cols-2 gap-3">
          {types.map((type_) => (
            <button
              key={type_}
              onClick={() => setAccountType(type_)}
              className="p-3 border border-gray-200 rounded-lg hover:border-blue-400 hover:bg-blue-50 transition-colors text-left"
            >
              {ACCOUNT_TYPE_DISPLAY[type_]}
            </button>
          ))}
        </div>
        <button
          onClick={() => setCategory(null)}
          className="text-sm text-gray-500 hover:text-gray-700"
        >
          Back
        </button>
      </div>
    );
  }

  // Step 3: Form fields
  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <h3 className="text-lg font-semibold">
        {isEditing ? "Edit" : "New"}{" "}
        {accountType ? ACCOUNT_TYPE_DISPLAY[accountType] : "Account"}
      </h3>

      {error && (
        <div className="p-3 bg-red-50 border border-red-200 rounded text-red-700 text-sm">
          {error}
        </div>
      )}

      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Name
        </label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          required
        />
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Description
        </label>
        <input
          type="text"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
        />
      </div>

      {category === "Cash" && (
        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Value
          </label>
          <input
            type="number"
            step="0.01"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          />
        </div>
      )}

      {category === "Debt" && (
        <>
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Balance
            </label>
            <input
              type="number"
              step="0.01"
              value={balance}
              onChange={(e) => setBalance(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Interest Rate (%)
            </label>
            <input
              type="number"
              step="0.01"
              value={interestRate}
              onChange={(e) => setInterestRate(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            />
          </div>
        </>
      )}

      <div className="flex gap-3 pt-2">
        <button
          type="submit"
          disabled={submitting}
          className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 disabled:opacity-50 transition-colors"
        >
          {submitting ? "Saving..." : isEditing ? "Save" : "Create"}
        </button>
        <button
          type="button"
          onClick={isEditing ? onCancel : () => setAccountType(null)}
          className="px-4 py-2 border border-gray-300 rounded-md hover:bg-gray-50 transition-colors"
        >
          {isEditing ? "Cancel" : "Back"}
        </button>
      </div>
    </form>
  );
}
