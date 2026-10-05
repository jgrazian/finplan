"use client";

import { SegmentedControl } from "@/components/ui";
import type { DrawdownBody } from "@/lib/api/types";
import {
  conversionKey,
  conversionOfKey,
  conversionOptions,
  type ConversionSetting,
} from "@/lib/view/drawdown";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/**
 * Roth conversions for every strategy at once (spec 21): the plan's own, none,
 * or filling to a bracket each year. Disabled, with the reason, when the plan
 * has no account to convert from or into.
 */
export function ConversionToggle({
  body,
  value,
  onChange,
}: {
  body: DrawdownBody;
  value: ConversionSetting;
  onChange: (setting: ConversionSetting) => void;
}) {
  const unavailable = body.conversions.unavailable;
  return (
    <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: "6px 10px" }}>
      <span
        style={{ fontSize: 11, letterSpacing: "0.06em", textTransform: "uppercase", color: MUTED }}
      >
        Roth conversions
      </span>
      <SegmentedControl
        ariaLabel="Roth conversions"
        options={conversionOptions(body)}
        value={conversionKey(value)}
        onChange={(key) => onChange(conversionOfKey(key))}
      />
      <span style={{ fontSize: 12.5, color: MUTED }}>
        {unavailable ??
          (value?.kind === "UpTo"
            ? "Each Dec 30, pre-tax money moves to the Roth up to the top of the bracket. Not spending: marked above the bars, its tax below."
            : null)}
      </span>
    </div>
  );
}
