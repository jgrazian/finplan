/**
 * The API's TypeScript surface.
 *
 * Everything in `./generated` is written by `scripts/gen-bindings.sh` straight
 * from the `finplan_server` request/response structs — never edit those files.
 * This module re-exports them and adds the few shapes ts-rs cannot express.
 */
export * from "./generated";

import type { FlavorSpec, UpdateAccount } from "./generated";
import type { Percentile } from "@/lib/types";

/**
 * `PATCH /scenarios/{id}/accounts/{id}` takes an optional flattened flavor.
 * `Option<FlavorSpec>` has no flattened form for ts-rs to emit, so the union is
 * composed here instead; see the `#[ts(skip)]` on `UpdateAccount::flavor`.
 */
export type UpdateAccountBody = UpdateAccount & (FlavorSpec | { flavor?: undefined });

/** The `status` column of a run. Stored as text, so narrow it at the edge. */
export type RunStatus = "queued" | "running" | "succeeded" | "failed" | "canceled";

export function isTerminal(status: string): boolean {
  return status === "succeeded" || status === "failed" || status === "canceled";
}

/**
 * The `series` query naming one of the run's stored paths.
 *
 * A run keeps the whole fan, but its per-account series, cash flows and ledger
 * describe one path at a time — so every request that reads them says which,
 * and they all have to say the same thing to agree on screen. The server
 * resolves the value to the nearest path it actually stored.
 */
export const SERIES: Record<Percentile, string> = {
  p5: "0.05",
  p50: "0.5",
  p95: "0.95",
};
