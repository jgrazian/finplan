import type { CSSProperties } from "react";

/**
 * The row driving an inspector: an accent rule down its left edge carries the
 * selection, over a wash light enough that the row's own figures and tags
 * still read as the loudest thing in it.
 */
export const SELECTED_ROW: CSSProperties = {
  background: "color-mix(in srgb, var(--color-accent) 7%, transparent)",
  boxShadow: "inset 3px 0 0 var(--color-accent)",
};

export function rowStyle(selected: boolean): CSSProperties | undefined {
  return selected ? SELECTED_ROW : undefined;
}
