"use client";

import { Button, Tag, Tooltip } from "@/components/ui";
import {
  PERSIST_DENIED_NOTE,
  PRIVACY_PROMISE,
  SAFARI_NUDGE,
} from "@/lib/local/durability";
import type { PlanHome } from "@/lib/nav";

const RULE = { borderBottom: "1px solid var(--color-divider)" } as const;

/**
 * Where the open plan lives and what can be done about it, in one slim line
 * under the header: the home badge, Move to cloud or Download to this device,
 * the backup button when one is due, and the quiet notes about storage that
 * could be cleared. Shown only when local mode is on, so with it off the app
 * has no line here at all.
 */
export function PlanHomeBar({
  home,
  account,
  backupDue,
  persistDenied,
  safariNudge,
  onMoveToCloud,
  onDownload,
  onBackup,
  onFiles,
  onSignIn,
  onDismissSafari,
  readOnly,
}: {
  home: PlanHome;
  /** Signed in to an account: the only way to put a plan in the cloud. */
  account: boolean;
  /** "Back up this plan", next to Move to cloud. */
  backupDue: boolean;
  persistDenied: boolean;
  safariNudge: boolean;
  onMoveToCloud: () => void;
  onDownload: () => void;
  onBackup: () => void;
  onFiles: () => void;
  onSignIn: () => void;
  onDismissSafari: () => void;
  /** The server cannot be reached, so what needs it is held back. */
  readOnly?: boolean;
}) {
  const local = home === "local";
  return (
    <>
      <div
        role="status"
        style={{
          ...RULE,
          padding: "6px 16px",
          fontSize: 12,
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          gap: "4px 10px",
        }}
      >
        <Tooltip
          ariaLabel={local ? "This plan lives on this device" : "This plan lives in the cloud"}
          content={
            local
              ? PRIVACY_PROMISE
              : "This plan is stored on FinPlan's servers with your account."
          }
        >
          <Tag tone={local ? "neutral" : "accent-2"}>{local ? "This device" : "Cloud"}</Tag>
        </Tooltip>

        {local &&
          (account ? (
            <Button
              variant="ghost"
              disabled={readOnly}
              title={readOnly ? "No connection to the server." : undefined}
              onClick={onMoveToCloud}
            >
              Move to cloud
            </Button>
          ) : (
            <button type="button" className="linkbtn" onClick={onSignIn}>
              Sign in to move to cloud
            </button>
          ))}
        {local && backupDue && (
          <Button variant="primary" onClick={onBackup}>
            Back up this plan
          </Button>
        )}
        {!local && (
          <Button variant="ghost" onClick={onDownload}>
            Download to this device
          </Button>
        )}
        <Button variant="ghost" onClick={onFiles}>
          Export / import…
        </Button>

        {local && persistDenied && (
          <span role="note" className="ns-mut">
            {PERSIST_DENIED_NOTE}
          </span>
        )}
      </div>

      {local && safariNudge && (
        <p
          role="note"
          style={{
            ...RULE,
            margin: 0,
            padding: "6px 16px",
            fontSize: 12,
            display: "flex",
            flexWrap: "wrap",
            gap: "4px 10px",
            alignItems: "center",
          }}
        >
          <span>{SAFARI_NUDGE}</span>
          <button type="button" className="linkbtn" onClick={onDismissSafari}>
            Dismiss
          </button>
        </p>
      )}
    </>
  );
}
