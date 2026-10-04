"use client";

import { useMemo } from "react";
import { planApiFor } from "@/lib/nav/api";
import { homeOf } from "@/lib/nav/url";
import type {
  Account as ApiAccount,
  Asset,
  Event as ApiEvent,
  NamedParameter,
  Profile,
  Scenario as ApiScenario,
} from "@/lib/api/types";
import type {
  Account,
  AssumptionChoices,
  InflationProfile,
  PlanEvent,
  ReturnProfile,
  ScenarioParams,
} from "@/lib/types";
import { type PlanAxis, planAxis } from "@/lib/view/axis";
import { toViewAccounts } from "@/lib/view/accounts";
import { toInflationChoices, toTaxChoices } from "@/lib/view/assumptions";
import { namesOf, toViewEvents } from "@/lib/view/events";
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
  parameters: NamedParameter[];
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
  /** What the Plan strip's inflation and tax pickers can be set to. */
  assumptions: AssumptionChoices;
  /** Name of the scenario's inflation profile, for the Profiles screen. */
  activeInflationProfile: string | undefined;
  raw: RawWorkspace;
  loading: boolean;
  error: Error | undefined;
  reload: () => void;
}

/**
 * Everything the Portfolio and Plan screens read, for one scenario.
 *
 * `planRef` names the plan's home (`l12`, or a cloud slug), which also owns the
 * profile and tax libraries read alongside it. Ids are only unique within a
 * home, so the home is part of what a loaded workspace is keyed on.
 */
export function useWorkspace(scenarioId: number | undefined, planRef: string | undefined): Workspace {
  const home = homeOf(planRef);
  const { data, error, loading, reload } = useAsync(async () => {
    if (scenarioId == null) return undefined;
    const api = planApiFor(home);
    // Independent reads; one round trip's latency rather than seven.
    const [scenario, accounts, assets, events, parameters, returnProfiles, inflationProfiles, taxConfigs] =
      await Promise.all([
        api.scenarios.get(scenarioId),
        api.accounts.list(scenarioId),
        api.assets.list(scenarioId),
        api.events.list(scenarioId),
        api.parameters.list(scenarioId),
        api.returnProfiles.list(),
        api.inflationProfiles.list(),
        api.taxConfigs.list(),
      ]);
    return { home, scenario, accounts, assets, events, parameters, returnProfiles, inflationProfiles, taxConfigs };
  }, [scenarioId, home]);

  return useMemo(() => {
    if (!data || data.home !== home || data.scenario.id !== scenarioId) {
      return {
        scenario: undefined,
        params: undefined,
        axis: undefined,
        accounts: [],
        events: [],
        returnProfiles: [],
        inflationProfiles: [],
        assumptions: { inflation: [], tax: [] },
        activeInflationProfile: undefined,
        raw: { accounts: [], assets: [], events: [], parameters: [], returnProfiles: [] },
        loading,
        error,
        reload,
      };
    }

    const { scenario, accounts, assets, events, parameters, returnProfiles, inflationProfiles, taxConfigs } =
      data;
    const axis = planAxis(scenario);

    const names = namesOf({ accounts, assets, events, parameters });

    const inflation = inflationProfiles.find((p) => p.id === scenario.inflation_profile_id);
    const taxConfig = taxConfigs.find((t) => t.id === scenario.tax_config_id);
    const viewInflation = toViewInflationProfiles(inflationProfiles);

    return {
      scenario,
      params: {
        name: scenario.name,
        start: scenario.start_date,
        durationYears: scenario.duration_years,
        birthDate: scenario.birth_date ?? "",
        inflationProfile: inflation?.name ?? "—",
        inflationProfileId: scenario.inflation_profile_id,
        taxConfig: taxConfig?.name ?? "—",
        taxConfigId: scenario.tax_config_id,
        deferredTaxRate: scenario.deferred_tax_rate,
      },
      axis,
      accounts: toViewAccounts(accounts, { assets, profiles: returnProfiles, events }),
      events: toViewEvents(events, scenario, axis, names),
      returnProfiles: toViewReturnProfiles(returnProfiles),
      inflationProfiles: viewInflation,
      assumptions: {
        inflation: toInflationChoices(viewInflation),
        tax: toTaxChoices(taxConfigs),
      },
      activeInflationProfile: inflation?.name,
      raw: { accounts, assets, events, parameters, returnProfiles },
      loading,
      error,
      reload,
    };
  }, [data, loading, error, reload, scenarioId, home]);
}
