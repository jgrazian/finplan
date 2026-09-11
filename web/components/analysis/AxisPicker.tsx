"use client";

import { Blueprint, Dropdown, NumberInput, StatLabel } from "@/components/ui";
import type { AnalysisParameter } from "@/lib/api/types";
import { paramId } from "@/lib/view/analysis";

/** An axis as the screen holds it: which parameter, over what, in how many steps. */
export interface Axis {
  parameterId: string;
  min: number;
  max: number;
  steps: number;
}

/** An axis seeded from a parameter's own suggested range. */
export function axisFor(parameter: AnalysisParameter, steps = 6): Axis {
  return { parameterId: parameter.id, min: parameter.min, max: parameter.max, steps };
}

/**
 * One swept parameter and its range, as a labelled block in the toolbar.
 *
 * The range is editable in place rather than behind a dialog: changing where an
 * axis starts is the second thing anyone does after choosing it, and it is the
 * one edit that needs another run.
 */
export function AxisPicker({
  slot,
  parameters,
  value,
  onChange,
  /** Parameters already spoken for by another axis. */
  taken = [],
  showSteps = true,
  disabled,
}: {
  /** `x`, `y`, or a word — whatever names this axis on screen. */
  slot: string;
  parameters: AnalysisParameter[];
  value: Axis;
  onChange: (next: Axis) => void;
  taken?: string[];
  /** Off for a solve, which chooses its own probes. */
  showSteps?: boolean;
  disabled?: boolean;
}) {
  const selected = parameters.find((p) => p.id === value.parameterId);
  const money = selected?.kind === "amount";

  return (
    <Blueprint
      style={{
        padding: "6px 10px",
        display: "flex",
        gap: 8,
        alignItems: "center",
        flexWrap: "wrap",
      }}
    >
      <StatLabel>{slot}</StatLabel>
      <Dropdown
        inline
        ariaLabel={`${slot} axis parameter`}
        disabled={disabled}
        options={parameters.map((p) => ({
          value: p.id,
          label: paramId(p),
          detail: p.event_name,
          disabled: p.id !== value.parameterId && taken.includes(p.id),
        }))}
        value={value.parameterId}
        onChange={(parameterId) => {
          // A new parameter brings its own range: the previous one's bounds
          // are meaningless on a different quantity, and silently keeping
          // them is how a sweep ends up over ages 2,500 to 10,000.
          const next = parameters.find((p) => p.id === parameterId);
          onChange(
            next
              ? { parameterId, min: next.min, max: next.max, steps: value.steps }
              : { ...value, parameterId },
          );
        }}
      />
      <Range
        label="from"
        money={money}
        value={value.min}
        disabled={disabled}
        onCommit={(min) => onChange({ ...value, min })}
      />
      <Range
        label="to"
        money={money}
        value={value.max}
        disabled={disabled}
        onCommit={(max) => onChange({ ...value, max })}
      />
      {showSteps && (
        <Range
          label="steps"
          value={value.steps}
          width={42}
          min={2}
          max={12}
          disabled={disabled}
          onCommit={(steps) => onChange({ ...value, steps: Math.round(steps) })}
        />
      )}
    </Blueprint>
  );
}

function Range({
  label,
  value,
  onCommit,
  money,
  width = 84,
  min,
  max,
  disabled,
}: {
  label: string;
  value: number;
  onCommit: (value: number) => void;
  money?: boolean;
  width?: number;
  min?: number;
  max?: number;
  disabled?: boolean;
}) {
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 5 }}>
      <span
        style={{
          fontSize: 11,
          color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
        }}
      >
        {label}
      </span>
      <NumberInput
        aria-label={label}
        value={value}
        onCommit={onCommit}
        decimals={0}
        min={min}
        max={max}
        disabled={disabled}
        prefix={money ? "$" : undefined}
        style={{ width, minHeight: 28 }}
      />
    </span>
  );
}
