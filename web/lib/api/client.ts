/**
 * One function per `finplan_server` route, typed with the generated bindings.
 *
 * Grouped by resource so a caller reads as `api.runs.results(id)`. Paths mirror
 * `crates/finplan_server/src/api/mod.rs`.
 */
import { http } from "./http";
import type {
  Account,
  Asset,
  CompileReport,
  CreateAccount,
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
  HistoryPreset,
  PasswordChange,
  Position,
  Profile,
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
    register: (body: Credentials) => http.post<UserResponse>("/auth/register", body),
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
  },

  accounts: {
    list: (scenarioId: number) => http.get<Account[]>(`${scenario(scenarioId)}/accounts`),
    create: (scenarioId: number, body: CreateAccount) =>
      http.post<Account>(`${scenario(scenarioId)}/accounts`, body),
    update: (scenarioId: number, id: number, body: UpdateAccountBody) =>
      http.patch<Account>(`${scenario(scenarioId)}/accounts/${id}`, body),
    remove: (scenarioId: number, id: number) =>
      http.delete(`${scenario(scenarioId)}/accounts/${id}`),
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
  },

  returnProfiles: {
    list: () => http.get<Profile[]>("/return-profiles"),
    create: (body: CreateProfile) => http.post<Profile>("/return-profiles", body),
    update: (id: number, body: UpdateProfile) =>
      http.patch<Profile>(`/return-profiles/${id}`, body),
    remove: (id: number) => http.delete(`/return-profiles/${id}`),
  },

  inflationProfiles: {
    list: () => http.get<Profile[]>("/inflation-profiles"),
    create: (body: CreateProfile) => http.post<Profile>("/inflation-profiles", body),
    remove: (id: number) => http.delete(`/inflation-profiles/${id}`),
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
  },
};
