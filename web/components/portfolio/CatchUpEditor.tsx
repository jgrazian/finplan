"use client";

import { Button, CurrencyInput, NumberInput } from "@/components/ui";
import type { CatchUpSpec, ContributionPeriod } from "@/lib/api/types";

const MUTED = "color-mix(in srgb, var(--color-text) 55%, transparent)";

const ROW = {
  display: "grid",
  gridTemplateColumns: "minmax(0, 0.8fr) minmax(0, 0.8fr) minmax(0, 1.2fr) auto",
  gap: 8,
  alignItems: "center",
} as const;

/**
 * The age tiers of extra room on top of a contribution limit.
 *
 * One row per tier: first age, last age (blank for no upper bound) and the
 * extra amount. Ages count as of December 31, so a tier from 50 opens in the
 * year of the 50th birthday. Where tiers overlap the larger amount applies,
 * not the sum — which is how a 401(k)'s 60–63 catch-up replaces its age-50 one.
 */
export function CatchUpEditor({
  tiers,
  period,
  readOnly,
  onChange,
}: {
  tiers: CatchUpSpec[];
  period: ContributionPeriod;
  readOnly?: boolean;
  onChange: (tiers: CatchUpSpec[]) => void;
}) {
  const update = (i: number, patch: Partial<CatchUpSpec>) =>
    onChange(tiers.map((tier, j) => (j === i ? { ...tier, ...patch } : tier)));
  const per = period === "Yearly" ? "yr" : "mo";

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      {tiers.length > 0 && (
        <div style={{ ...ROW, fontSize: 11, color: MUTED }}>
          <span>From age</span>
          <span>Through age</span>
          <span>Extra</span>
          <span />
        </div>
      )}
      {tiers.map((tier, i) => (
        <div key={i} style={ROW}>
          <NumberInput
            style={{ minHeight: 32 }}
            value={tier.from_age}
            decimals={0}
            group={false}
            min={0}
            max={120}
            readOnly={readOnly}
            onValueChange={(age) => update(i, { from_age: clampAge(age) })}
            aria-label={`Catch-up ${i + 1} first age`}
          />
          <NumberInput
            style={{ minHeight: 32 }}
            nullable
            value={tier.through_age}
            decimals={0}
            group={false}
            min={0}
            max={120}
            placeholder="no limit"
            readOnly={readOnly}
            onValueChange={(age) => update(i, { through_age: age == null ? null : clampAge(age) })}
            aria-label={`Catch-up ${i + 1} last age`}
          />
          <CurrencyInput
            style={{ minHeight: 32 }}
            value={tier.amount}
            min={0}
            suffix={`/ ${per}`}
            readOnly={readOnly}
            onValueChange={(amount) => update(i, { amount: Math.max(0, amount) })}
            aria-label={`Catch-up ${i + 1} extra amount`}
          />
          <button
            type="button"
            disabled={readOnly}
            onClick={() => onChange(tiers.filter((_, j) => j !== i))}
            aria-label={`Remove catch-up ${i + 1}`}
            title="Remove this catch-up"
            style={{
              border: 0,
              background: "none",
              padding: "0 2px",
              fontSize: 15,
              lineHeight: 1,
              color: MUTED,
              cursor: "pointer",
            }}
          >
            ×
          </button>
        </div>
      ))}
      {tiers.some((tier) => tier.through_age != null && tier.through_age < tier.from_age) && (
        <p style={{ margin: 0, fontSize: 11.5, color: "var(--color-accent-900)" }}>
          A catch-up cannot end before it starts.
        </p>
      )}
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <Button
          variant="add"
          disabled={readOnly}
          onClick={() =>
            onChange([...tiers, { from_age: 50, through_age: null, amount: 0 }])
          }
        >
          Add catch-up
        </Button>
        <span style={{ fontSize: 11, color: MUTED }}>
          By age at year end · overlapping tiers take the larger, not the sum
        </span>
      </div>
    </div>
  );
}

function clampAge(age: number): number {
  return Math.min(120, Math.max(0, Math.round(age)));
}
