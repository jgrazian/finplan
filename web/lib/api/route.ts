/**
 * Picking the home that answers for a plan.
 *
 * The two implementations are passed in rather than imported, so this module
 * reaches neither `fetch` nor the store worker, and the routing is tested with
 * stand-ins. `lib/nav/api.ts` binds the real `remoteApi` and `localApi`.
 */
import { homeOf, type PlanHome } from "../nav/url.ts";
import type { PlanApi } from "./plan.ts";

/** What names a home: the home itself, or a plan ref (`l12`, `s8a4f21b7c903`) whose prefix does. */
export type PlanTarget = PlanHome | string | undefined;

/** The home a target names; nothing named is the cloud's, as it was before local plans. */
export function homeOfTarget(target: PlanTarget): PlanHome {
  return target === "local" || target === "cloud" ? target : homeOf(target);
}

export function createPlanRouter(homes: Record<PlanHome, PlanApi>) {
  return (target?: PlanTarget): PlanApi => homes[homeOfTarget(target)];
}
