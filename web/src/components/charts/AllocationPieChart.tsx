"use client";

import {
  PieChart,
  Pie,
  Cell,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from "recharts";
import type { AllocationSlice } from "@/lib/types";
import { formatCurrency, formatPercent, getChartColor } from "@/lib/utils";

interface AllocationPieChartProps {
  title: string;
  data: AllocationSlice[];
  colorMap?: Record<string, string>;
}

export function AllocationPieChart({
  title,
  data,
  colorMap,
}: AllocationPieChartProps) {
  // Filter out negative values for pie chart (debts)
  const positiveData = data.filter((d) => d.value > 0);

  if (positiveData.length === 0) {
    return (
      <div className="bg-white border border-gray-200 rounded-lg p-6">
        <h3 className="text-lg font-semibold text-gray-700 mb-4">{title}</h3>
        <p className="text-gray-500 text-sm text-center py-8">No data to display</p>
      </div>
    );
  }

  return (
    <div className="bg-white border border-gray-200 rounded-lg p-6">
      <h3 className="text-lg font-semibold text-gray-700 mb-4">{title}</h3>
      <ResponsiveContainer width="100%" height={300}>
        <PieChart>
          <Pie
            data={positiveData}
            dataKey="value"
            nameKey="name"
            cx="50%"
            cy="50%"
            outerRadius={100}
            label={({ name, ...rest }) => {
              const pct = (rest as unknown as Record<string, number>).percentage ?? 0;
              return `${name} (${formatPercent(pct)})`;
            }}
          >
            {positiveData.map((entry, index) => (
              <Cell
                key={entry.name}
                fill={colorMap?.[entry.name] ?? getChartColor(index)}
              />
            ))}
          </Pie>
          <Tooltip
            formatter={(value) => formatCurrency(Number(value))}
          />
          <Legend />
        </PieChart>
      </ResponsiveContainer>

      {/* Show negative items (debts) as a note below */}
      {data.some((d) => d.value < 0) && (
        <div className="mt-4 pt-4 border-t border-gray-100">
          <p className="text-sm text-gray-500 font-medium">Liabilities:</p>
          {data
            .filter((d) => d.value < 0)
            .map((d) => (
              <p key={d.name} className="text-sm text-red-600">
                {d.name}: {formatCurrency(d.value)}
              </p>
            ))}
        </div>
      )}
    </div>
  );
}
