import { http } from "./http";
import type { Entitlements } from "./generated/Entitlements";

/**
 * Billing's view of what this account may do. Not plan data, so it stays on the
 * server: a plan's runs, inputs and reports go through its `PlanApi`.
 */
export const historyApi = {
  entitlements: () => http.get<Entitlements>("/billing/entitlements"),
};
