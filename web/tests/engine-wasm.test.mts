/**
 * Smoke test of the browser engine (spec 19): the real `.wasm`, loaded the way
 * a worker loads it, driven through make a plan, edit it, read it back, run it
 * and read the results.
 *
 * It needs `web/lib/engine/pkg`, which `scripts/build-wasm.sh` writes (`pnpm
 * wasm`). Without it the test skips with a message, unless FINPLAN_REQUIRE_WASM
 * is set, which CI sets so a broken build can never pass by skipping.
 */
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const pkg = (file: string) => fileURLToPath(new URL(`../lib/engine/pkg/${file}`, import.meta.url));

/** The slice of the generated module this test calls. */
interface Engine {
  initSync: (input: { module: BufferSource }) => unknown;
  model_version: () => string;
  library_seed: () => string;
  new_plan: (body: string, library: string, id: number, now: string) => string;
  apply_edit: (graph: string, library: string, op: string, now?: string | null) => string;
  read: (graph: string, library: string, query: string) => string;
  snapshot: (graph: string, library: string) => string;
  coordinator_new: (snapshot: string, settings: string) => number;
  coordinator_next_round: (handle: number) => string | undefined;
  coordinator_absorb: (handle: number, outputs: string) => void;
  coordinator_completed: (handle: number) => number;
  coordinator_finish: (handle: number) => string;
  coordinator_drop: (handle: number) => void;
  prepare: (snapshot: string, settings: string) => number;
  run_batch: (handle: number, spec: string) => string;
  release: (handle: number) => void;
  results_view: (runResults: string, runId: number, scenarioId: number, series?: string | null) => string;
  ledger_page: (runResults: string, runId: number, query: string) => string;
}

const built = existsSync(pkg("finplan_wasm.js")) && existsSync(pkg("finplan_wasm_bg.wasm"));

if (!built && process.env.FINPLAN_REQUIRE_WASM) {
  test("the engine is built", () => {
    assert.fail("FINPLAN_REQUIRE_WASM is set but web/lib/engine/pkg is missing: run scripts/build-wasm.sh");
  });
}

test(
  "a plan is made, edited, run and read through the wasm engine",
  { skip: built ? false : "web/lib/engine/pkg is not built; run `pnpm wasm` (scripts/build-wasm.sh)" },
  async () => {
    const engine = (await import(pathToFileURL(pkg("finplan_wasm.js")).href)) as Engine;
    engine.initSync({ module: readFileSync(pkg("finplan_wasm_bg.wasm")) });

    assert.match(engine.model_version(), /^finplan-/);

    const library = engine.library_seed();
    const profile = (JSON.parse(library) as { return_profiles: { id: number }[] }).return_profiles[0].id;

    let graph = engine.new_plan(
      JSON.stringify({ name: "Smoke", start_date: "2026-01-01", birth_date: "1980-01-01", duration_years: 10 }),
      library,
      1,
      "2026-10-03 12:00:00",
    );
    const edit = (op: object) => {
      const out = JSON.parse(engine.apply_edit(graph, library, JSON.stringify(op), "2026-10-03 12:01:00")) as {
        graph: unknown;
        outcome: { id: number | null };
      };
      graph = JSON.stringify(out.graph);
      return out.outcome.id;
    };
    const read = <T,>(query: object) => JSON.parse(engine.read(graph, library, JSON.stringify(query))) as T;

    const accountId = edit({
      op: "create_account",
      body: { name: "Checking", flavor: "Bank", cash_value: 50_000, return_profile_id: profile },
    });
    assert.equal(accountId, 1);
    edit({ op: "update_scenario", body: { name: "Smoke test" } });
    edit({ op: "create_asset", body: { name: "VTI", initial_price: 250, return_profile_id: profile } });

    assert.equal(read<{ name: string }>({ query: "scenario" }).name, "Smoke test");
    const accounts = read<{ name: string; cash_value: number }[]>({ query: "accounts" });
    assert.deepEqual(
      accounts.map((a) => [a.name, a.cash_value]),
      [["Checking", 50_000]],
    );
    assert.equal(read<{ ok: boolean }>({ query: "compile_report" }).ok, true);

    // A refusal is thrown as the server's error, status and code included.
    assert.throws(
      () => edit({ op: "delete_account", id: 99 }),
      (thrown: unknown) => {
        const error = JSON.parse(String(thrown)) as { status: number; code: string };
        return error.status === 404 && error.code === "not_found";
      },
    );

    // A local run: one coordinator, one prepared plan, batches run by the host.
    const { json: snapshot, hash } = JSON.parse(engine.snapshot(graph, library)) as { json: string; hash: string };
    assert.match(hash, /^[0-9a-f]{64}$/);
    const settings = JSON.stringify({ iterations: 40, seed: 12345, batch_size: 10, parallel_batches: 2 });
    const coordinator = engine.coordinator_new(snapshot, settings);
    const prepared = engine.prepare(snapshot, settings);
    for (let specs = engine.coordinator_next_round(coordinator); specs; specs = engine.coordinator_next_round(coordinator)) {
      // BatchSpec and BatchOutput stay strings: iteration seeds exceed 2^53.
      const outputs = (JSON.parse(specs) as object[]).map((spec) => engine.run_batch(prepared, JSON.stringify(spec)));
      engine.coordinator_absorb(coordinator, `[${outputs.join(",")}]`);
    }
    assert.equal(engine.coordinator_completed(coordinator), 40);
    const runResults = engine.coordinator_finish(coordinator);
    engine.coordinator_drop(coordinator);
    engine.release(prepared);

    const results = JSON.parse(engine.results_view(runResults, 1, 1)) as {
      stats: { num_iterations: number; success_rate: number };
      bands: unknown[];
    };
    assert.equal(results.stats.num_iterations, 40);
    assert.ok(results.bands.length >= 3);
    const page = JSON.parse(engine.ledger_page(runResults, 1, JSON.stringify({ limit: 5 }))) as { entries: unknown[] };
    assert.ok(page.entries.length <= 5);
  },
);
