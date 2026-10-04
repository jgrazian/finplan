/**
 * What to do about a local run, given how long this device says it will take
 * (spec 19, "Performance and run estimates"). Pure, so the thresholds are
 * tested without a render.
 */
import type { RunEstimate } from "./runtime.ts";

/** Under this, just run. */
export const QUICK_RUN_SECONDS = 10;
/** Over this, the server is offered. Between the two the run starts with its estimate showing. */
export const SLOW_RUN_SECONDS = 60;
/**
 * How much faster FinPlan's servers are taken to be than a browser on this
 * device, for the "about N s" in the offer. A guess, and labelled "about": the
 * server cannot be asked before the plan is sent, and the plan is not sent
 * until the person says so.
 */
export const SERVER_SPEEDUP = 8;
const MIN_SERVER_SECONDS = 2;

export type RunChoice =
  /** Run here; nothing to say. */
  | { kind: "run" }
  /** Run here, with the estimate and a Cancel button on the progress bar. */
  | { kind: "run-with-estimate"; seconds: number }
  /** Ask: the server, or here anyway. */
  | {
      kind: "offer-offload";
      seconds: number;
      serverSeconds: number;
      reason: "slow" | "constrained";
    };

/**
 * The spec: under 10 s run; 10–60 s run and show the estimate; over 60 s, or a
 * constrained device (two cores or fewer, low memory), offer the server.
 *
 * A constrained device is offered the server only once the run is no longer
 * trivial. Offering on every run of a two-core laptop would put a question in
 * front of a half-second run, and the constraint matters when the run is big.
 */
export function decideRun(estimate: Pick<RunEstimate, "seconds" | "constrained">): RunChoice {
  const seconds = Number.isFinite(estimate.seconds) ? Math.max(0, estimate.seconds) : 0;
  if (seconds > SLOW_RUN_SECONDS) {
    return { kind: "offer-offload", seconds, serverSeconds: serverSeconds(seconds), reason: "slow" };
  }
  if (seconds >= QUICK_RUN_SECONDS) {
    if (estimate.constrained) {
      return {
        kind: "offer-offload",
        seconds,
        serverSeconds: serverSeconds(seconds),
        reason: "constrained",
      };
    }
    return { kind: "run-with-estimate", seconds };
  }
  return { kind: "run" };
}

/** The server's side of the offer, from this device's estimate. */
export function serverSeconds(localSeconds: number): number {
  return Math.max(MIN_SERVER_SECONDS, Math.round(localSeconds / SERVER_SPEEDUP));
}

/** "25 s", "2 min": a duration as people say it. */
export function formatDuration(seconds: number): string {
  const rounded = Math.max(1, Math.round(seconds));
  if (rounded < 90) return `${rounded} s`;
  const minutes = Math.round(rounded / 60);
  if (minutes < 90) return `${minutes} min`;
  return `${Math.round(minutes / 6) / 10} h`;
}

/** What the budget route says, narrowed to what the offer needs. */
export interface OffloadBudgetView {
  available: boolean;
  unavailable_reason: "guest" | "budget_spent" | null;
  resets_at: string;
}

export type OffloadAvailability =
  | { available: true }
  | { available: false; reason: string; signIn?: boolean };

const NEEDS_ACCOUNT = "Running on FinPlan's servers needs an account. Sign in to use it.";

/**
 * Whether the server can take this run, and when not, why in words for the
 * offer. `account` is signed in and not a guest; `budget` is `undefined` until
 * `GET /compute/budget` has answered (or when it could not).
 */
export function offloadAvailability(
  account: boolean,
  budget: OffloadBudgetView | undefined,
): OffloadAvailability {
  if (!account) return { available: false, reason: NEEDS_ACCOUNT, signIn: true };
  if (!budget) {
    return { available: false, reason: "Could not check your server budget. Try again in a moment." };
  }
  if (budget.available) return { available: true };
  if (budget.unavailable_reason === "guest") {
    return { available: false, reason: NEEDS_ACCOUNT, signIn: true };
  }
  return {
    available: false,
    reason: `Your server run budget for this month is spent. It resets ${resetDate(budget.resets_at)}.`,
  };
}

/** "October 1": the day a budget resets. The server resets at 00:00 UTC, so the day is read in UTC. */
export function resetDate(resetsAt: string): string {
  const at = new Date(resetsAt);
  if (Number.isNaN(at.getTime())) return "next month";
  return at.toLocaleDateString("en-US", { month: "long", day: "numeric", timeZone: "UTC" });
}
