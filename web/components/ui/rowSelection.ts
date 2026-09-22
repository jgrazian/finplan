import type { CSSProperties } from "react";

/** The accent wash plus left rule that marks the row driving an inspector. */
export const SELECTED_ROW: CSSProperties = {
  background: "color-mix(in srgb, var(--color-accent) 14%, transparent)",
  boxShadow: "inset 2px 0 0 var(--color-accent)",
};

export function rowStyle(selected: boolean): CSSProperties | undefined {
  return selected ? SELECTED_ROW : undefined;
}
