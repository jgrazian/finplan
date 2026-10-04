"use client";

import { Button } from "@/components/ui";
import { PERSIST_DENIED_NOTE, SAFARI_NUDGE } from "@/lib/local/durability";

const LINE = {
  borderBottom: "1px solid var(--color-divider)",
  margin: 0,
  padding: "6px 16px",
  fontSize: 12,
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "4px 10px",
} as const;

/**
 * The warnings a plan kept on this device can need, under the header: a
 * backup is due, the browser would not promise to keep site data, or Safari
 * will clear it after a week away. Where the plan lives and the moves between
 * homes are the header's scenario switcher; this says only what is at risk, so
 * with nothing to warn about it is not there at all.
 */
export function PlanHomeBar({
  backupDue,
  persistDenied,
  safariNudge,
  onBackup,
  onDismissSafari,
}: {
  /** "Back up this plan": edits since the last export, and long enough ago. */
  backupDue: boolean;
  persistDenied: boolean;
  safariNudge: boolean;
  onBackup: () => void;
  onDismissSafari: () => void;
}) {
  if (!backupDue && !persistDenied && !safariNudge) return null;
  return (
    <>
      {(backupDue || persistDenied) && (
        <div role="status" style={LINE}>
          {backupDue && (
            <Button variant="primary" onClick={onBackup}>
              Back up this plan
            </Button>
          )}
          {persistDenied && (
            <span role="note" className="ns-mut">
              {PERSIST_DENIED_NOTE}
            </span>
          )}
        </div>
      )}

      {safariNudge && (
        <p role="note" style={LINE}>
          <span>{SAFARI_NUDGE}</span>
          <button type="button" className="linkbtn" onClick={onDismissSafari}>
            Dismiss
          </button>
        </p>
      )}
    </>
  );
}
