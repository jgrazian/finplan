"use client";

import type { ReactNode } from "react";
import { Button } from "@/components/ui";
import { formatDuration } from "@/lib/local/estimate";
import type { GuestMigration } from "@/lib/local/homes";
import type { OffloadState } from "@/lib/local/offload";
import { STORE_CLEARED_NOTICE } from "@/lib/local/durability";

const LINE = {
  margin: 0,
  padding: "8px 16px",
  fontSize: 12,
  borderBottom: "1px solid var(--color-divider)",
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "4px 12px",
} as const;

function Line({ children, alert }: { children: ReactNode; alert?: boolean }) {
  return (
    <p role={alert ? "alert" : "status"} style={LINE}>
      {children}
    </p>
  );
}

/**
 * Under the header while a run is going: what it costs, and where it runs. The
 * Cancel button is the progress bar's own; this says how long to expect, and
 * for an offloaded run, that the plan was sent and is not kept.
 */
export function RunNotice({
  seconds,
  offload,
}: {
  /** The estimate for a run here that is long enough to say so (10–60 s). */
  seconds?: number;
  /** An offload in flight. */
  offload: OffloadState;
}) {
  if (offload.phase === "sending" || offload.phase === "queued" || offload.phase === "running" || offload.phase === "saving") {
    return (
      <Line>
        Running on FinPlan&rsquo;s servers. Your plan was sent for this run only and is not stored
        there; the results are saved on this device. Cancel stops the run.
      </Line>
    );
  }
  if (seconds === undefined) return null;
  return (
    <Line>
      Running on this device: about {formatDuration(seconds)}. Cancel stops it.
    </Line>
  );
}

/** How an offload ended, when it did not end in a run on screen. */
export function OffloadOutcome({
  state,
  onDismiss,
}: {
  state: OffloadState;
  onDismiss: () => void;
}) {
  if (state.phase === "stale") {
    return (
      <Line alert>
        <span>{state.message}</span>
        <Button variant="primary" onClick={() => window.location.reload()}>
          Reload
        </Button>
      </Line>
    );
  }
  if (state.phase === "failed") {
    return (
      <Line alert>
        <span>The run on FinPlan&rsquo;s servers did not finish: {state.message}</span>
        <button type="button" className="linkbtn" onClick={onDismiss}>
          Dismiss
        </button>
      </Line>
    );
  }
  if (state.phase === "done") {
    return (
      <Line>
        <span>
          Ran on FinPlan&rsquo;s servers. The results are saved on this device; the plan was not
          stored there.
        </span>
        <button type="button" className="linkbtn" onClick={onDismiss}>
          Dismiss
        </button>
      </Line>
    );
  }
  return null;
}

/** Spec 17's guest, after the move: "Your plan now lives on this device". */
export function MigrationNotice({
  result,
  onDismiss,
}: {
  result: GuestMigration;
  onDismiss: () => void;
}) {
  if (result.status === "migrated") {
    return (
      <Line>
        <span>
          {result.plans === 1
            ? "Your plan now lives on this device."
            : `Your ${result.plans} plans now live on this device.`}{" "}
          It is no longer on FinPlan&rsquo;s servers; export a backup from the plan&rsquo;s menu
          whenever you like.
        </span>
        <button type="button" className="linkbtn" onClick={onDismiss}>
          Dismiss
        </button>
      </Line>
    );
  }
  if (result.status === "failed") {
    return (
      <Line alert>
        <span>
          Your guest plan could not be moved to this device ({result.error}). It is still on
          FinPlan&rsquo;s servers, unchanged.
        </span>
        <button type="button" className="linkbtn" onClick={onDismiss}>
          Dismiss
        </button>
      </Line>
    );
  }
  return null;
}

/** A returning visitor whose browser emptied the store: say so, and offer the way back. */
export function StoreClearedNotice({ onImport }: { onImport: () => void }) {
  return (
    <div role="alert" style={{ padding: "16px 24px 0", maxWidth: 560 }}>
      <p style={{ margin: "0 0 10px", fontSize: 13, lineHeight: 1.5 }}>{STORE_CLEARED_NOTICE}</p>
      <Button variant="primary" onClick={onImport}>
        Import a backup file
      </Button>
    </div>
  );
}
