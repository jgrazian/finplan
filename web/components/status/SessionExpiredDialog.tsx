"use client";

import { Button } from "@/components/ui";

/**
 * Tier 3 takes the screen.
 *
 * The only tier that does: no retry can fix an expired session, so there is
 * nothing to wait for and no reason to leave the app looking usable. What was
 * typed is not thrown away — the fields keep it — and the dialog says so,
 * because that is the question anyone reading this actually has.
 */
export function SessionExpiredDialog({ onSignIn }: { onSignIn: () => void }) {
  return (
    <div
      role="presentation"
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 60,
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        padding: "12vh 16px 16px",
        background: "var(--color-scrim)",
      }}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="session-expired-title"
        style={{
          width: "min(440px, 100%)",
          background: "var(--color-bg)",
          border: "1px solid var(--color-divider)",
          boxShadow: "var(--shadow-lg)",
          padding: "16px 18px 18px",
          display: "flex",
          flexDirection: "column",
          gap: 12,
        }}
      >
        <h4 id="session-expired-title" style={{ margin: 0 }}>
          Session expired
        </h4>
        <p style={{ margin: 0, fontSize: 13, lineHeight: 1.55 }}>
          Sign in again to save anything further. Nothing you have typed is
          discarded — it stays in the field that holds it, and can be saved once
          you are back.
        </p>
        <div style={{ display: "flex", justifyContent: "flex-end" }}>
          <Button variant="primary" onClick={onSignIn}>
            Sign in
          </Button>
        </div>
      </div>
    </div>
  );
}
