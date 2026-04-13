"use client";

import { useEffect, useState } from "react";
import type { AllocationData } from "@/lib/types";
import * as api from "@/lib/api";
import { formatCurrency } from "@/lib/utils";
import { CATEGORY_COLORS } from "@/lib/utils";
import { AllocationPieChart } from "@/components/charts/AllocationPieChart";
import { AppShell } from "@/components/layout/AppShell";

export default function AllocationPage() {
  const [data, setData] = useState<AllocationData | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api
      .getAllocation()
      .then(setData)
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return <AppShell><div className="text-gray-500">Loading allocation data...</div></AppShell>;
  }

  if (!data || (data.by_account.length === 0 && data.by_asset.length === 0)) {
    return (
      <AppShell><div>
        <h1 className="text-2xl font-bold text-gray-900 mb-6">Allocation</h1>
        <div className="text-center py-12 text-gray-500">
          <p className="text-lg">No allocation data</p>
          <p className="mt-1 text-sm">
            Add accounts and holdings to see your portfolio allocation.
          </p>
        </div>
      </div></AppShell>
    );
  }

  return (
    <AppShell><div>
      <div className="mb-6">
        <h1 className="text-2xl font-bold text-gray-900">Allocation</h1>
        <p className="text-gray-500 mt-1">
          Net Worth:{" "}
          <span className="font-semibold text-gray-900">
            {formatCurrency(data.total_value)}
          </span>
        </p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 xl:grid-cols-3 gap-6">
        <AllocationPieChart
          title="By Category"
          data={data.by_category}
          colorMap={CATEGORY_COLORS}
        />
        <AllocationPieChart title="By Account" data={data.by_account} />
        <AllocationPieChart title="By Asset" data={data.by_asset} />
      </div>
    </div></AppShell>
  );
}
