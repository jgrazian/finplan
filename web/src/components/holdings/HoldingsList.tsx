"use client";

import { useState } from "react";
import type { Holding } from "@/lib/types";
import { formatCurrency } from "@/lib/utils";
import { HoldingForm } from "./HoldingForm";
import * as api from "@/lib/api";

interface HoldingsListProps {
  accountId: number;
  holdings: Holding[];
  onUpdate: () => void;
}

export function HoldingsList({
  accountId,
  holdings,
  onUpdate,
}: HoldingsListProps) {
  const [adding, setAdding] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);

  const total = holdings.reduce((sum, h) => sum + h.value, 0);

  async function handleCreate(data: { asset_name: string; value: number }) {
    await api.createHolding(accountId, data);
    setAdding(false);
    onUpdate();
  }

  async function handleUpdate(
    holdingId: number,
    data: { asset_name?: string; value?: number }
  ) {
    await api.updateHolding(holdingId, data);
    setEditingId(null);
    onUpdate();
  }

  async function handleDelete(holdingId: number) {
    await api.deleteHolding(holdingId);
    onUpdate();
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-lg font-semibold text-gray-700">Holdings</h3>
        {!adding && (
          <button
            onClick={() => setAdding(true)}
            className="px-3 py-1.5 text-sm bg-blue-600 text-white rounded-md hover:bg-blue-700 transition-colors"
          >
            Add Holding
          </button>
        )}
      </div>

      {adding && (
        <div className="mb-4 p-4 bg-gray-50 border border-gray-200 rounded-lg">
          <HoldingForm
            onSubmit={handleCreate}
            onCancel={() => setAdding(false)}
          />
        </div>
      )}

      {holdings.length === 0 && !adding ? (
        <p className="text-gray-500 text-sm">
          No holdings yet. Add assets to this account.
        </p>
      ) : (
        <div className="space-y-2">
          {holdings.map((holding) =>
            editingId === holding.id ? (
              <div
                key={holding.id}
                className="p-4 bg-gray-50 border border-gray-200 rounded-lg"
              >
                <HoldingForm
                  initial={holding}
                  onSubmit={(data) => handleUpdate(holding.id, data)}
                  onCancel={() => setEditingId(null)}
                />
              </div>
            ) : (
              <div
                key={holding.id}
                className="flex items-center justify-between p-3 bg-white border border-gray-200 rounded-lg"
              >
                <div>
                  <span className="font-medium text-gray-900">
                    {holding.asset_name}
                  </span>
                  {total > 0 && (
                    <span className="ml-2 text-xs text-gray-500">
                      {((holding.value / total) * 100).toFixed(1)}%
                    </span>
                  )}
                </div>
                <div className="flex items-center gap-3">
                  <span className="font-semibold text-gray-900">
                    {formatCurrency(holding.value)}
                  </span>
                  <button
                    onClick={() => setEditingId(holding.id)}
                    className="text-sm text-blue-600 hover:text-blue-800"
                  >
                    Edit
                  </button>
                  <button
                    onClick={() => handleDelete(holding.id)}
                    className="text-sm text-red-600 hover:text-red-800"
                  >
                    Delete
                  </button>
                </div>
              </div>
            )
          )}
        </div>
      )}
    </div>
  );
}
