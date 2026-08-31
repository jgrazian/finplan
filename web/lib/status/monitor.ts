/**
 * Server health, as one observable that lives outside React.
 *
 * `http.ts` is a plain module rather than a hook, so the thing it reports into
 * has to be one too; components read it back through `useServerStatus`.
 *
 * The tiers are the design's: silent while recovery is still plausible, a
 * notice while it is being attempted, an ink bar once changes are being
 * refused, and a blocking field when no retry can help. Severity is carried by
 * field weight, not hue — see `StatusBar`.
 */

/** Backoff between reconnect probes. The first three run silently. */
const BACKOFF_MS = [1000, 2000, 4000, 8000, 16000, 30000];

/** Consecutive failures before the notice shows, and before we call it offline. */
const NOTICE_AFTER = 4;
const OFFLINE_AFTER = 6;

/** How long the recovery confirmation holds before it collapses. */
const CONFIRM_MS = 4000;

export type Tier = 0 | 1 | 2 | 3;
export type IssueKind = "connection" | "run" | "session";
export type IssueAction = "retry" | "runAgain" | "signIn";

export interface StatusIssue {
  kind: IssueKind;
  tier: Tier;
  /** The fact, then the consequence. No apology, no error code. */
  message: string;
  /** Small print beside the actions: what is still true. */
  detail?: string;
  /** The long form, behind `Details`. */
  more?: string;
  actions: IssueAction[];
}

export interface ServerStatus {
  /** The bar to show. Absent when the app is healthy: no bar is the signal. */
  issue: StatusIssue | undefined;
  /** Live issues the bar is not showing; they stack behind `Details`. */
  others: StatusIssue[];
  /** Writes are being refused, so add and delete cannot honestly be offered. */
  offline: boolean;
  /** Epoch ms of the last answer the server actually gave. */
  lastContact: number | undefined;
  /** Consecutive failed requests and probes. */
  attempt: number;
  /** Seconds until the next automatic probe, ticking while a bar shows it. */
  retryIn: number | undefined;
  /** Shown for four seconds on the way back to healthy. */
  confirmation: { message: string; detail?: string } | undefined;
}

const INITIAL: ServerStatus = {
  issue: undefined,
  others: [],
  offline: false,
  lastContact: undefined,
  attempt: 0,
  retryIn: undefined,
  confirmation: undefined,
};

let status: ServerStatus = INITIAL;

const listeners = new Set<() => void>();

// The three independent sources of trouble. Each replaces its own kind rather
// than stacking, so one flapping connection cannot bury a failed run.
let attempt = 0;
let lastContact: number | undefined;
let refusedWrites = 0;
/** A write has been refused, so "changes will not save" is already true. */
let sawRefusedWrite = false;
let runIssue: StatusIssue | undefined;
let sessionExpired = false;
let sessionEstablished = false;

let probeTimer: ReturnType<typeof setTimeout> | undefined;
let confirmTimer: ReturnType<typeof setTimeout> | undefined;
let countdownTimer: ReturnType<typeof setInterval> | undefined;
let confirmation: ServerStatus["confirmation"];
let nextProbeAt: number | undefined;

function clockOf(at: number): string {
  return new Date(at).toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function connectionTier(): Tier {
  if (attempt === 0) return 0;
  if (sawRefusedWrite || attempt >= OFFLINE_AFTER) return 2;
  return attempt >= NOTICE_AFTER ? 1 : 0;
}

function connectionIssue(): StatusIssue | undefined {
  const tier = connectionTier();
  if (tier === 0) return undefined;
  if (tier === 1) {
    return {
      kind: "connection",
      tier: 1,
      message: "Reconnecting to the server.",
      detail: `attempt ${attempt}`,
      actions: [],
    };
  }
  return {
    kind: "connection",
    tier: 2,
    message: "No connection to the server. You can keep reading; changes will not save.",
    detail: lastContact ? `last contact ${clockOf(lastContact)}` : undefined,
    more:
      refusedWrites > 0
        ? `${refusedWrites} ${refusedWrites === 1 ? "change" : "changes"} could not be saved while the server was unreachable. Your values are still in the fields that hold them.`
        : undefined,
    actions: ["retry"],
  };
}

function sessionIssue(): StatusIssue | undefined {
  if (!sessionExpired) return undefined;
  return {
    kind: "session",
    tier: 3,
    message: "Your session expired. Sign in again to save anything further.",
    actions: ["signIn"],
  };
}

function publish() {
  // One bar at a time: the gravest live issue wins, the rest stack behind
  // Details. Ties go to the newest, which is why the session — always the most
  // recent thing to have happened when it happens — is listed first.
  const live = [sessionIssue(), connectionIssue(), runIssue].filter(
    (i): i is StatusIssue => i != null,
  );
  live.sort((a, b) => b.tier - a.tier);

  status = {
    issue: live[0],
    others: live.slice(1),
    offline: connectionTier() >= 2 || sessionExpired,
    lastContact,
    attempt,
    retryIn:
      nextProbeAt == null ? undefined : Math.max(0, Math.ceil((nextProbeAt - Date.now()) / 1000)),
    confirmation,
  };
  for (const listener of listeners) listener();
}

function scheduleProbe() {
  if (probeTimer) clearTimeout(probeTimer);
  const delay = BACKOFF_MS[Math.min(attempt - 1, BACKOFF_MS.length - 1)];
  nextProbeAt = Date.now() + delay;
  probeTimer = setTimeout(probe, delay);

  // The countdown is the only moving part of the notice, so it ticks here —
  // outside React, and only while there is a bar to show it in.
  stopCountdown();
  if (connectionTier() >= 1) countdownTimer = setInterval(publish, 1000);
}

function stopCountdown() {
  if (countdownTimer) clearInterval(countdownTimer);
  countdownTimer = undefined;
}

/**
 * Ask the server whether it is there yet.
 *
 * Deliberately raw `fetch` rather than the API client: a probe that reported
 * its own outcome back through `http` would recurse.
 */
async function probe() {
  probeTimer = undefined;
  nextProbeAt = undefined;
  stopCountdown();
  publish();
  try {
    const response = await fetch("/api/health", { cache: "no-store" });
    if (!response.ok) throw new Error(String(response.status));
    reached();
  } catch {
    failed("read");
  }
}

/** The server answered. Any answer at all means the connection is alive. */
function reached() {
  const wasVisible = connectionTier() >= 1;
  lastContact = Date.now();

  // The common case, called on every successful request — and the run poller
  // makes one twice a second. Nothing observable changed, so nothing is
  // published: subscribers are not re-rendered for a healthy heartbeat.
  if (attempt === 0 && !sawRefusedWrite) return;

  if (probeTimer) clearTimeout(probeTimer);
  probeTimer = undefined;
  nextProbeAt = undefined;
  stopCountdown();
  attempt = 0;
  sawRefusedWrite = false;

  if (wasVisible) {
    // The bar confirms once, holds, then collapses. A healthy app shows no
    // status at all.
    confirmation = {
      message: "Connected.",
      detail:
        refusedWrites > 0
          ? `${refusedWrites} ${refusedWrites === 1 ? "change was" : "changes were"} not saved`
          : undefined,
    };
    if (confirmTimer) clearTimeout(confirmTimer);
    confirmTimer = setTimeout(() => {
      confirmation = undefined;
      publish();
    }, CONFIRM_MS);
  }
  refusedWrites = 0;
  publish();
}

/** The request never arrived, or the server answered that it is unwell. */
function failed(kind: "read" | "write") {
  attempt += 1;
  if (kind === "write") {
    refusedWrites += 1;
    sawRefusedWrite = true;
  }
  scheduleProbe();
  publish();
}

export const serverMonitor = {
  subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },

  snapshot(): ServerStatus {
    return status;
  },

  /** Hydration reads this: nothing can have failed before the app mounts. */
  initial(): ServerStatus {
    return INITIAL;
  },

  reached,
  failed,

  /** Retry now, rather than waiting out the backoff. */
  retryNow() {
    if (probeTimer) clearTimeout(probeTimer);
    probeTimer = undefined;
    stopCountdown();
    void probe();
  },

  /** A 401 only means "expired" once a session existed to expire. */
  expireSession() {
    if (!sessionEstablished || sessionExpired) return;
    sessionExpired = true;
    publish();
  },

  sessionStarted() {
    sessionEstablished = true;
    if (sessionExpired) {
      sessionExpired = false;
      publish();
    }
  },

  sessionEnded() {
    sessionEstablished = false;
    if (sessionExpired) {
      sessionExpired = false;
      publish();
    }
  },

  /**
   * A run that stopped short. `hasResults` decides whether the bar can promise
   * that what is on screen is the last complete run, or only that it is stale.
   */
  runFailed(run: {
    completed: number;
    total: number;
    message: string | null;
    hasResults: boolean;
  }) {
    const stopped =
      run.completed > 0
        ? `The simulation stopped after ${run.completed.toLocaleString("en-US")} of ${run.total.toLocaleString("en-US")} iterations.`
        : "The simulation did not start.";
    runIssue = {
      kind: "run",
      tier: 2,
      message: `${stopped} ${run.hasResults
          ? "Results below are from the last complete run."
          : "There are no results to show yet."
        }`,
      more: run.message ?? undefined,
      actions: ["runAgain"],
    };
    publish();
  },

  runCleared() {
    if (!runIssue) return;
    runIssue = undefined;
    publish();
  },
};
