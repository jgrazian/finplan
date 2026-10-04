/**
 * The routes that only a server can answer: auth, the account, contact, and the
 * AI features (drafts, review, suggestions, plan chat).
 *
 * Plan data does not live here. It goes through a `PlanApi` — `remoteApi` for a
 * plan in the cloud, `localApi` for one in this browser — picked by
 * `planApiFor`/`usePlanApi`, so a screen never names a home. Keeping the plan
 * groups off `api` is what makes that a compile error to forget.
 *
 * Grouped by resource so a caller reads as `api.auth.me()`. Paths mirror
 * `crates/finplan_server/src/api/mod.rs`.
 */
import { http } from "./http";
import type { ComputeClient } from "../local/offload";
import type {
  ComputeBudget,
  ComputeRun,
  ComputeRunCreated,
  ComputeRunRequest,
  ContactMessageReceipt,
  CreateContactMessage,
  CreateDraft,
  Credentials,
  DeleteAccount,
  DocumentManifest,
  DraftAnswers,
  DraftMessage,
  DraftCreated,
  DraftStatus,
  PasswordChange,
  Preview,
  RegisterCredentials,
  SessionInfo,
  StartDraft,
  StartDrafting,
  UpdateDraft,
  UpdatePreferences,
  UpdateUserProfile,
  UserResponse,
} from "./types";
import type {
  AppliedSuggestion,
  ApplySuggestion,
  ChatRequest,
  DismissSuggestion,
  PreviewSuggestion,
  Review,
  Suggestion,
  SuggestionStatus,
  SuggestionThread,
  PlanThread,
} from "./suggestions";

export { remoteApi } from "./remote";

const scenario = (id: number) => `/scenarios/${id}`;

export const api = {
  auth: {
    me: () => http.get<UserResponse>("/auth/me"),
    login: (body: Credentials) => http.post<UserResponse>("/auth/login", body),
    register: (body: RegisterCredentials) => http.post<UserResponse>("/auth/register", body),
    logout: () => http.post<void>("/auth/logout"),
    /** A guest session (spec 17): 403 when guest access is off, 429 when rate limited. */
    guest: () => http.post<UserResponse>("/auth/guest"),
    /** Turn the caller's guest into an account in place; plans stay with it. */
    claimGuest: (body: RegisterCredentials) =>
      http.post<UserResponse>("/auth/guest/claim", body),
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

  /**
   * Server offload (spec 19): a local plan's run, sent to FinPlan's servers.
   * Remote-only and never on a `PlanApi`: the plan leaves the device for it,
   * so it is only reached from the explicit "Run on FinPlan servers" action.
   */
  compute: {
    budget: () => http.get<ComputeBudget>("/compute/budget"),
    create: (body: ComputeRunRequest) => http.post<ComputeRunCreated>("/compute/runs", body),
    get: (id: number) => http.get<ComputeRun>(`/compute/runs/${id}`),
    /** Cancels a running job; on a finished one it deletes the results. */
    remove: (id: number) => http.delete(`/compute/runs/${id}`),
  } satisfies ComputeClient & { budget: () => Promise<ComputeBudget> },

  contact: {
    submit: (body: CreateContactMessage) =>
      http.post<ContactMessageReceipt>("/contact-messages", body),
  },

  /**
   * The Review tab: notes written about one run, each carrying the plan
   * changes it proposes. `run` writes a fresh review of the named run (the
   * latest succeeded one when `null`); `get` reads the last one written.
   */
  review: {
    get: (scenarioId: number) => http.get<Review | null>(`${scenario(scenarioId)}/review`),
    run: (scenarioId: number, runId: number | null = null) =>
      http.post<Review>(`${scenario(scenarioId)}/review`, { run_id: runId }),
  },

  suggestions: {
    list: (scenarioId: number, status?: SuggestionStatus) =>
      http.get<Suggestion[]>(
        `${scenario(scenarioId)}/suggestions${status ? `?status=${status}` : ""}`,
      ),
    /**
     * Simulates a path's steps (all of them, or through one) against the
     * note's run and keeps the result as that path's check; the plan is not
     * written.
     */
    preview: (id: number, body: PreviewSuggestion) =>
      http.post<Preview>(`/suggestions/${id}/preview`, body),
    /**
     * Applies a path's remaining steps, or those through one step. 409 when
     * the plan moved since the note was written; the body lists why.
     */
    apply: (id: number, body: ApplySuggestion) =>
      http.post<AppliedSuggestion>(`/suggestions/${id}/apply`, body),
    dismiss: (id: number, body: DismissSuggestion) =>
      http.post<Suggestion>(`/suggestions/${id}/dismiss`, body),
    /** Undoes a dismissal or an "it's correct": the note is open again. 409 otherwise. */
    reopen: (id: number) => http.post<Suggestion>(`/suggestions/${id}/reopen`, {}),
    /**
     * The follow-up thread about a note. Sending starts a model turn and
     * answers at once with the thread `running`; read it again until the
     * reply lands. 409 when AI review is off, a turn is already running, or
     * the thread is full.
     */
    chat: {
      get: (id: number) => http.get<SuggestionThread>(`/suggestions/${id}/chat`),
      send: (id: number, message: string) =>
        http.post<SuggestionThread>(`/suggestions/${id}/chat`, { message } satisfies ChatRequest),
    },
  },

  /**
   * Plan chat on the Review tab: one thread per plan. Sending spends one of
   * the month's plan chat messages and answers at once with the thread
   * `running`; read it again until the reply lands. The model's changes come
   * back as notes on the review, never as edits. 409 when AI review is off,
   * the plan has no review, or a turn is running; 403 when the month's
   * messages are spent.
   */
  planChat: {
    get: (scenarioId: number) => http.get<PlanThread>(`${scenario(scenarioId)}/chat`),
    send: (scenarioId: number, message: string) =>
      http.post<PlanThread>(`${scenario(scenarioId)}/chat`, { message } satisfies ChatRequest),
    /** Start over. The notes the thread added stay on the review. */
    clear: (scenarioId: number) => http.delete(`${scenario(scenarioId)}/chat`),
  },

  /**
   * AI-guided drafts (`api::drafts`): a draft is a scenario with
   * `status: "draft"`, never listed with the plans. Creating one spends a
   * draft from the month's quota.
   */
  drafts: {
    create: (body: Partial<StartDraft> = {}) => http.post<DraftStatus>("/drafts", body),
    get: (id: number) => http.get<DraftStatus>(`/drafts/${id}`),
    update: (id: number, body: UpdateDraft) => http.patch<DraftStatus>(`/drafts/${id}`, body),
    /** Deletes the draft, its notes and its documents. */
    remove: (id: number) => http.delete(`/drafts/${id}`),
    /** Starts the drafting agent; progress is read with `get`. */
    start: (id: number, body: StartDrafting) =>
      http.post<DraftStatus>(`/drafts/${id}/start`, body),
    /** Answers the agent's open questions, by key, with anything else the person wrote; it resumes. */
    answer: (id: number, body: DraftAnswers) =>
      http.post<DraftStatus>(`/drafts/${id}/answers`, body),
    /** A follow-up to a finished draft: the agent runs again over it. 409 while it writes or waits. */
    message: (id: number, body: DraftMessage) =>
      http.post<DraftStatus>(`/drafts/${id}/messages`, body),
    /**
     * Create & run: 409 while the agent is still writing. With `add_open` the
     * open notes are added first; 422 names any that could not be, and the
     * draft stays a draft.
     */
    createAndRun: (id: number, body: CreateDraft = {}) =>
      http.post<DraftCreated>(`/drafts/${id}/create`, body),
    documents: {
      list: (id: number) => http.get<DocumentManifest[]>(`/drafts/${id}/documents`),
      /** 413 over the tier's file or size limit, 415 for a type it cannot read. */
      upload: (id: number, files: readonly File[]) => {
        const form = new FormData();
        for (const file of files) form.append("file", file, file.name);
        return http.upload<DocumentManifest[]>(`/drafts/${id}/documents`, form);
      },
      remove: (id: number, documentId: number) =>
        http.delete(`/drafts/${id}/documents/${documentId}`),
    },
  },
};
