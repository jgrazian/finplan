"use client";

import { type ReactNode, useEffect, useRef } from "react";
import { Button } from "./Button";

/**
 * A modal form surface.
 *
 * The inspectors are drawers precisely so nothing is covered up, but creating a
 * row is different: it has no row to sit beside yet, and it is a commit-or-
 * abandon step rather than an edit. Escape and the backdrop both abandon it.
 */
export function Dialog({
  title,
  onClose,
  onSubmit,
  submitLabel,
  footer,
  busy,
  error,
  children,
}: {
  title: string;
  onClose: () => void;
  onSubmit: () => void;
  submitLabel?: string;
  /** Replaces the standard Cancel/submit controls for flows with multiple paths. */
  footer?: ReactNode;
  busy?: boolean;
  error?: string;
  children: ReactNode;
}) {
  const panel = useRef<HTMLFormElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    // Focus the first control so the dialog is usable without reaching for the
    // mouse, and so screen readers land inside it.
    panel.current?.querySelector<HTMLElement>("input, select")?.focus();
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      role="presentation"
      onClick={(e) => e.target === e.currentTarget && onClose()}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 50,
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        padding: "8vh 16px 16px",
        overflowY: "auto",
        background: "var(--color-scrim)",
      }}
    >
      <form
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onSubmit={(e) => {
          e.preventDefault();
          onSubmit();
        }}
        style={{
          width: "min(560px, 100%)",
          background: "var(--color-bg)",
          border: "1px solid var(--color-divider)",
          boxShadow: "var(--shadow-lg)",
          padding: "16px 18px 18px",
          display: "flex",
          flexDirection: "column",
          gap: 12,
        }}
      >
        <h4 style={{ margin: 0 }}>{title}</h4>
        {children}

        {error && (
          <p style={{ margin: 0, fontSize: 12, color: "var(--color-accent-700)" }}>{error}</p>
        )}

        {footer ?? (
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 4 }}>
            <Button type="button" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={busy}>
              {busy ? "…" : submitLabel}
            </Button>
          </div>
        )}
      </form>
    </div>
  );
}

/** Two controls on one row, the dialogs' default rhythm. */
export function DialogRow({ children }: { children: ReactNode }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>{children}</div>
  );
}
