/**
 * The API's TypeScript surface.
 *
 * Everything in `./generated` is written by `scripts/gen-bindings.sh` straight
 * from the `finplan_server` request/response structs — never edit those files.
 * This module re-exports them and adds the few shapes ts-rs cannot express.
 */
export * from "./generated";

// The what-if stack's bindings, named one by one: the barrel above is rebuilt
// by `gen-bindings.sh`, and these are what the Analysis tab's What-if mode is
// typed with whichever way round the two land.
export type { ApplyWhatIf } from "./generated/ApplyWhatIf";
export type { QuickWhatIf } from "./generated/QuickWhatIf";
export type { WhatIfEntry } from "./generated/WhatIfEntry";
export type { WhatIfFan } from "./generated/WhatIfFan";
export type { WhatIfLayer } from "./generated/WhatIfLayer";
export type { WhatIfOutcome } from "./generated/WhatIfOutcome";
export type { WhatIfStack } from "./generated/WhatIfStack";
export type { WhatIfStep } from "./generated/WhatIfStep";

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
 * A run keeps a separate real envelope. Its accounts, cash flows and ledger
 * describe a nominal-terminal-ranked representative path, named by the actual
 * response `series_id`. These selector requests resolve to the nearest stored
 * rank; detail/ledger requests must use the resolved ID, not the selector target.
 */
export const SERIES: Record<Percentile, string> = {
  p10: "0.1",
  p50: "0.5",
  p90: "0.9",
};

/**
 * The example runs a new run stores. A run started before these keeps its
 * P5/P95 paths, and the server answers a P10/P90 request with the nearest.
 */
export const STORED_PERCENTILES = [0.1, 0.5, 0.9];
