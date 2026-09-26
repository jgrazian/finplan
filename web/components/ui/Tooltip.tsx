"use client";

import { type ReactNode, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { cx } from "./cx";

export interface TooltipProps {
  /** Plain text or inline formatting; do not wrap an existing button or input. */
  children: ReactNode;
  /** Explanatory text or inline formatting, without links or other controls. */
  content: ReactNode;
  /** Accessible name for a label made from multiple inline elements. */
  ariaLabel?: string;
  className?: string;
}

/**
 * Wrap a heading or field label: <Tooltip content="Details">Label</Tooltip>.
 * The trigger inherits the surrounding type. A native top-layer popover keeps
 * the explanation inside the document's accessibility tree without clipping
 * against a scrolling panel. Focus stays on the trigger throughout.
 */
export function Tooltip({ children, content, ariaLabel, className }: TooltipProps) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const bubble = useRef<HTMLSpanElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pinned = useRef(false);
  const [open, setOpen] = useState(false);

  function cancelClose() {
    clearTimeout(timer.current);
  }

  function dismiss() {
    cancelClose();
    pinned.current = false;
    setOpen(false);
  }

  function leave() {
    cancelClose();
    timer.current = setTimeout(() => {
      if (!pinned.current && document.activeElement !== trigger.current) setOpen(false);
    }, 150);
  }

  useEffect(() => () => clearTimeout(timer.current), []);

  useLayoutEffect(() => {
    const anchor = trigger.current;
    const tip = bubble.current;
    if (!open || !anchor || !tip) return;

    tip.showPopover();
    function position() {
      if (!anchor || !tip) return;
      const rect = anchor.getBoundingClientRect();
      const width = document.documentElement.clientWidth;
      const height = document.documentElement.clientHeight;
      const gap = 8;
      const edge = 12;
      const bounds = tip.getBoundingClientRect();
      const below = rect.bottom + gap;
      const top = below + bounds.height <= height - edge
        ? below
        : Math.max(edge, Math.min(rect.top - gap - bounds.height, height - edge - bounds.height));
      const left = Math.max(edge, Math.min(rect.left, width - edge - bounds.width));
      tip.style.top = `${top}px`;
      tip.style.left = `${left}px`;
    }
    position();
    const observer = new ResizeObserver(position);
    observer.observe(tip);
    observer.observe(anchor);
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);

    function close() {
      clearTimeout(timer.current);
      pinned.current = false;
      setOpen(false);
    }
    function onKey(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      // Dismiss help before Escape reaches a surrounding dialog.
      event.preventDefault();
      event.stopPropagation();
      close();
    }
    function onOutside(event: PointerEvent) {
      if (event.target instanceof Node && !anchor?.contains(event.target) && !tip?.contains(event.target)) close();
    }
    document.addEventListener("keydown", onKey, true);
    document.addEventListener("pointerdown", onOutside, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", position, true);
      document.removeEventListener("keydown", onKey, true);
      document.removeEventListener("pointerdown", onOutside, true);
      tip.hidePopover();
    };
  }, [open]);

  return (
    <span className={cx("tooltip", className)}>
      <button
        ref={trigger}
        type="button"
        className="tooltip-trigger"
        aria-label={ariaLabel ?? (typeof children === "string" ? children : undefined)}
        aria-describedby={id}
        onPointerEnter={(event) => {
          if (event.pointerType === "touch") return;
          cancelClose();
          setOpen(true);
        }}
        onPointerLeave={leave}
        onFocus={() => { cancelClose(); setOpen(true); }}
        onBlur={dismiss}
        onClick={() => {
          cancelClose();
          pinned.current = !pinned.current;
          setOpen(pinned.current);
        }}
      >
        {children}
      </button>
      <span
        ref={bubble}
        id={id}
        role="tooltip"
        popover="manual"
        className="tooltip-content"
        onPointerEnter={cancelClose}
        onPointerLeave={leave}
      >
        {content}
      </span>
    </span>
  );
}
