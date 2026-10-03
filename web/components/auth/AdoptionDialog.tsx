"use client";

import { Button } from "@/components/ui";
import type { Adoption } from "@/lib/hooks/useSession";

/**
 * Asked once, right after a guest signs in to an existing account: the guest
 * is about to be deleted, so this is the only chance to bring its plan along.
 * A refusal (a full plan slot, say) stays on screen with the choice still open.
 */
export function AdoptionDialog({
  adoption,
  onAdopt,
  onDecline,
}: {
  adoption: Adoption;
  onAdopt: () => void;
  onDecline: () => void;
}) {
  const { plans, busy, error } = adoption;
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
        aria-labelledby="adopt-title"
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
        <h4 id="adopt-title" style={{ margin: 0 }}>
          Bring your guest plan into this account?
        </h4>
        <p style={{ margin: 0, fontSize: 13, lineHeight: 1.55 }}>
          {plans === 1 ? "The plan you built as a guest" : `The ${plans} plans you built as a guest`}{" "}
          will be copied here, named &ldquo;Guest – &hellip;&rdquo;. Your guest session has ended,
          so anything you leave behind is gone.
        </p>
        {error && (
          <p role="alert" style={{ margin: 0, fontSize: 12, color: "var(--color-accent-700)" }}>
            {error}
          </p>
        )}
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <Button disabled={busy} onClick={onDecline}>
            No, start fresh
          </Button>
          <Button variant="primary" disabled={busy} onClick={onAdopt}>
            {busy ? "Bringing…" : error ? "Try again" : "Yes, bring it"}
          </Button>
        </div>
      </div>
    </div>
  );
}
