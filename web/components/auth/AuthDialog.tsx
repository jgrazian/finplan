"use client";

import { useEffect } from "react";
import { Button } from "@/components/ui";
import type { Session } from "@/lib/hooks/useSession";
import { LoginForm } from "./LoginForm";

export type AuthMode = "signIn" | "signUp";

/**
 * Sign up or sign in over the workbench, for a guest who wants out of being
 * one. The guest's plan stays on screen underneath: Sign up claims it in
 * place, and Sign in offers to bring it along afterwards.
 */
export function AuthDialog({
  session,
  mode,
  onClose,
}: {
  session: Session;
  mode: AuthMode;
  onClose: () => void;
}) {
  const { clearError } = session;
  useEffect(() => {
    clearError();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, clearError]);

  return (
    <div
      role="presentation"
      onClick={(e) => e.target === e.currentTarget && onClose()}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 60,
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        padding: "8vh 16px 16px",
        overflowY: "auto",
        background: "var(--color-scrim)",
        backdropFilter: "blur(3px)",
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={mode === "signUp" ? "Create a free account" : "Sign in"}
        className="dialog-panel"
        style={{ width: "min(400px, 100%)", gap: 12 }}
      >
        <LoginForm key={mode} session={session} initialMode={mode} embedded />
        <Button variant="ghost" block onClick={onClose}>
          Keep browsing as a guest
        </Button>
      </div>
    </div>
  );
}
