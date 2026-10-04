/**
 * The slice of the generated WASM module (`./pkg/finplan_wasm.js`, built by
 * `scripts/build-wasm.sh`) that the local engine calls.
 *
 * Everything crosses as JSON text. The module itself is only imported by the
 * two worker entry files and by tests (which load it with `initSync`); the rest
 * of the engine takes an `Engine` so it runs the same in a worker, in Node and
 * in a test with the module swapped for a stand-in.
 *
 * A failed call throws a string, the JSON of an `EngineError`
 * (`{status, code, message, body?}`); `errors.ts` turns it into an error.
 */
export interface Engine {
  model_version(): string;
  library_seed(): string;
  new_plan(body: string, library: string, id: number, now: string): string;
  setup_plan(body: string, library: string, id: number, now: string): string;
  duplicate_plan(graph: string, new_id: number, name: string, now: string): string;
  apply_edit(graph: string, library: string, op: string, now?: string | null): string;
  apply_library(library: string, plans: string, op: string, now?: string | null): string;
  read(graph: string, library: string, query: string): string;
  read_library(library: string, plans: string, query: string): string;
  snapshot(graph: string, library: string): string;
  export_archive(graphs: string): string;
  import_archive(archive: string): string;
  preview_archive(archive: string): string;
  restore_plan(
    library: string,
    graph: string,
    new_id: number,
    name: string,
    now: string,
    suffix: string,
  ): string;
  run_cost(graph: string, settings: string): string;

  coordinator_new(snapshot: string, settings: string): number;
  coordinator_info(handle: number): string;
  coordinator_next_round(handle: number): string | undefined;
  coordinator_absorb(handle: number, outputs: string): void;
  coordinator_completed(handle: number): number;
  coordinator_finish(handle: number): string;
  coordinator_drop(handle: number): void;

  prepare(snapshot: string, settings: string): number;
  /** `progress(done, total)` is called as the batch's simulations finish; return true to stop it. */
  run_batch(
    handle: number,
    spec: string,
    progress?: (done: number, total: number) => boolean | void,
  ): string;
  release(handle: number): void;

  results_view(runResults: string, runId: number, scenarioId: number, series?: string | null): string;
  ledger_page(runResults: string, runId: number, query: string): string;
  results_open(runResults: string): number;
  results_view_open(handle: number, runId: number, scenarioId: number, series?: string | null): string;
  ledger_page_open(handle: number, runId: number, query: string): string;
  results_close(handle: number): void;

  analysis_plan(graph: string, library: string, body: string): string;
  analysis_run(
    graph: string,
    library: string,
    body: string,
    progress: (done: number, total: number) => boolean | void,
  ): string;
  analysis_shard(
    graph: string,
    library: string,
    body: string,
    shard: number,
    shards: number,
    progress: (done: number, total: number) => boolean | void,
  ): string;
  /** `answers` is a `string[]` JSON of what the shards returned. */
  analysis_finish(
    graph: string,
    library: string,
    body: string,
    answers: string,
    progress: (done: number, total: number) => boolean | void,
  ): string;
  quick_what_if(
    graph: string,
    library: string,
    body: string,
    progress: (done: number, total: number) => boolean | void,
  ): string;
  /** `seed` is the path's decimal seed, as `PathResults.seed` has it; `request` a `DrawdownRequest`. */
  drawdown(snapshot: string, seed: string, request: string): string;
  /** `body` is a `CompareRequest`; the answer a `DrawdownComparison`. */
  drawdown_compare(
    snapshot: string,
    body: string,
    progress: (done: number, total: number) => boolean | void,
  ): string;
  apply_what_if(graph: string, library: string, body: string, newId: number, now: string): string;
  check_what_if_stack(body: string): string;
  /** `copyId` is NaN to apply to the plan itself. */
  apply_note(
    graph: string,
    library: string,
    pathKey: string,
    steps: string,
    copyId: number,
    copyName: string,
    now: string,
  ): string;
  local_review(
    graph: string,
    library: string,
    runResults: string,
    runId: number,
    reviewedAt: string,
    silenced: string,
  ): string;
}

/** `BatchSpec` objects are flat (`{"index":0,"seed":..,"iterations":..}`). */
const FLAT_OBJECT = /\{[^{}]*\}/g;

/**
 * The specs of a round, one string each, without parsing them: a batch's seed
 * is a full 64-bit integer, which `JSON.parse` would round. A spec has no
 * nested object, so a flat match splits the array exactly.
 */
export function splitSpecs(round: string): string[] {
  return round.match(FLAT_OBJECT) ?? [];
}

/** `BatchOutput`s joined into the array `coordinator_absorb` takes, again as text. */
export function joinOutputs(outputs: readonly string[]): string {
  return `[${outputs.join(",")}]`;
}
