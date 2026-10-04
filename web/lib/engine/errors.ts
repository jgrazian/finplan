/**
 * Errors from the local engine, shaped as `ApiError` (`web/lib/api/http.ts`).
 *
 * `ApiError` lives beside `fetch`, which nothing the local engine runs may
 * import, so the engine throws a `LocalError` with the same four fields and
 * `isUnauthorized`; the main-thread client (`client.ts`), which may import
 * `http.ts`, re-throws each as a real `ApiError`, so `instanceof ApiError`
 * checks in screens (a 409 on a stale what-if, the review's refusals) see no
 * difference between a plan's two homes.
 */
import type { EngineError } from "../api/generated/EngineError.ts";

export interface ErrorFields {
  status: number;
  code: string;
  message: string;
  /** The server's own error body, for routes that explain a refusal in fields. */
  body?: unknown;
}

export class LocalError extends Error {
  status: number;
  code: string;
  body?: unknown;
  isUnauthorized = false;

  constructor(status: number, code: string, message: string, body?: unknown) {
    super(message);
    this.name = "LocalError";
    this.status = status;
    this.code = code;
    this.body = body;
  }

  /** The fields that survive structured clone. */
  fields(): ErrorFields {
    return { status: this.status, code: this.code, message: this.message, body: this.body };
  }
}

/** A refusal in the server's own shape: its code names the kind. */
export function localError(status: number, code: string, message: string): LocalError {
  return new LocalError(status, code, message, { error: { code, message } });
}

export const notFound = (what: string) => localError(404, "not_found", `${what} not found`);
export const badRequest = (message: string) => localError(400, "bad_request", message);
export const conflict = (message: string) => localError(409, "conflict", message);

/** What the engine's `EngineError` says, or undefined for anything else. */
function asEngineError(value: unknown): EngineError | undefined {
  let parsed: unknown = value;
  if (typeof value === "string") {
    try {
      parsed = JSON.parse(value);
    } catch {
      return undefined;
    }
  }
  if (parsed && typeof parsed === "object") {
    const candidate = parsed as Partial<EngineError>;
    if (
      typeof candidate.status === "number" &&
      typeof candidate.code === "string" &&
      typeof candidate.message === "string"
    ) {
      return candidate as EngineError;
    }
  }
  return undefined;
}

/**
 * Whatever a failed call threw, as a `LocalError`: an engine refusal keeps its
 * status, code and body; anything else (a trap, a browser error) is a 500 whose
 * message is its own.
 */
export function toLocalError(thrown: unknown): LocalError {
  if (thrown instanceof LocalError) return thrown;
  const engine = asEngineError(thrown);
  if (engine) return new LocalError(engine.status, engine.code, engine.message, engine.body);
  const message = thrown instanceof Error ? thrown.message : String(thrown);
  return localError(500, "internal", message);
}

/** A thrown engine panic: the instance is dead and must be replaced. */
export function isPanic(thrown: unknown): boolean {
  return asEngineError(thrown)?.code === "panic";
}

/** Call `fn`, turning what it throws into a `LocalError`. */
export function guard<T>(fn: () => T): T {
  try {
    return fn();
  } catch (thrown) {
    throw toLocalError(thrown);
  }
}

/** The error a cancelled run or analysis ends with. */
export const CANCELLED = "canceled";
