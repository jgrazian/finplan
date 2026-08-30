/**
 * The one place a request to `finplan-server` is made.
 *
 * Requests go to a same-origin `/api/...` path that `next.config.ts` rewrites
 * onto the Rust server, so the session cookie rides along without CORS.
 */
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

type Method = "GET" | "POST" | "PATCH" | "PUT" | "DELETE";

async function request<T>(
  method: Method,
  path: string,
  body?: unknown,
): Promise<T> {
  const response = await fetch(`/api${path}`, {
    method,
    credentials: "include",
    headers: body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

  if (!response.ok) throw await toApiError(response);

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
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, body ?? {}),
  patch: <T>(path: string, body: unknown) => request<T>("PATCH", path, body),
  put: <T>(path: string, body: unknown) => request<T>("PUT", path, body),
  delete: (path: string) => request<void>("DELETE", path),
};
