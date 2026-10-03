import { homeOfTarget, type PlanTarget } from "../api/route.ts";
import type { PlanHome } from "./url.ts";

/** Shown wherever an AI feature is replaced by its locked state on a local plan. */
export const MOVE_TO_CLOUD_FOR_AI = "Move to cloud to use AI";

/** What a plan's home can do, for screens to gate on rather than to call and be refused. */
export interface PlanCapabilities {
  home: PlanHome;
  /**
   * AI review, plan chat and drafts. They run on the server against a stored
   * plan, so only a cloud plan has them (the tier still decides how many).
   */
  ai: boolean;
  /** Running on FinPlan's servers instead of this device: it costs compute, so it needs an account. */
  offload: boolean;
  /**
   * Goal seeks are metered. The server meters them by tier; a local run is on
   * the visitor's own CPU, where a cap in client code would be decoration.
   */
  goalSeekQuota: boolean;
  /** Why `ai` is off, in words for the locked state. Present exactly when it is. */
  cloudOnlyReason?: string;
}

/** The parts of the session a plan's capabilities depend on. */
export interface CapabilityContext {
  /** Signed in to an account, which a guest is not. */
  account: boolean;
}

/**
 * The capabilities of the plan `target` names — a ref, or a home for a plan
 * not made yet. Pure, so the table of what each home can do is checked without
 * a render; `usePlanCapabilities` supplies the session.
 */
export function planCapabilities(
  target: PlanTarget,
  { account }: CapabilityContext,
): PlanCapabilities {
  const home = homeOfTarget(target);
  if (home === "local") {
    return {
      home,
      ai: false,
      offload: account,
      goalSeekQuota: false,
      cloudOnlyReason: MOVE_TO_CLOUD_FOR_AI,
    };
  }
  // A cloud plan already runs on the server: there is nothing to offload to.
  return { home, ai: true, offload: false, goalSeekQuota: true };
}
