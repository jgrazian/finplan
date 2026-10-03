"use client";

import { useMemo } from "react";
import { useGuest } from "@/components/auth/GuestContext";
import { type PlanCapabilities, planCapabilities } from "@/lib/nav/capabilities";
import { useOpenPlanRef } from "@/lib/nav/plan";

/**
 * What the home of `ref` supports — the open plan when no ref is given.
 *
 * A guest has no account, so the session decides `offload`: a workbench only
 * renders for a signed-in user or a guest, and the guest flag is what
 * separates them.
 */
export function usePlanCapabilities(ref?: string): PlanCapabilities {
  const open = useOpenPlanRef();
  const { guest } = useGuest();
  const target = ref ?? open;
  return useMemo(() => planCapabilities(target, { account: !guest }), [target, guest]);
}
