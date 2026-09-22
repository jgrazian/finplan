import type { Entitlements } from "../api/generated/Entitlements";

export const BETA_ACCESS_NOTICE =
  "Feedback beta · Free access during the beta. Compute limits apply. Future plans and pricing may change.";

/** Deployment policy and feature access are distinct: beta access is not a paid subscription. */
export function accessPresentation(access: Pick<Entitlements, "access_mode" | "pro">) {
  switch (access.access_mode) {
    case "self_hosted":
      return {
        label: "Self-hosted access",
        description: "All planning features are available. This instance does not enforce subscription limits or collect payments.",
        showBilling: false,
      };
    case "beta":
      return {
        label: "Feedback beta",
        description: "Free access during the beta, including saved plans, comparisons, run history, reports, and advanced analysis. Future plans and pricing may change.",
        showBilling: false,
      };
    case "subscription":
      return {
        label: access.pro ? "Pro" : "Free",
        description: access.pro
          ? "Unlimited saved plans with comparisons, run history, reports, and advanced analysis."
          : "The complete planning model, one editable saved plan, and one goal seek per month.",
        showBilling: true,
      };
  }
}
