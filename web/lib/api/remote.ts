/**
 * The plan-shaped routes of `finplan_server`, as the cloud implementation of
 * `PlanApi`: the home of a plan that lives on the server.
 *
 * Grouped by resource so a caller reads as `remoteApi.runs.results(id)`; screens
 * reach it through `planApiFor`/`usePlanApi` rather than by name. Paths mirror
 * `crates/finplan_server/src/api/mod.rs`.
 */
import type { PlanApi } from "./plan";
import { http } from "./http";
import type { ArchiveImported } from "./generated/ArchiveImported";
import type { ArchivePreview } from "./generated/ArchivePreview";
import type { ImportArchive } from "./generated/ImportArchive";
import type { PlanArchive } from "./generated/PlanArchive";
import type { RunComparison } from "./generated/RunComparison";
import type { RunInputs } from "./generated/RunInputs";
import type { RunReport } from "./generated/RunReport";
import type { SetupCreated } from "./generated/SetupCreated";
import type { SetupPlan } from "./generated/SetupPlan";
import type {
  Account,
  ApplyWhatIf,
  Analysis,
  AnalysisOutcome,
  AnalysisParameter,
  Asset,
  CachedSweep,
  CompileReport,
  CreateAccount,
  CreateAnalysis,
  CreateAsset,
  CreatePosition,
  CreateProfile,
  CreateRun,
  CreateScenario,
  CreateTaxConfig,
  Event,
  EventBody,
  NamedParameter,
  ParameterBody,
  ExpressionValidation,
  ExpressionValidationRequest,
  HistoryPreset,
  LedgerPage,
  LedgerQuery,
  PreflightReport,
  Position,
  Profile,
  QuickWhatIf,
  ReorderRequest,
  Results,
  Run,
  Scenario,
  TaxConfig,
  UpdateAccountBody,
  UpdateAsset,
  UpdatePosition,
  UpdateProfile,
  UpdateScenario,
  UpdateTaxConfig,
  WhatIfOutcome,
  WhatIfStack,
} from "./types";

const scenario = (id: number) => `/scenarios/${id}`;

export const remoteApi: PlanApi = {
  /** Whole-home plan archives; also what carries a guest's plan across a sign-in. */
  archives: {
    exportAll: () => http.get<PlanArchive>("/archives"),
    preview: (archive: PlanArchive) => http.post<ArchivePreview>("/archives/preview", archive),
    /** `from_guest` marks an adoption, so the server can count it. */
    import: (body: ImportArchive) => http.post<ArchiveImported>("/archives/import", body),
  },

  scenarios: {
    list: () => http.get<Scenario[]>("/scenarios"),
    get: (id: number) => http.get<Scenario>(scenario(id)),
    create: (body: CreateScenario) => http.post<Scenario>("/scenarios", body),
    update: (id: number, body: UpdateScenario) => http.patch<Scenario>(scenario(id), body),
    remove: (id: number) => http.delete(scenario(id)),
    duplicate: (id: number, name: string) =>
      http.post<Scenario>(`${scenario(id)}/duplicate`, { name }),
    /** Lower the scenario without running it, to surface config errors early. */
    compile: (id: number) => http.post<CompileReport>(`${scenario(id)}/compile`),
    /** What would stop the next run, and what is worth a second look. */
    preflight: (id: number) => http.get<PreflightReport>(`${scenario(id)}/preflight`),
    setup: (body: SetupPlan) => http.post<SetupCreated>("/scenarios/setup", body),
    inputHash: (id: number) => http.get<{ input_hash: string }>(`${scenario(id)}/input-hash`),
    archive: (id: number) => http.get<PlanArchive>(`${scenario(id)}/archive`),
  },

  assets: {
    list: (scenarioId: number) => http.get<Asset[]>(`${scenario(scenarioId)}/assets`),
    create: (scenarioId: number, body: CreateAsset) =>
      http.post<Asset>(`${scenario(scenarioId)}/assets`, body),
    update: (scenarioId: number, id: number, body: UpdateAsset) =>
      http.patch<Asset>(`${scenario(scenarioId)}/assets/${id}`, body),
    remove: (scenarioId: number, id: number) =>
      http.delete(`${scenario(scenarioId)}/assets/${id}`),
    /** One write for the whole list, rather than a PATCH per row moved. */
    reorder: (scenarioId: number, ids: ReorderRequest["ids"]) =>
      http.post<void>(`${scenario(scenarioId)}/assets/reorder`, { ids }),
  },

  accounts: {
    list: (scenarioId: number) => http.get<Account[]>(`${scenario(scenarioId)}/accounts`),
    create: (scenarioId: number, body: CreateAccount) =>
      http.post<Account>(`${scenario(scenarioId)}/accounts`, body),
    update: (scenarioId: number, id: number, body: UpdateAccountBody) =>
      http.patch<Account>(`${scenario(scenarioId)}/accounts/${id}`, body),
    remove: (scenarioId: number, id: number) =>
      http.delete(`${scenario(scenarioId)}/accounts/${id}`),
    reorder: (scenarioId: number, ids: ReorderRequest["ids"]) =>
      http.post<void>(`${scenario(scenarioId)}/accounts/reorder`, { ids }),
    positions: (scenarioId: number, id: number) =>
      http.get<Position[]>(`${scenario(scenarioId)}/accounts/${id}/positions`),
    addPosition: (scenarioId: number, id: number, body: CreatePosition) =>
      http.post<Position>(`${scenario(scenarioId)}/accounts/${id}/positions`, body),
    updatePosition: (
      scenarioId: number,
      id: number,
      positionId: number,
      body: UpdatePosition,
    ) =>
      http.patch<Position>(
        `${scenario(scenarioId)}/accounts/${id}/positions/${positionId}`,
        body,
      ),
    removePosition: (scenarioId: number, id: number, positionId: number) =>
      http.delete(`${scenario(scenarioId)}/accounts/${id}/positions/${positionId}`),
    reorderPositions: (scenarioId: number, id: number, ids: ReorderRequest["ids"]) =>
      http.post<void>(`${scenario(scenarioId)}/accounts/${id}/positions/reorder`, { ids }),
  },

  events: {
    list: (scenarioId: number) => http.get<Event[]>(`${scenario(scenarioId)}/events`),
    create: (scenarioId: number, body: EventBody) =>
      http.post<Event>(`${scenario(scenarioId)}/events`, body),
    /** PUT, not PATCH: a trigger is a tree and partial merges have no meaning. */
    replace: (scenarioId: number, id: number, body: EventBody) =>
      http.put<Event>(`${scenario(scenarioId)}/events/${id}`, body),
    remove: (scenarioId: number, id: number) =>
      http.delete(`${scenario(scenarioId)}/events/${id}`),
    /** Presentation only: an event fires on its trigger, not on its place. */
    reorder: (scenarioId: number, ids: ReorderRequest["ids"]) =>
      http.post<void>(`${scenario(scenarioId)}/events/reorder`, { ids }),
  },

  parameters: {
    list: (scenarioId: number) =>
      http.get<NamedParameter[]>(`${scenario(scenarioId)}/parameters`),
    create: (scenarioId: number, body: ParameterBody) =>
      http.post<NamedParameter>(`${scenario(scenarioId)}/parameters`, body),
    update: (scenarioId: number, id: number, body: ParameterBody) =>
      http.patch<NamedParameter>(`${scenario(scenarioId)}/parameters/${id}`, body),
    remove: (scenarioId: number, id: number) =>
      http.delete(`${scenario(scenarioId)}/parameters/${id}`),
  },

  expressions: {
    validate: (scenarioId: number, body: ExpressionValidationRequest) =>
      http.post<ExpressionValidation>(`${scenario(scenarioId)}/expressions/validate`, body),
  },

  returnProfiles: {
    list: () => http.get<Profile[]>("/return-profiles"),
    create: (body: CreateProfile) => http.post<Profile>("/return-profiles", body),
    update: (id: number, body: UpdateProfile) =>
      http.patch<Profile>(`/return-profiles/${id}`, body),
    remove: (id: number) => http.delete(`/return-profiles/${id}`),
    /** The library is the user's, so this reorders it for every scenario. */
    reorder: (ids: ReorderRequest["ids"]) =>
      http.post<void>("/return-profiles/reorder", { ids }),
  },

  inflationProfiles: {
    list: () => http.get<Profile[]>("/inflation-profiles"),
    create: (body: CreateProfile) => http.post<Profile>("/inflation-profiles", body),
    remove: (id: number) => http.delete(`/inflation-profiles/${id}`),
    reorder: (ids: ReorderRequest["ids"]) =>
      http.post<void>("/inflation-profiles/reorder", { ids }),
  },

  /** The bootstrap histories the engine ships with, series and all. */
  historyPresets: () => http.get<HistoryPreset[]>("/history-presets"),

  taxConfigs: {
    list: () => http.get<TaxConfig[]>("/tax-configs"),
    create: (body: CreateTaxConfig) => http.post<TaxConfig>("/tax-configs", body),
    update: (id: number, body: UpdateTaxConfig) =>
      http.patch<TaxConfig>(`/tax-configs/${id}`, body),
    remove: (id: number) => http.delete(`/tax-configs/${id}`),
  },

  /**
   * Sweeps, sensitivity rankings and goal seeks. One route family: POST to
   * ask, GET to poll, GET results when it says `succeeded`.
   *
   * Unlike runs these are not persisted, so an id outlives only the server
   * process that issued it. Nothing here should be bookmarked.
   */
  analysis: {
    /** What this plan can vary, and the range each axis defaults to. */
    parameters: (scenarioId: number) =>
      http.get<AnalysisParameter[]>(`${scenario(scenarioId)}/analysis/parameters`),
    start: (scenarioId: number, body: CreateAnalysis) =>
      http.post<Analysis>(`${scenario(scenarioId)}/analyses`, body),
    get: (id: number) => http.get<Analysis>(`/analyses/${id}`),
    cancel: (id: number) => http.post<Analysis>(`/analyses/${id}/cancel`),
    /** Refused until the job reaches `succeeded`. */
    results: (id: number) => http.get<AnalysisOutcome>(`/analyses/${id}/results`),
    /**
     * The scenario's most recent sweep, kept server-side so a reload finds the
     * grid again. `null` before anything has been swept. Keyed by scenario
     * rather than by job id, which is what a reloaded page no longer holds.
     */
    cachedSweep: (scenarioId: number) =>
      http.get<CachedSweep | null>(`${scenario(scenarioId)}/analyses/sweep`),
    /**
     * Store how the sweep's graphs are arranged. The server keeps the array as
     * sent and never reads into it, so the shape of a graph stays a client
     * decision; `parseLayout` is where it is checked on the way back.
     */
    saveSweepLayout: (scenarioId: number, graphs: unknown[]) =>
      http.put<void>(`${scenario(scenarioId)}/analyses/sweep/layout`, graphs),
  },

  /**
   * The What-if override stack: stored per scenario so a reload finds the same
   * layers, and applied — onto this scenario or a copy — only when asked. What
   * the stack does to the outcome is an analysis like any other:
   * `analysis.start` with `{ kind: "what-if", layers }`.
   */
  whatIf: {
    get: (scenarioId: number) => http.get<WhatIfStack>(`${scenario(scenarioId)}/what-if`),
    save: (scenarioId: number, body: WhatIfStack) =>
      http.put<void>(`${scenario(scenarioId)}/what-if`, body),
    /** Returns the scenario written to: this one, or the new copy when named. */
    apply: (scenarioId: number, body: ApplyWhatIf) =>
      http.post<Scenario>(`${scenario(scenarioId)}/what-if/apply`, body),
    /**
     * A small what-if answered in the response — no job, no polling. Aborting
     * `signal` stops the server's simulation too.
     */
    quick: (scenarioId: number, body: QuickWhatIf, signal?: AbortSignal) =>
      http.post<WhatIfOutcome>(`${scenario(scenarioId)}/what-if/quick`, body, signal),
  },

  runs: {
    list: (scenarioId: number) => http.get<Run[]>(`${scenario(scenarioId)}/runs`),
    create: (scenarioId: number, body: Partial<CreateRun> = {}) =>
      http.post<Run>(`${scenario(scenarioId)}/runs`, body),
    get: (id: number) => http.get<Run>(`/runs/${id}`),
    cancel: (id: number) => http.post<Run>(`/runs/${id}/cancel`),
    remove: (id: number) => http.delete(`/runs/${id}`),
    /**
     * `series` picks the path the per-account series and cash flows describe:
     * `"mean"`, or a percentile such as `"0.5"`. Defaults to the median.
     */
    results: (id: number, series?: string) =>
      http.get<Results>(`/runs/${id}/results${series ? `?series=${series}` : ""}`),
    /**
     * The itemised effects behind one year of the cash-flow table. Kept out of
     * `results` because the ledger dwarfs everything else a run stores and the
     * screen reads one year of it at a time.
     */
    ledger: (id: number, query: LedgerQuery = {}) => {
      const params = new URLSearchParams();
      for (const [key, value] of Object.entries(query)) {
        if (value != null) params.set(key, String(value));
      }
      const search = params.toString();
      return http.get<LedgerPage>(`/runs/${id}/ledger${search ? `?${search}` : ""}`);
    },
      inputs: (id: number) => http.get<RunInputs>(`/runs/${id}/inputs`),
    report: (id: number) => http.get<RunReport>(`/runs/${id}/report`),
    compare: (left: number, right: number) =>
      http.post<RunComparison>("/run-comparisons", { left_run_id: left, right_run_id: right }),
    archive: (id: number) => http.get<unknown>(`/runs/${id}/archive`),
  },
};
