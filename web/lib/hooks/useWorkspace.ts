"use client";

import { useMemo } from "react";
import { api } from "@/lib/api/client";
import type {
  Account as ApiAccount,
  Asset,
  Event as ApiEvent,
  Profile,
  Scenario as ApiScenario,
} from "@/lib/api/types";
import type {
  Account,
  InflationProfile,
  PlanEvent,
  ReturnProfile,
  ScenarioParams,
} from "@/lib/types";
import { type PlanAxis, planAxis } from "@/lib/view/axis";
import { toViewAccounts } from "@/lib/view/accounts";
import { toViewEvents } from "@/lib/view/events";
import { toViewInflationProfiles, toViewReturnProfiles } from "@/lib/view/profiles";
import { useAsync } from "./useAsync";

/**
 * The API's own rows, alongside the view models.
 *
 * The tables read view models, but the create/delete dialogs work in ids and
 * request bodies, so they need the rows the server actually returned.
 */
export interface RawWorkspace {
  accounts: ApiAccount[];
  assets: Asset[];
  events: ApiEvent[];
  returnProfiles: Profile[];
}

export interface Workspace {
  scenario: ApiScenario | undefined;
  params: ScenarioParams | undefined;
  axis: PlanAxis | undefined;
  accounts: Account[];
  events: PlanEvent[];
  returnProfiles: ReturnProfile[];
  inflationProfiles: InflationProfile[];
  /** Name of the scenario's inflation profile, for the Profiles screen. */
  activeInflationProfile: string | undefined;
  raw: RawWorkspace;
  loading: boolean;
  error: Error | undefined;
  reload: () => void;
}

/** Everything the Portfolio and Plan screens read, for one scenario. */
export function useWorkspace(scenarioId: number | undefined, iterations: number): Workspace {
  const { data, error, loading, reload } = useAsync(async () => {
    if (scenarioId == null) return undefined;
    // Independent reads; one round trip's latency rather than seven.
    const [scenario, accounts, assets, events, returnProfiles, inflationProfiles, taxConfigs] =
      await Promise.all([
        api.scenarios.get(scenarioId),
        api.accounts.list(scenarioId),
        api.assets.list(scenarioId),
        api.events.list(scenarioId),
        api.returnProfiles.list(),
        api.inflationProfiles.list(),
        api.taxConfigs.list(),
      ]);
    return { scenario, accounts, assets, events, returnProfiles, inflationProfiles, taxConfigs };
  }, [scenarioId]);

  return useMemo(() => {
    if (!data) {
      return {
        scenario: undefined,
        params: undefined,
        axis: undefined,
        accounts: [],
        events: [],
        returnProfiles: [],
        inflationProfiles: [],
        activeInflationProfile: undefined,
        raw: { accounts: [], assets: [], events: [], returnProfiles: [] },
        loading,
        error,
        reload,
      };
    }

    const { scenario, accounts, assets, events, returnProfiles, inflationProfiles, taxConfigs } =
      data;
    const axis = planAxis(scenario);

    const assetName = new Map(assets.map((a) => [a.id, a.name]));
    const accountName = new Map(accounts.map((a) => [a.id, a.name]));
    const eventName = new Map(events.map((e) => [e.id, e.name]));
    const names = {
      account: (id: number) => accountName.get(id) ?? `account ${id}`,
      asset: (id: number) => assetName.get(id) ?? `asset ${id}`,
      event: (id: number) => eventName.get(id) ?? `event ${id}`,
    };

    const inflation = inflationProfiles.find((p) => p.id === scenario.inflation_profile_id);
    const taxConfig = taxConfigs.find((t) => t.id === scenario.tax_config_id);

    return {
      scenario,
      params: {
        start: scenario.start_date,
        durationYears: scenario.duration_years,
        birthDate: scenario.birth_date ?? "",
        iterations,
        inflationProfile: inflation?.name ?? "—",
        taxConfig: taxConfig?.name ?? "—",
      },
      axis,
      accounts: toViewAccounts(accounts, { assets, profiles: returnProfiles, events }),
      events: toViewEvents(events, scenario, axis, names),
      returnProfiles: toViewReturnProfiles(returnProfiles),
      inflationProfiles: toViewInflationProfiles(inflationProfiles),
      activeInflationProfile: inflation?.name,
      raw: { accounts, assets, events, returnProfiles },
      loading,
      error,
      reload,
    };
  }, [data, iterations, loading, error, reload]);
}
