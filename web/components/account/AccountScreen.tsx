"use client";

import { Button, rowStyle } from "@/components/ui";
import type { Scenario, UserResponse } from "@/lib/api/types";
import { useNav } from "@/lib/nav";
import { BillingPanel } from "./BillingPanel";
import { VerificationPanel } from "./VerificationPanel";
import { DataPanel } from "./DataPanel";
import { PreferencesPanel } from "./PreferencesPanel";
import { ProfilePanel } from "./ProfilePanel";
import { SecurityPanel } from "./SecurityPanel";

type SectionId = "billing" | "profile" | "security" | "data" | "preferences";

const SECTIONS: ReadonlyArray<{ id: SectionId; label: string; note: string }> = [
  { id: "billing", label: "Plan and billing", note: "Access, usage, editable plan" },
  { id: "profile", label: "Profile", note: "Name, email, birth date" },
  { id: "security", label: "Security", note: "Password and devices" },
  { id: "data", label: "Data", note: "Scenarios, export, deletion" },
  { id: "preferences", label: "Preferences", note: "Defaults and appearance" },
];

/**
 * Account settings — the inspector pattern the rest of the app uses, turned
 * inward.
 *
 * A section list drives one panel, exactly as the Portfolio outline drives the
 * rail, so nothing new has to be learned to find the password field. It takes
 * the whole content area rather than opening as a modal: these are destinations
 * you navigate to, not a decision to commit or abandon.
 */
export function AccountScreen({
  user,
  scenarios,
  onUserChange,
  onSignOut,
  onDeleted,
  offline,
}: {
  user: UserResponse;
  scenarios: Scenario[];
  onUserChange: (user: UserResponse) => void;
  onSignOut: () => void;
  /** The account is gone: everything above this has to be torn down. */
  onDeleted: () => void;
  /** Writes are being refused, so the forms close rather than lie. */
  offline?: boolean;
}) {
  // The section is the query's sub-tab, the same slot the Portfolio tab's
  // segmented control uses, so a link to the password field is just a URL.
  const nav = useNav();
  const active = SECTIONS.find((s) => s.id === nav.section) ?? SECTIONS[0];

  return (
    <div style={{ display: "grid", gridTemplateColumns: "250px 1fr", alignItems: "stretch" }}>
      <div style={{ borderRight: "1px solid var(--color-divider)", padding: "14px 0" }}>
        <h6 style={{ margin: "0 0 8px", padding: "0 16px" }}>Account</h6>
        {/* Buttons rather than rows with a click handler: this is navigation,
            and it should reach the keyboard without being reimplemented. */}
        <nav style={{ display: "flex", flexDirection: "column" }} aria-label="Account sections">
          {SECTIONS.map((entry) => (
            <button
              key={entry.id}
              type="button"
              className="rowsel acct-section"
              aria-current={entry.id === active.id ? "page" : undefined}
              style={rowStyle(entry.id === active.id)}
              onClick={() => nav.setSection(entry.id)}
            >
              <span style={{ fontFamily: "var(--font-heading)", fontWeight: 600, fontSize: 14 }}>
                {entry.label}
              </span>
              <span
                style={{
                  fontSize: 11.5,
                  color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
                }}
              >
                {entry.note}
              </span>
            </button>
          ))}
        </nav>
        <div style={{ padding: "14px 16px 0" }}>
          <Button variant="ghost" onClick={onSignOut}>
            Sign out
          </Button>
        </div>
      </div>

      <div style={{ padding: "20px 24px 24px" }}>
        <h3 style={{ margin: "0 0 16px" }}>{active.label}</h3>

        {active.id === "billing" && <BillingPanel scenarios={scenarios} readOnly={offline} />}
        {active.id === "profile" && (<>
          <ProfilePanel user={user} onSaved={onUserChange} readOnly={offline} />
          <VerificationPanel user={user} onSaved={onUserChange} readOnly={offline} />
        </>)}
        {active.id === "security" && <SecurityPanel readOnly={offline} />}
        {active.id === "data" && (
          <DataPanel
            user={user}
            scenarios={scenarios}
            onDeleted={onDeleted}
            readOnly={offline}
          />
        )}
        {active.id === "preferences" && (
          <PreferencesPanel user={user} onSaved={onUserChange} readOnly={offline} />
        )}
      </div>
    </div>
  );
}
