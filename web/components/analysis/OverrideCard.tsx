"use client";

import { Blueprint } from "@/components/ui";
import type { LayerSlot, LayerView } from "@/lib/view/whatIf";

const MUTED = "color-mix(in srgb, var(--color-text) 55%, transparent)";

/**
 * One override layer: a switch, what kind it is, what it costs or buys, and
 * the sentence that says what it does with its values steppable in place.
 */
export function OverrideCard({
  view,
  impact,
  disabled,
  onToggle,
  onStep,
  onRemove,
}: {
  view: LayerView;
  /** `+2.1 pts`, `off`, `…` while it is being measured. */
  impact: string;
  disabled?: boolean;
  onToggle: () => void;
  onStep: (slot: LayerSlot, dir: 1 | -1) => void;
  onRemove: () => void;
}) {
  const off = !view.enabled || view.problem != null;
  return (
    <Blueprint
      style={{
        padding: "10px 12px",
        display: "flex",
        flexDirection: "column",
        gap: 8,
        opacity: off ? 0.5 : 1,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <input
          type="checkbox"
          checked={view.enabled}
          disabled={disabled}
          onChange={onToggle}
          aria-label={`Apply ${view.short}`}
          style={{ accentColor: "var(--color-accent)", margin: 0, cursor: "pointer" }}
        />
        <span className="stat-l">{view.kind}</span>
        <span
          style={{
            marginLeft: "auto",
            fontFamily: "var(--font-heading)",
            fontWeight: 600,
            fontSize: 15,
            color: off ? MUTED : "var(--color-accent-800)",
          }}
        >
          {impact}
        </span>
        <button
          type="button"
          onClick={onRemove}
          disabled={disabled}
          aria-label={`Remove ${view.short}`}
          title="Remove this override"
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
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          fontSize: 14,
          flexWrap: "wrap",
        }}
      >
        {view.parts.map((part, i) =>
          part.type === "text" ? (
            <span key={i} style={part.muted ? { fontSize: 12, color: MUTED } : undefined}>
              {part.text}
            </span>
          ) : (
            <span key={i} className="wi-step">
              <button
                type="button"
                aria-label={`Less ${part.ariaLabel}`}
                disabled={disabled || !part.canDec}
                onClick={() => onStep(part.slot, -1)}
              >
                −
              </button>
              <output aria-live="polite">{part.value}</output>
              <button
                type="button"
                aria-label={`More ${part.ariaLabel}`}
                disabled={disabled || !part.canInc}
                onClick={() => onStep(part.slot, 1)}
              >
                +
              </button>
            </span>
          ),
        )}
      </div>
      {view.problem && <span style={{ fontSize: 11.5, color: MUTED }}>{view.problem}</span>}
    </Blueprint>
  );
}
