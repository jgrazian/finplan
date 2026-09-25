/**
 * The one place a request to `finplan-server` is made.
 *
 * Requests go to a same-origin `/api/...` path that `next.config.ts` rewrites
 * onto the Rust server, so the session cookie rides along without CORS.
 */
import { serverMonitor } from "@/lib/status/monitor";
import type { ErrorBody } from "./generated";

/** A non-2xx response, carrying the server's own error code and message. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }

  /** No session, or an expired one — the caller should show the login form. */
  get isUnauthorized(): boolean {
    return this.status === 401;
  }
}

/**
 * The request never reached the server, or the answer never came back.
 *
 * Distinct from `ApiError`, which is the server refusing in its own words: a
 * caller can say "not saved — the server did not answer" only for this one.
 */
export class NetworkError extends Error {
  constructor(message = "the server did not answer") {
    super(message);
    this.name = "NetworkError";
  }
}

type Method = "GET" | "POST" | "PATCH" | "PUT" | "DELETE";

async function request<T>(
  method: Method,
  path: string,
  body?: unknown,
  signal?: AbortSignal,
): Promise<T> {
  // Every outcome is reported to the monitor, which owns the reconnect
  // schedule and the status bar. A refused write is graver than a refused
  // read: it is the moment "changes will not save" becomes true.
  const writing = method !== "GET";

  let response: Response;
  try {
    response = await fetch(`/api${path}`, {
      method,
      credentials: "include",
      headers: body === undefined ? undefined : { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal,
    });
  } catch (err) {
    // The caller withdrew the question; the server is not at fault.
    if (signal?.aborted) throw err;
    serverMonitor.failed(writing ? "write" : "read");
    throw new NetworkError();
  }

  if (!response.ok) {
    const error = await toApiError(response);
    if (error.isUnauthorized) {
      serverMonitor.expireSession();
      serverMonitor.reached();
    } else if (response.status >= 500) {
      serverMonitor.failed(writing ? "write" : "read");
    } else {
      // A 4xx is an answer, not an outage — the server is up and disagreeing.
      // Validation belongs under the field that caused it, never in the bar.
      serverMonitor.reached();
    }
    throw error;
  }

  serverMonitor.reached();

  // 204 on every delete, and `/health` answers in plain text.
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

async function toApiError(response: Response): Promise<ApiError> {
  try {
    const body = (await response.json()) as ErrorBody;
    return new ApiError(response.status, body.error.code, body.error.message);
  } catch {
    // A proxy or a panic can answer with something that is not our error shape.
    return new ApiError(response.status, "unknown", response.statusText);
  }
}

export const http = {
  get: <T>(path: string) => request<T>("GET", path),
  /** `signal` aborts the request, for answers the caller may stop wanting. */
  post: <T>(path: string, body?: unknown, signal?: AbortSignal) =>
    request<T>("POST", path, body ?? {}, signal),
  patch: <T>(path: string, body: unknown) => request<T>("PATCH", path, body),
  put: <T>(path: string, body: unknown) => request<T>("PUT", path, body),
  /** A body is optional, and only account deletion sends one (the password). */
  delete: (path: string, body?: unknown) => request<void>("DELETE", path, body),
};
