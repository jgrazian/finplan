"use client";

import type { DrawdownChoice, FundingPolicySpec } from "@/lib/api/types";
import {
  BRACKET_CEILINGS,
  ceilingOf,
  choiceKey,
  choiceLabel,
  isCurrentChoice,
  strategyDescription,
} from "@/lib/view/drawdown";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/**
 * The strategy row: pill chips and what the pick means. The selected Bracket
 * filling chip carries its ceiling inside the pill, so the two never wrap
 * apart. A select cannot sit inside a button, so that pill groups the two.
 */
export function StrategyChips({
  choices,
  selected,
  onSelect,
  planFunding,
  ceiling,
  onCeiling,
}: {
  choices: DrawdownChoice[];
  selected: number;
  onSelect: (key: string) => void;
  planFunding: FundingPolicySpec | null | undefined;
  ceiling: number;
  onCeiling: (ceiling: number) => void;
}) {
  const active = choices[selected]?.choice;
  return (
    <div style={{ display: "flex", flexWrap: "wrap", alignItems: "flex-start", gap: "8px 16px" }}>
      {/* Sized to its chips, so when the row is short the description wraps under
          them whole, rather than the chips wrapping inside their group. */}
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6, flex: "1 1 auto" }}>
        <span
          id="dd-strategy-label"
          style={{ fontSize: 11, letterSpacing: "0.06em", textTransform: "uppercase", color: MUTED, marginRight: 4 }}
        >
          Strategy
        </span>
        <div role="radiogroup" aria-labelledby="dd-strategy-label" style={{ display: "contents" }}>
          {choices.map((c, i) => {
            const on = i === selected;
            const withCeiling = on && ceilingOf(c.choice) != null;
            const chip = (
              <button
                key={choiceKey(c.choice)}
                type="button"
                role="radio"
                aria-checked={on}
                onClick={() => onSelect(choiceKey(c.choice))}
                style={{
                  cursor: "pointer",
                  font: "inherit",
                  fontSize: 13,
                  padding: withCeiling ? "4px 4px 4px 12px" : "4px 12px",
                  borderRadius: 999,
                  border: withCeiling
                    ? "1px solid transparent"
                    : "1px solid " + (on ? "var(--color-accent)" : "var(--color-divider)"),
                  background: withCeiling ? "transparent" : on ? "var(--color-accent)" : "var(--color-raised)",
                  color: on ? "var(--color-bg)" : "var(--color-text)",
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 6,
                  whiteSpace: "nowrap",
                }}
              >
                {choiceLabel(c.choice)}
                {isCurrentChoice(c.choice, planFunding) && (
                  <span style={{ fontSize: 10, opacity: 0.8, textTransform: "uppercase", letterSpacing: "0.05em" }}>
                    current
                  </span>
                )}
              </button>
            );
            if (!withCeiling) return chip;
            return (
              <span
                key={choiceKey(c.choice)}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  borderRadius: 999,
                  border: "1px solid var(--color-accent)",
                  background: "var(--color-accent)",
                  color: "var(--color-bg)",
                  paddingRight: 4,
                  whiteSpace: "nowrap",
                }}
              >
                {chip}
                <label style={{ display: "inline-flex", alignItems: "center", gap: 4, fontSize: 12 }}>
                  <span style={{ opacity: 0.8 }}>up to</span>
                  <select
                    aria-label="Bracket ceiling"
                    value={ceiling}
                    onChange={(e) => onCeiling(Number(e.target.value))}
                    style={{
                      font: "inherit",
                      fontSize: 12,
                      fontWeight: 600,
                      color: "inherit",
                      background: "color-mix(in srgb, var(--color-bg) 18%, transparent)",
                      border: "none",
                      borderRadius: 999,
                      padding: "2px 6px",
                      cursor: "pointer",
                    }}
                  >
                    {BRACKET_CEILINGS.map((b) => (
                      // The open list is drawn on the browser's own surface, not the pill.
                      <option key={b} value={b} style={{ color: "var(--color-text)", background: "var(--color-raised)" }}>
                        {Math.round(b * 100)}%
                      </option>
                    ))}
                  </select>
                </label>
              </span>
            );
          })}
        </div>
      </div>
      {active && (
        <p
          style={{
            margin: "0 0 0 auto",
            flex: "1 1 260px",
            maxWidth: 420,
            fontSize: 13,
            color: MUTED,
            textAlign: "right",
          }}
        >
          {strategyDescription(active)}
        </p>
      )}
    </div>
  );
}
