"use client";

import Link from "next/link";
import type { Account } from "@/lib/types";
import { ACCOUNT_TYPE_DISPLAY } from "@/lib/types";
import { formatCurrency } from "@/lib/utils";

export function AccountCard({ account }: { account: Account }) {
  return (
    <Link
      href={`/accounts/${account.id}`}
      className="block p-4 bg-white border border-gray-200 rounded-lg hover:border-blue-300 hover:shadow-sm transition-all"
    >
      <div className="flex items-center justify-between">
        <div className="min-w-0 flex-1">
          <h3 className="font-medium text-gray-900 truncate">{account.name}</h3>
          <span className="inline-block mt-1 px-2 py-0.5 text-xs rounded-full bg-gray-100 text-gray-600">
            {ACCOUNT_TYPE_DISPLAY[account.account_type]}
          </span>
        </div>
        <div className="text-right ml-4">
          <span
            className={`text-lg font-semibold ${
              account.total_value < 0 ? "text-red-600" : "text-gray-900"
            }`}
          >
            {formatCurrency(account.total_value)}
          </span>
        </div>
      </div>
    </Link>
  );
}
