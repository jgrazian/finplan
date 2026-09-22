import type { Run } from "../api/types";

/** On reload reconnect to live work before adopting a completed result. */
export function preferredRun(runs: Run[]): Run | undefined {
  return runs.find((run) => run.status === "queued" || run.status === "running") ??
    runs.find((run) => run.status === "succeeded") ?? runs[0];
}

/** Use enqueue time: completion time can follow edits made during the run. */
export function resultsAreStale(
  scenario: { id: number; updated_at: string } | undefined,
  run: Pick<Run, "scenario_id" | "created_at"> | undefined,
  savedSinceStart: boolean,
): boolean {
  return scenario != null && run?.scenario_id === scenario.id &&
    (savedSinceStart || scenario.updated_at > run.created_at);
}

/** Keep the first captured display context across percentile reads and navigation.
 * A locally started replacement explicitly replaces it because SQLite may reuse IDs.
 * This is session context only; durable historic provenance requires server snapshots.
 */
export function rememberRunContext<T>(
  contexts: Map<string, T>,
  run: Pick<Run, "scenario_id" | "id" | "created_at">,
  incoming: T,
  replace = false,
): T {
  const key = `${run.scenario_id}:${run.id}:${run.created_at}`;
  if (replace || !contexts.has(key)) contexts.set(key, incoming);
  return contexts.get(key)!;
}
