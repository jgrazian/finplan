"use client";

import { DateInput, NumberInput } from "@/components/ui";
import { parameterDate, parameterDay } from "@/lib/view/analysis";

/** Editable bounds use the same units as the parameter editor. */
export function ParameterRangeInput({ kind, label, value, onCommit, disabled }: {
  kind: string;
  label: string;
  value: number;
  onCommit: (value: number) => void;
  disabled?: boolean;
}) {
  if (kind === "date") {
    return <DateInput ariaLabel={label} value={parameterDate(value)} disabled={disabled}
      onValueChange={(date) => { const day = parameterDay(date); if (Number.isFinite(day)) onCommit(day); }}
      style={{ width: 154, minHeight: 28 }} />;
  }
  if (kind === "age") {
    const months = Math.round(value * 12);
    return <span style={{ display: "inline-flex", gap: 4 }}>
      <NumberInput aria-label={`${label} years`} value={Math.floor(months / 12)} min={0} max={255}
        suffix="yr" decimals={0} disabled={disabled} style={{ width: 68, minHeight: 28 }}
        onCommit={(years) => onCommit(Math.round(years) + (months % 12) / 12)} />
      <NumberInput aria-label={`${label} months`} value={months % 12} min={0} max={11}
        suffix="mo" decimals={0} disabled={disabled} style={{ width: 62, minHeight: 28 }}
        onCommit={(extra) => onCommit(Math.floor(months / 12) + Math.round(extra) / 12)} />
    </span>;
  }
  const rate = kind === "rate";
  return <NumberInput aria-label={label} value={rate ? Number((value * 100).toFixed(8)) : value}
    onCommit={(next) => onCommit(rate ? next / 100 : next)} decimals={rate ? 6 : 2}
    allowNegative disabled={disabled} prefix={kind === "amount" ? "$" : undefined}
    suffix={rate ? "%" : undefined} style={{ width: 100, minHeight: 28 }} />;
}
