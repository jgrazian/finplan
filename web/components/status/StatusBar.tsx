"use client";

import { useState } from "react";
import { type StatusIssue, serverMonitor } from "@/lib/status/monitor";
import { useServerStatus } from "@/lib/status/useServerStatus";

/**
 * Server state, in one bar under the nav.
 *
 * Four tiers, and severity is carried by field weight rather than hue: a
 * notice is an accent tint, an error is paper type on an ink field, and the
 * blocking tier is the same field with a dialog over the screen. Field state —
 * a rejected save, a validation message — never appears here; it belongs under
 * the input that caused it.
 */
export function StatusBar({
  onRetry,
  onRunAgain,
  onSignIn,
}: {
  /** Refresh whatever the outage left stale, alongside the reconnect probe. */
  onRetry?: () => void;
  onRunAgain?: () => void;
  onSignIn?: () => void;
}) {
  const status = useServerStatus();
  const [expanded, setExpanded] = useState(false);
  const { issue, others, confirmation, retryIn } = status;

  if (!issue) {
    return confirmation ? (
      <div className="sbar sbar-notice" role="status">
        <CheckIcon />
        <span>{confirmation.message}</span>
        {confirmation.detail && <span className="sact sbar-note">{confirmation.detail}</span>}
      </div>
    ) : null;
  }

  const detail = [
    issue.detail,
    issue.kind === "connection" && retryIn != null
      ? retryIn > 0
        ? `retrying in ${retryIn}s`
        : "retrying"
      : undefined,
  ]
    .filter(Boolean)
    .join(" · ");

  const stack = others.length > 0 || issue.more != null;

  return (
    <>
      <div
        className={`sbar ${TIER_CLASS[issue.tier]}`}
        role={issue.tier >= 2 ? "alert" : "status"}
      >
        <TierIcon issue={issue} />
        <span>{issue.message}</span>
        <span className="sact">
          {detail && <span className="sbar-note">{detail}</span>}
          {stack && (
            <button
              className="sbtn"
              type="button"
              aria-expanded={expanded}
              onClick={() => setExpanded((open) => !open)}
            >
              Details
            </button>
          )}
          {issue.actions.includes("retry") && (
            <button
              className="sbtn"
              type="button"
              onClick={() => {
                serverMonitor.retryNow();
                onRetry?.();
              }}
            >
              Retry now
            </button>
          )}
          {issue.actions.includes("runAgain") && (
            <button className="sbtn" type="button" onClick={onRunAgain}>
              Run again
            </button>
          )}
          {issue.actions.includes("signIn") && (
            <button className="sbtn" type="button" onClick={onSignIn}>
              Sign in
            </button>
          )}
        </span>
      </div>

      {/* The newest error wins the bar; anything else that is still true
          stacks here rather than being lost. */}
      {expanded && stack && (
        <div className="sbar-stack">
          {issue.more && <p>{issue.more}</p>}
          {others.map((other) => (
            <p key={other.kind}>
              <strong style={{ fontWeight: 500 }}>{other.message}</strong>
              {other.more ? ` ${other.more}` : ""}
            </p>
          ))}
        </div>
      )}
    </>
  );
}

const TIER_CLASS: Record<number, string> = {
  1: "sbar-notice",
  2: "sbar-error",
  3: "sbar-blocking",
};

function TierIcon({ issue }: { issue: StatusIssue }) {
  if (issue.kind === "session") return <LockIcon />;
  if (issue.kind === "run") return <WarningIcon />;
  return issue.tier === 1 ? <ClockIcon /> : <OfflineIcon />;
}

const SVG = {
  width: 15,
  height: 15,
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round",
} as const;

function ClockIcon() {
  return (
    <svg {...SVG} aria-hidden="true">
      <circle cx="8" cy="8" r="6.25" />
      <path d="M8 4.5V8l2.6 1.6" />
    </svg>
  );
}

function OfflineIcon() {
  return (
    <svg {...SVG} aria-hidden="true">
      <circle cx="8" cy="8" r="6.25" />
      <line x1="4.1" y1="11.9" x2="11.9" y2="4.1" />
    </svg>
  );
}

function WarningIcon() {
  return (
    <svg {...SVG} aria-hidden="true">
      <path d="M8 2.4 14.4 13.4H1.6z" />
      <line x1="8" y1="6.4" x2="8" y2="9.5" />
      <circle cx="8" cy="11.5" r="0.55" fill="currentColor" stroke="none" />
    </svg>
  );
}

function LockIcon() {
  return (
    <svg {...SVG} aria-hidden="true">
      <rect x="3.5" y="7" width="9" height="6.4" />
      <path d="M5.8 7V5.2a2.2 2.2 0 0 1 4.4 0V7" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg {...SVG} aria-hidden="true">
      <path d="M3 8.6 6.4 12 13 4.6" />
    </svg>
  );
}
