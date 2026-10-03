"use client";

import { useCallback, useMemo } from "react";
import type { Scenario } from "@/lib/api/types";
import { planApiFor } from "@/lib/nav/api";
import { mergePlanLists } from "@/lib/nav/merge";
import type { PlanHome } from "@/lib/nav/url";
import { useAsync } from "./useAsync";

export interface PlanList {
  /** Both homes' plans, merged; undefined until every home asked has answered or failed. */
  data: Scenario[] | undefined;
  /** Set only when no home could answer. One home down leaves the other's plans listed. */
  error: Error | undefined;
  /** Homes that were asked and failed, so a screen can say some plans are missing. */
  failed: PlanHome[];
  reload: () => void;
}

/**
 * The scenario list across homes: local plans when local mode is on, cloud
 * plans when the session has a server to ask.
 *
 * Waits for every home asked before it offers anything. The workbench resolves
 * `?scenario=` against this list and rewrites the address to what it found, so
 * answering early with one home would send a link to a plan in the other to the
 * first plan it happened to see.
 */
export function usePlanList({ local, cloud }: { local: boolean; cloud: boolean }): PlanList {
  const localList = useAsync(
    async () => (local ? planApiFor("local").scenarios.list() : []),
    [local],
  );
  const cloudList = useAsync(
    async () => (cloud ? planApiFor("cloud").scenarios.list() : []),
    [cloud],
  );
  const reloadLocal = localList.reload;
  const reloadCloud = cloudList.reload;

  const reload = useCallback(() => {
    reloadLocal();
    reloadCloud();
  }, [reloadLocal, reloadCloud]);

  return useMemo(() => {
    const answered = (list: { data?: Scenario[]; error?: Error }) =>
      list.data !== undefined || list.error !== undefined;
    const ready = (!local || answered(localList)) && (!cloud || answered(cloudList));
    const merged = mergePlanLists<Scenario>({
      local: local ? { rows: localList.data, error: localList.error } : undefined,
      cloud: cloud ? { rows: cloudList.data, error: cloudList.error } : undefined,
    });
    return {
      data: ready && !merged.error ? merged.rows : undefined,
      error: merged.error,
      failed: ready ? merged.failed : [],
      reload,
    };
  }, [local, cloud, localList, cloudList, reload]);
}
