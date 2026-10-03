"use client";

import { createContext, type ReactNode, useContext } from "react";
import { Button } from "@/components/ui";
import { CREATE_ACCOUNT, SIGN_UP_FOR_ITERATIONS } from "@/lib/view/guest";

/**
 * What the screens need to know about a guest (spec 17), without threading it
 * through every props list between the app and a locked panel.
 */
export interface GuestState {
  /** The session is a guest's: no account yet. */
  guest: boolean;
  /**
   * A guest the server holds to the guest limits (hosted). A self-hosted
   * guest has no account either, but nothing is locked for it.
   */
  restricted: boolean;
  /** Days without a visit before the guest's plans are deleted. */
  retentionDays: number | null;
  openSignUp: () => void;
  openSignIn: () => void;
}

const SIGNED_IN: GuestState = {
  guest: false,
  restricted: false,
  retentionDays: null,
  openSignUp: () => undefined,
  openSignIn: () => undefined,
};

const GuestContext = createContext<GuestState>(SIGNED_IN);

export function GuestProvider({ value, children }: { value: GuestState; children: ReactNode }) {
  return <GuestContext.Provider value={value}>{children}</GuestContext.Provider>;
}

export function useGuest(): GuestState {
  return useContext(GuestContext);
}

/** "Create a free account", opening Sign up; plain text for anyone who has one. */
export function CreateAccountLink({ label = CREATE_ACCOUNT }: { label?: string }) {
  const { guest, openSignUp } = useGuest();
  if (!guest) return <>{label}</>;
  return (
    <button type="button" className="linkbtn" onClick={openSignUp}>
      {label}
    </button>
  );
}

/** "Sign up free for 1,000 iterations", for a guest held to the guest cap; nothing otherwise. */
export function IterationUpsell() {
  const { restricted } = useGuest();
  if (!restricted) return null;
  return <CreateAccountLink label={SIGN_UP_FOR_ITERATIONS} />;
}

/** A feature the guest limits switch off, in the place its screen would be. */
export function LockedFeature({ title, children }: { title: string; children: ReactNode }) {
  const { openSignUp } = useGuest();
  return (
    <div role="status" style={{ padding: "40px 24px", maxWidth: 520 }}>
      <h4 style={{ margin: "0 0 6px" }}>{title}</h4>
      <p
        style={{
          margin: "0 0 14px",
          fontSize: 13,
          lineHeight: 1.5,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
        }}
      >
        {children}
      </p>
      <Button variant="primary" onClick={openSignUp}>
        {CREATE_ACCOUNT}
      </Button>
    </div>
  );
}
