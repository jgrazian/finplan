/**
 * The plan-shaped half of the API, as one interface two homes implement.
 *
 * A plan lives either in this browser (`localApi`) or on the server
 * (`remoteApi`); the screens call a `PlanApi` and never learn which. Both
 * return the generated types, so `web/lib/view` stays the only layer that maps
 * them onto screens. What is not here is what only a server can do — auth, the
 * account, contact, drafts, review, suggestions and plan chat — and stays on
 * `api` in `client.ts`.
 *
 * Plain `import type` with `.ts` paths: the Node test runner loads this graph
 * to check the local backend against it, and it has no `@/` alias.
 */
import type { ArchiveImported } from "./generated/ArchiveImported.ts";
import type { ArchivePreview } from "./generated/ArchivePreview.ts";
import type { ImportArchive } from "./generated/ImportArchive.ts";
import type { PlanArchive } from "./generated/PlanArchive.ts";
import type { RunComparison } from "./generated/RunComparison.ts";
import type { RunInputs } from "./generated/RunInputs.ts";
import type { RunReport } from "./generated/RunReport.ts";
import type { SetupCreated } from "./generated/SetupCreated.ts";
import type { SetupPlan } from "./generated/SetupPlan.ts";
import type {
  Account,
  Analysis,
  AnalysisOutcome,
  AnalysisParameter,
  ApplyWhatIf,
  Asset,
  CachedSweep,
  CompareRequest,
  CompileReport,
  CreateAccount,
  CreateAnalysis,
  CreateAsset,
  CreatePosition,
  CreateProfile,
  CreateRun,
  CreateScenario,
  CreateTaxConfig,
  DrawdownBody,
  DrawdownComparison,
  DrawdownRequest,
  Event,
  EventBody,
  ExpressionValidation,
  ExpressionValidationRequest,
  HistoryPreset,
  LedgerPage,
  LedgerQuery,
  NamedParameter,
  ParameterBody,
  PreflightReport,
  Position,
  Profile,
  QuickWhatIf,
  ReorderRequest,
  Results,
  Run,
  Scenario,
  SetFunding,
  TaxConfig,
  UpdateAccountBody,
  UpdateAsset,
  UpdatePosition,
  UpdateProfile,
  UpdateScenario,
  UpdateTaxConfig,
  WhatIfOutcome,
  WhatIfStack,
} from "./types.ts";

export interface ScenariosApi {
  list: () => Promise<Scenario[]>;
  get: (id: number) => Promise<Scenario>;
  create: (body: CreateScenario) => Promise<Scenario>;
  update: (id: number, body: UpdateScenario) => Promise<Scenario>;
  /** When cash runs short: sell investments in this order, or `{ funding: null }` to record a shortfall instead. */
  setFunding: (id: number, body: SetFunding) => Promise<Scenario>;
  remove: (id: number) => Promise<void>;
  duplicate: (id: number, name: string) => Promise<Scenario>;
  /** The guided "answer a few questions" creation: the plan, its accounts and its events in one write. */
  setup: (body: SetupPlan) => Promise<SetupCreated>;
  /** Lower the scenario without running it, to surface config errors early. */
  compile: (id: number) => Promise<CompileReport>;
  /** What would stop the next run, and what is worth a second look. */
  preflight: (id: number) => Promise<PreflightReport>;
  /** The hash of the plan's inputs now, to tell whether a run still describes it. */
  inputHash: (id: number) => Promise<{ input_hash: string }>;
  /** One plan as an archive: the file for "Export" and the unit "Move to cloud" carries. */
  archive: (id: number) => Promise<PlanArchive>;
}

export interface AssetsApi {
  list: (scenarioId: number) => Promise<Asset[]>;
  create: (scenarioId: number, body: CreateAsset) => Promise<Asset>;
  update: (scenarioId: number, id: number, body: UpdateAsset) => Promise<Asset>;
  remove: (scenarioId: number, id: number) => Promise<void>;
  /** One write for the whole list, rather than a PATCH per row moved. */
  reorder: (scenarioId: number, ids: ReorderRequest["ids"]) => Promise<void>;
}

export interface AccountsApi {
  list: (scenarioId: number) => Promise<Account[]>;
  create: (scenarioId: number, body: CreateAccount) => Promise<Account>;
  update: (scenarioId: number, id: number, body: UpdateAccountBody) => Promise<Account>;
  remove: (scenarioId: number, id: number) => Promise<void>;
  reorder: (scenarioId: number, ids: ReorderRequest["ids"]) => Promise<void>;
  positions: (scenarioId: number, id: number) => Promise<Position[]>;
  addPosition: (scenarioId: number, id: number, body: CreatePosition) => Promise<Position>;
  updatePosition: (
    scenarioId: number,
    id: number,
    positionId: number,
    body: UpdatePosition,
  ) => Promise<Position>;
  removePosition: (scenarioId: number, id: number, positionId: number) => Promise<void>;
  reorderPositions: (scenarioId: number, id: number, ids: ReorderRequest["ids"]) => Promise<void>;
}

export interface EventsApi {
  list: (scenarioId: number) => Promise<Event[]>;
  create: (scenarioId: number, body: EventBody) => Promise<Event>;
  /** A replace, not a merge: a trigger is a tree and partial merges have no meaning. */
  replace: (scenarioId: number, id: number, body: EventBody) => Promise<Event>;
  remove: (scenarioId: number, id: number) => Promise<void>;
  /** Presentation only: an event fires on its trigger, not on its place. */
  reorder: (scenarioId: number, ids: ReorderRequest["ids"]) => Promise<void>;
}

export interface ParametersApi {
  list: (scenarioId: number) => Promise<NamedParameter[]>;
  create: (scenarioId: number, body: ParameterBody) => Promise<NamedParameter>;
  update: (scenarioId: number, id: number, body: ParameterBody) => Promise<NamedParameter>;
  remove: (scenarioId: number, id: number) => Promise<void>;
}

export interface ExpressionsApi {
  validate: (scenarioId: number, body: ExpressionValidationRequest) => Promise<ExpressionValidation>;
}

export interface ReturnProfilesApi {
  list: () => Promise<Profile[]>;
  create: (body: CreateProfile) => Promise<Profile>;
  update: (id: number, body: UpdateProfile) => Promise<Profile>;
  remove: (id: number) => Promise<void>;
  /** The library is the home's, so this reorders it for every plan there. */
  reorder: (ids: ReorderRequest["ids"]) => Promise<void>;
}

export interface InflationProfilesApi {
  list: () => Promise<Profile[]>;
  create: (body: CreateProfile) => Promise<Profile>;
  remove: (id: number) => Promise<void>;
  reorder: (ids: ReorderRequest["ids"]) => Promise<void>;
}

export interface TaxConfigsApi {
  list: () => Promise<TaxConfig[]>;
  create: (body: CreateTaxConfig) => Promise<TaxConfig>;
  update: (id: number, body: UpdateTaxConfig) => Promise<TaxConfig>;
  remove: (id: number) => Promise<void>;
}

/**
 * Sweeps, sensitivity rankings and goal seeks. One family: start to ask, get to
 * poll, results when it says `succeeded`. Not persisted, so an id outlives only
 * the process that issued it; nothing here should be bookmarked.
 */
export interface AnalysisApi {
  /** What this plan can vary, and the range each axis defaults to. */
  parameters: (scenarioId: number) => Promise<AnalysisParameter[]>;
  start: (scenarioId: number, body: CreateAnalysis) => Promise<Analysis>;
  get: (id: number) => Promise<Analysis>;
  cancel: (id: number) => Promise<Analysis>;
  /** Refused until the job reaches `succeeded`. */
  results: (id: number) => Promise<AnalysisOutcome>;
  /**
   * The scenario's most recent sweep, kept so a reload finds the grid again.
   * `null` before anything has been swept.
   */
  cachedSweep: (scenarioId: number) => Promise<CachedSweep | null>;
  /**
   * Store how the sweep's graphs are arranged. The array is kept as sent and
   * never read into, so the shape of a graph stays a client decision.
   */
  saveSweepLayout: (scenarioId: number, graphs: unknown[]) => Promise<void>;
}

/**
 * The What-if override stack: stored per plan so a reload finds the same
 * layers, and applied — onto this plan or a copy — only when asked.
 */
export interface WhatIfApi {
  get: (scenarioId: number) => Promise<WhatIfStack>;
  save: (scenarioId: number, body: WhatIfStack) => Promise<void>;
  /** Returns the scenario written to: this one, or the new copy when named. */
  apply: (scenarioId: number, body: ApplyWhatIf) => Promise<Scenario>;
  /** A small what-if answered in the call, with no job to poll. Aborting `signal` stops the simulation. */
  quick: (scenarioId: number, body: QuickWhatIf, signal?: AbortSignal) => Promise<WhatIfOutcome>;
}

export interface RunsApi {
  list: (scenarioId: number) => Promise<Run[]>;
  create: (scenarioId: number, body?: Partial<CreateRun>) => Promise<Run>;
  get: (id: number) => Promise<Run>;
  cancel: (id: number) => Promise<Run>;
  remove: (id: number) => Promise<void>;
  /**
   * `series` picks the path the per-account series and cash flows describe:
   * `"mean"`, or a percentile such as `"0.5"`. Defaults to the median.
   */
  results: (id: number, series?: string) => Promise<Results>;
  /**
   * The itemised effects behind one year of the cash-flow table. Kept out of
   * `results` because the ledger dwarfs everything else a run keeps and the
   * screen reads one year of it at a time.
   */
  ledger: (id: number, query?: LedgerQuery) => Promise<LedgerPage>;
  /**
   * The run's median path re-simulated under each withdrawal strategy
   * (spec 20), year by year from retirement. Refused with 409 for a run saved
   * before seeds were kept: "Run the plan again to see drawdown."
   */
  drawdown: (id: number, body?: DrawdownRequest) => Promise<DrawdownBody>;
  /**
   * A small Monte Carlo per strategy on one common seed. Aborting `signal`
   * stops the simulation.
   */
  drawdownCompare: (id: number, body?: CompareRequest, signal?: AbortSignal) => Promise<DrawdownComparison>;
  /** The inputs the run was made from: what freshness compares the plan's hash against. */
  inputs: (id: number) => Promise<RunInputs>;
  report: (id: number) => Promise<RunReport>;
  compare: (left: number, right: number) => Promise<RunComparison>;
  /** The run's inputs as a downloadable archive. */
  archive: (id: number) => Promise<unknown>;
}

/** Whole-home plan archives: backup, restore, and what moves a plan between homes. */
export interface ArchivesApi {
  exportAll: () => Promise<PlanArchive>;
  /** What an import would create, without creating it. */
  preview: (archive: PlanArchive) => Promise<ArchivePreview>;
  /** `from_guest` marks an adoption, so the server can count it. */
  import: (body: ImportArchive) => Promise<ArchiveImported>;
}

export interface PlanApi {
  scenarios: ScenariosApi;
  assets: AssetsApi;
  accounts: AccountsApi;
  events: EventsApi;
  parameters: ParametersApi;
  expressions: ExpressionsApi;
  returnProfiles: ReturnProfilesApi;
  inflationProfiles: InflationProfilesApi;
  /** The bootstrap histories the engine ships with, series and all. */
  historyPresets: () => Promise<HistoryPreset[]>;
  taxConfigs: TaxConfigsApi;
  analysis: AnalysisApi;
  whatIf: WhatIfApi;
  runs: RunsApi;
  archives: ArchivesApi;
}
