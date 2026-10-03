"use client";

import { createContext, useContext, type ReactNode } from "react";
import type { PlanApi } from "../api/plan";
import { planApiFor } from "./api";
import { useNav } from "./context";

/**
 * The ref of the plan the screens are showing, when that is not what the query
 * names. `scenario=` is absent on a fresh visit and stale after a deletion, and
 * the workbench resolves it to a real plan — maybe a local one — only once the
 * list has loaded. Everything below reads the resolved ref, so a plan is never
 * fetched from the wrong home for the render or two before the URL catches up.
 */
const OpenPlanContext = createContext<string | undefined>(undefined);

export function OpenPlanProvider({
  planRef,
  children,
}: {
  planRef: string | undefined;
  children: ReactNode;
}) {
  return <OpenPlanContext.Provider value={planRef}>{children}</OpenPlanContext.Provider>;
}

/** The ref of the open plan: the resolved one when a workbench provides it, else the query's. */
export function useOpenPlanRef(): string | undefined {
  const provided = useContext(OpenPlanContext);
  const nav = useNav();
  return provided ?? nav.scenario;
}

/**
 * The `PlanApi` for the open plan. Components call plan routes through this and
 * keep taking numeric ids; which home answers is decided here, once.
 */
export function usePlanApi(): PlanApi {
  return planApiFor(useOpenPlanRef());
}
