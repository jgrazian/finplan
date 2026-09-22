"use client";

import type { ReactNode } from "react";
import { SegmentedControl, type SegmentOption } from "@/components/ui";

/**
 * Second-level navigation inside a tab, with an optional caption that says
 * what the section is for. Sits above the split, flush with its padding.
 */
export function SubTabBar<T extends string>({
  options,
  value,
  onChange,
  caption,
  ariaLabel,
}: {
  options: ReadonlyArray<SegmentOption<T>>;
  value: T;
  onChange: (value: T) => void;
  caption?: ReactNode;
  ariaLabel?: string;
}) {
  return (
    <div
      style={{
        padding: "12px 20px 0",
        display: "flex",
        gap: 10,
        alignItems: "center",
      }}
    >
      <SegmentedControl
        options={options}
        value={value}
        onChange={onChange}
        ariaLabel={ariaLabel}
      />
      {caption && (
        <span
          style={{
            fontSize: 11,
            color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
          }}
        >
          {caption}
        </span>
      )}
    </div>
  );
}
