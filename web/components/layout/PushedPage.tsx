"use client";

import type { ReactNode } from "react";

/**
 * A phone's stand-in for the inspector rail: the drawer's contents as a
 * full-screen page pushed over the list. Back and Done do the same thing —
 * edits apply from the drawer's own footer, so there is nothing to commit on
 * the way out, only a selection to clear.
 */
export function PushedPage({
  back,
  title,
  onClose,
  children,
}: {
  /** What the back link returns to, e.g. "Accounts". */
  back: string;
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <div className="mobile-pushed" role="dialog" aria-label={title}>
      <div className="mobile-pushed-bar">
        <button type="button" className="mobile-pushed-btn" onClick={onClose}>
          ‹ {back}
        </button>
        <span className="mobile-pushed-title">{title}</span>
        <button type="button" className="mobile-pushed-btn" onClick={onClose}>
          Done
        </button>
      </div>
      {children}
    </div>
  );
}
