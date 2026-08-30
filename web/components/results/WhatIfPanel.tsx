"use client";

import { Button, RangeField, SectionHeading } from "@/components/ui";

export interface WhatIfOverrides {
  retirementAge: number;
  /** Annual spending in thousands, matching the slider's unit. */
  annualSpendK: number;
}

/**
 * Overrides that re-run the plan without duplicating the scenario. Fully
 * controlled — the page owns the values so the chart can react live.
 */
export function WhatIfPanel({
  value,
  onChange,
  onRerun,
  onSaveVariant,
}: {
  value: WhatIfOverrides;
  onChange: (next: WhatIfOverrides) => void;
  onRerun?: () => void;
  onSaveVariant?: () => void;
}) {
  return (
    <div>
      <SectionHeading className="mb-[8px]">What if</SectionHeading>

      <RangeField
        className="mb-[12px]"
        label={`Retirement age — ${value.retirementAge}`}
        min={58}
        max={70}
        value={value.retirementAge}
        onValueChange={(retirementAge) => onChange({ ...value, retirementAge })}
      />
      <RangeField
        className="mb-[12px]"
        label={`Annual spending — $${value.annualSpendK},000`}
        min={90}
        max={220}
        step={5}
        value={value.annualSpendK}
        onValueChange={(annualSpendK) => onChange({ ...value, annualSpendK })}
      />

      <div
        style={{
          fontSize: 12,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
          marginBottom: 10,
        }}
      >
        Overrides the base scenario without duplicating it.
      </div>

      <Button block onClick={onRerun}>
        Re-run with overrides
      </Button>
      <Button variant="ghost" block onClick={onSaveVariant}>
        Save as variant…
      </Button>
    </div>
  );
}
