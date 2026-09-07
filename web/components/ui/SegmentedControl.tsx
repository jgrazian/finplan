"use client";

import { useId } from "react";

export interface SegmentOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
  title?: string;
}

/**
 * Radio-backed segmented control. Generic over the option union so callers
 * keep their literal types (`'fan' | 'stack' | 'bar'`) end to end.
 */
export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  name,
  ariaLabel,
}: {
  options: ReadonlyArray<SegmentOption<T>>;
  value: T;
  onChange: (value: T) => void;
  /** Defaults to a generated id; set it when two controls share a form. */
  name?: string;
  ariaLabel?: string;
}) {
  const generated = useId();
  const group = name ?? generated;
  return (
    <div className="seg" role="radiogroup" aria-label={ariaLabel}>
      {options.map((opt) => (
        <label className="seg-opt" key={opt.value} title={opt.title} style={opt.disabled ? { opacity: 0.5 } : undefined}>
          <input
            type="radio"
            name={group}
            disabled={opt.disabled}
            checked={value === opt.value}
            onChange={() => onChange(opt.value)}
          />
          {opt.label}
        </label>
      ))}
    </div>
  );
}
