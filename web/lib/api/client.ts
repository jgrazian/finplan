/**
 * One function per `finplan_server` route, typed with the generated bindings.
 *
 * Grouped by resource so a caller reads as `api.runs.results(id)`. Paths mirror
 * `crates/finplan_server/src/api/mod.rs`.
 */
import { http } from "./http";
import type {
  Account,
  Analysis,
  AnalysisOutcome,
  AnalysisParameter,
  Asset,
  CachedSweep,
  CompileReport,
  ContactMessageReceipt,
  CreateContactMessage,
  CreateAccount,
  CreateAnalysis,
  CreateAsset,
  CreatePosition,
  CreateProfile,
  CreateRun,
  CreateScenario,
  CreateTaxConfig,
  Credentials,
  DeleteAccount,
  Event,
  EventBody,
  NamedParameter,
  ParameterBody,
  ExpressionValidation,
  ExpressionValidationRequest,
  HistoryPreset,
  LedgerPage,
  LedgerQuery,
  PasswordChange,
  Position,
  Profile,
  RegisterCredentials,
  ReorderRequest,
  Results,
  Run,
  Scenario,
  SessionInfo,
  TaxConfig,
  UpdateAccountBody,
  UpdateAsset,
  UpdatePosition,
  UpdatePreferences,
  UpdateProfile,
  UpdateScenario,
  UpdateTaxConfig,
  UpdateUserProfile,
  UserResponse,
} from "./types";

const scenario = (id: number) => `/scenarios/${id}`;

export const api = {
  auth: {
    me: () => http.get<UserResponse>("/auth/me"),
    login: (body: Credentials) => http.post<UserResponse>("/auth/login", body),
    register: (body: RegisterCredentials) => http.post<UserResponse>("/auth/register", body),
    logout: () => http.post<void>("/auth/logout"),
  },

  /**
   * The account itself. Profile and preferences are PUTs rather than PATCHes:
   * the account form is one Save over every field, so an emptied field has to
   * mean "clear it" — which a merge cannot say.
   */
  account: {
    updateProfile: (body: UpdateUserProfile) => http.put<UserResponse>("/auth/profile", body),
    updatePreferences: (body: UpdatePreferences) =>
      http.put<UserResponse>("/auth/preferences", body),
    changePassword: (body: PasswordChange) => http.post<void>("/auth/password", body),
    sessions: () => http.get<SessionInfo[]>("/auth/sessions"),
    revokeSession: (id: string) => http.delete(`/auth/sessions/${encodeURIComponent(id)}`),
    /** Unrecoverable, and cascades to every scenario, run and session. */
    remove: (body: DeleteAccount) => http.delete("/auth/me", body),
  },

  contact: {
    submit: (body: CreateContactMessage) =>
      http.post<ContactMessageReceipt>("/contact-messages", body),
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
  },
};
