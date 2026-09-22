"use client";

import type { HandleProps } from "@/lib/hooks/useReorder";

/**
 * The grip a row is dragged by, in the gutter the selected row's accent rule
 * runs down — so the thing that marks a row and the thing that moves it are in
 * the same place, and neither costs the row a column of its own.
 *
 * It is a real button, not a decorated span: focus it and the arrow keys move
 * the row, which is the whole feature without a pointer.
 */
export function DragHandle({
  label,
  props,
}: {
  /** What is being moved, for the screen reader: "Reorder Brokerage". */
  label: string;
  props: HandleProps;
}) {
  return (
    <button
      type="button"
      className="grip"
      aria-label={`Reorder ${label}`}
      title="Drag to reorder, or use ↑ ↓"
      {...props}
    >
      <svg width="6" height="14" viewBox="0 0 6 14" fill="currentColor" aria-hidden>
        {[2, 5, 8, 11].map((y) => (
          <g key={y}>
            <circle cx="1" cy={y} r="1" />
            <circle cx="5" cy={y} r="1" />
          </g>
        ))}
      </svg>
    </button>
  );
}

/**
 * The line showing where a dragged row would land, drawn across the list at
 * the boundary it would slot into. Null while nothing is being dragged.
 */
export function DropLine({ at }: { at: number | undefined }) {
  if (at == null) return null;
  return (
    <div
      aria-hidden
      style={{
        position: "absolute",
        left: 0,
        right: 0,
        top: at - 1,
        height: 2,
        background: "var(--color-accent)",
        pointerEvents: "none",
        zIndex: 2,
      }}
    />
  );
}
