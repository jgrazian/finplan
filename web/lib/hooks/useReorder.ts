"use client";

import { useCallback, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent, MouseEvent, PointerEvent } from "react";

/**
 * How far the pointer has to travel before a press on a grip becomes a drag.
 * Without it a click that lands on the grip by a pixel would reorder the list.
 */
const THRESHOLD = 3;

/** Anything a row can be keyed by. Every list here uses its server id. */
export type RowKey = string | number;

/** What a grip needs to be one: the pointer gestures and the arrow keys. */
export interface HandleProps {
  onPointerDown: (e: PointerEvent<HTMLElement>) => void;
  onPointerMove: (e: PointerEvent<HTMLElement>) => void;
  onPointerUp: (e: PointerEvent<HTMLElement>) => void;
  onPointerCancel: (e: PointerEvent<HTMLElement>) => void;
  onKeyDown: (e: KeyboardEvent<HTMLElement>) => void;
  onClick: (e: MouseEvent<HTMLElement>) => void;
  disabled?: boolean;
  "data-dragging"?: "" | undefined;
}

export interface Reorder<K extends RowKey> {
  /** The order to render in: the stored one, or what the drag is proposing. */
  order: K[];
  /** Goes on the box the rows sit in; the drop line is measured against it. */
  attachList: (el: HTMLElement | null) => void;
  /** Registers a row's element, so the drop point can be found by pointer. */
  attachRow: (key: K) => (el: HTMLElement | null) => void;
  /** The row being carried, drawn at half strength while it travels. */
  dragging: K | undefined;
  /** Where the drop line sits inside the list box, or nothing when idle. */
  indicator: number | undefined;
  handleProps: (key: K) => HandleProps;
  /** For the list box: `position: relative`, and no text selection mid-drag. */
  listStyle: CSSProperties;
}

/** `list` with the item at `from` lifted out and dropped in before `before`. */
function moved<K>(list: readonly K[], from: number, before: number): K[] {
  const next = list.slice();
  const [item] = next.splice(from, 1);
  next.splice(before > from ? before - 1 : before, 0, item);
  return next;
}

function same<K>(a: readonly K[], b: readonly K[]): boolean {
  return a.length === b.length && a.every((x, i) => x === b[i]);
}

/**
 * Drag-to-reorder for a list of rows, by a grip in each row's left gutter.
 *
 * The order lives on the server, so a drop is a write and a reload — which
 * takes a moment the hand should not have to wait through. So the committed
 * order is held here until a fresh list arrives: the row lands where it was
 * dropped and stays there, instead of snapping back for a frame. Whatever the
 * server then sends replaces it, so a refused write puts the row back.
 *
 * Pointer events rather than HTML5 drag-and-drop: a `<tr>` is not a reliable
 * drag source across browsers, and half these lists are grids of divs anyway.
 */
export function useReorder<K extends RowKey>({
  keys,
  onReorder,
  disabled,
}: {
  /** The rows, in the order the server last returned them. */
  keys: readonly K[];
  /**
   * Absent where the list is not reorderable — offline, or read-only. A
   * promise that rejects releases the held order, so a write the server refused
   * puts the row back where it was rather than leaving the list lying.
   */
  onReorder?: (order: K[]) => void | Promise<unknown>;
  disabled?: boolean;
}): Reorder<K> {
  const [held, setHeld] = useState<K[]>();
  /** The row under the pointer and where it would land, once past THRESHOLD. */
  const [drag, setDrag] = useState<{ key: K; before: number; top: number }>();
  /** The list as it last arrived, to notice when a fresh one replaces it. */
  const [arrived, setArrived] = useState<readonly K[]>(keys);

  const list = useRef<HTMLElement | null>(null);
  const rows = useRef(new Map<K, HTMLElement>());
  /** Pressed but not yet dragging: which row, and where the press landed. */
  const press = useRef<{ key: K; y: number }>(undefined);

  // A new list from the server ends the hold: whatever it says is now the
  // truth, including the order the drop just wrote. Adjusted here rather than
  // in an effect — this is state derived from a prop, and an effect would
  // render the stale order once before correcting it.
  if (!same(arrived, keys)) {
    setArrived(keys);
    setHeld(undefined);
  }

  const order = held ?? [...keys];

  const attachList = useCallback((el: HTMLElement | null) => {
    list.current = el;
  }, []);

  const attachRow = useCallback(
    (key: K) => (el: HTMLElement | null) => {
      if (el) rows.current.set(key, el);
      else rows.current.delete(key);
    },
    [],
  );

  const active = !disabled && onReorder != null && order.length > 1;

  /**
   * Where a pointer at `y` would drop: the number of rows whose middle it has
   * already passed. Measured live rather than cached, because nothing moves
   * during a drag — only the line does — so the rects stay true.
   */
  const measure = (y: number): { before: number; top: number } | undefined => {
    const box = list.current?.getBoundingClientRect();
    if (!box) return undefined;
    const rects = order.map((k) => rows.current.get(k)?.getBoundingClientRect());
    let before = 0;
    for (const rect of rects) {
      if (rect && y > rect.top + rect.height / 2) before += 1;
    }
    const edge =
      before < rects.length ? rects[before]?.top : rects[rects.length - 1]?.bottom;
    return edge == null ? undefined : { before, top: edge - box.top };
  };

  const commit = (key: K, before: number) => {
    const from = order.indexOf(key);
    if (from < 0) return;
    const next = moved(order, from, before);
    if (same(next, order)) return;
    setHeld(next);
    const write = onReorder?.(next);
    // Only releases the hold; reporting the refusal is the caller's to do.
    if (write) Promise.resolve(write).catch(() => setHeld(undefined));
  };

  const handleProps = (key: K): HandleProps => ({
    disabled: !active,
    "data-dragging": drag?.key === key ? "" : undefined,

    // The grip drives the order; the row under it drives the inspector. A
    // press on one must never do the other.
    onClick: (e) => e.stopPropagation(),

    onPointerDown: (e) => {
      if (!active || e.button !== 0) return;
      e.preventDefault();
      e.stopPropagation();
      e.currentTarget.setPointerCapture(e.pointerId);
      press.current = { key, y: e.clientY };
    },

    onPointerMove: (e) => {
      const start = press.current;
      if (!start || start.key !== key) return;
      if (!drag && Math.abs(e.clientY - start.y) < THRESHOLD) return;
      const at = measure(e.clientY);
      if (at) setDrag({ key, ...at });
    },

    onPointerUp: (e) => {
      if (e.currentTarget.hasPointerCapture(e.pointerId)) {
        e.currentTarget.releasePointerCapture(e.pointerId);
      }
      if (press.current?.key === key && drag?.key === key) commit(key, drag.before);
      press.current = undefined;
      setDrag(undefined);
    },

    onPointerCancel: () => {
      press.current = undefined;
      setDrag(undefined);
    },

    // A grip is a focusable button, so the list is reorderable without a
    // pointer at all: the same move, one row at a time.
    onKeyDown: (e) => {
      // Rows answer to Enter and Space by opening in the inspector. Swallow
      // both here: the key that reached the grip was meant for the grip.
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        e.stopPropagation();
        return;
      }
      if (!active || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
      const from = order.indexOf(key);
      const before = e.key === "ArrowUp" ? from - 1 : from + 2;
      if (from < 0 || before < 0 || before > order.length) return;
      e.preventDefault();
      e.stopPropagation();
      commit(key, before);
    },
  });

  return {
    order,
    attachList,
    attachRow,
    dragging: drag?.key,
    indicator: drag?.top,
    handleProps,
    listStyle: { position: "relative", ...(drag ? { userSelect: "none" } : null) },
  };
}
