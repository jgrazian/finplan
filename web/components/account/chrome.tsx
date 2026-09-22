"use client";

import type { ReactNode } from "react";
import { Button } from "@/components/ui";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/** The paragraph under a form that says what the fields above actually do. */
export function PanelNote({ children }: { children: ReactNode }) {
  return (
    <p style={{ fontSize: 12, margin: "12px 0 0", maxWidth: "60ch", lineHeight: 1.55, color: MUTED }}>
      {children}
    </p>
  );
}

/**
 * Save and Discard for a whole-form panel.
 *
 * Both stay visible and go quiet when there is nothing to save, so the pair
 * never moves the layout under the pointer as you type. The server's own
 * refusal is the message shown; there is no second rule set here guessing at
 * the same thing.
 */
export function SaveRow({
  label,
  dirty,
  busy,
  error,
  readOnly,
  onSave,
  onDiscard,
}: {
  label: string;
  dirty: boolean;
  busy?: boolean;
  error?: string;
  readOnly?: boolean;
  onSave: () => void;
  onDiscard?: () => void;
}) {
  return (
    <div style={{ marginTop: 18 }}>
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <Button
          variant="primary"
          disabled={!dirty || busy || readOnly}
          title={readOnly ? "No connection to the server." : undefined}
          onClick={onSave}
        >
          {busy ? "…" : label}
        </Button>
        {onDiscard && (
          <Button disabled={!dirty || busy} onClick={onDiscard}>
            Discard
          </Button>
        )}
        {readOnly && (
          <span style={{ fontSize: 11, color: MUTED }}>read-only until the server answers</span>
        )}
      </div>
      {error && (
        <p style={{ margin: "8px 0 0", fontSize: 12, color: "var(--color-accent-700)" }}>{error}</p>
      )}
    </div>
  );
}

/** A section heading inside a panel, matching the canvas's `h6` rhythm. */
export function PanelHeading({ children }: { children: ReactNode }) {
  return <h6 style={{ margin: "22px 0 6px" }}>{children}</h6>;
}
