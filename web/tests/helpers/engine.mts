/**
 * Shared set-up for the local-engine tests: the real `.wasm`, loaded as a Node
 * test loads it (`initSync`), and a backend over the in-memory store and an
 * in-process compute pool.
 *
 * The engine needs `web/lib/engine/pkg` (`pnpm wasm`). A test file that needs it
 * calls `requireEngine()` at its top level: without the build it registers a
 * skipped test, unless FINPLAN_REQUIRE_WASM is set (CI), when it registers a
 * failing one.
 */
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import type { PlanApi } from "../../lib/api/plan.ts";
import { type EngineRuntime, type LocalEngine, createLocalEngine } from "../../lib/engine/backend.ts";
import type { Engine } from "../../lib/engine/engine.ts";
import { type WorkerPool, inProcessPool } from "../../lib/engine/pool.ts";
import { MemoryStore } from "../../lib/engine/store.ts";

const pkg = (file: string) => fileURLToPath(new URL(`../../lib/engine/pkg/${file}`, import.meta.url));

export const built = existsSync(pkg("finplan_wasm.js")) && existsSync(pkg("finplan_wasm_bg.wasm"));

/** What a test file calls once, at its top: false when the engine is missing and the tests should not be registered. */
export function requireEngine(name: string): boolean {
  if (built) return true;
  if (process.env.FINPLAN_REQUIRE_WASM) {
    test(`${name}: the engine is built`, () => {
      assert.fail("FINPLAN_REQUIRE_WASM is set but web/lib/engine/pkg is missing: run scripts/build-wasm.sh");
    });
  } else {
    test(`${name}`, { skip: "web/lib/engine/pkg is not built; run `pnpm wasm`" }, () => undefined);
  }
  return false;
}

let loaded: Promise<Engine> | undefined;

/** The WASM engine, instantiated once for the process. */
export function loadEngine(): Promise<Engine> {
  loaded ??= (async () => {
    const wasm = (await import(pathToFileURL(pkg("finplan_wasm.js")).href)) as Engine & {
      initSync: (input: { module: BufferSource }) => unknown;
    };
    wasm.initSync({ module: readFileSync(pkg("finplan_wasm_bg.wasm")) });
    return wasm;
  })();
  return loaded;
}

export interface Harness {
  engine: Engine;
  store: MemoryStore;
  pool: WorkerPool;
  local: LocalEngine;
  api: PlanApi;
  runtime: EngineRuntime;
  /** Advances the clock by `seconds`. */
  tick(seconds?: number): void;
}

/** A backend over a fresh store, with a clock that only moves when told to. */
export async function harness(
  options: { poolSize?: number; hardware?: { cores?: number; memoryGb?: number } } = {},
): Promise<Harness> {
  const engine = await loadEngine();
  const store = new MemoryStore();
  const pool = inProcessPool(engine, options.poolSize ?? 2);
  let now = Date.UTC(2026, 9, 3, 12, 0, 0);
  let seeds = 0;
  const local = createLocalEngine({
    engine,
    store,
    pool,
    hardware: options.hardware ?? { cores: 8 },
    now: () => new Date(now),
    // A distinct, reproducible seed for each run.
    random: () => ((seeds++ * 0.6180339887) % 1) || 0.5,
  });
  return {
    engine,
    store,
    pool,
    local,
    api: local.api,
    runtime: local.runtime,
    tick: (seconds = 1) => {
      now += seconds * 1000;
    },
  };
}

/** The answers of guided setup for a plan with accounts, a salary and spending: a plan worth running. */
export function setupAnswers(profileId: number, overrides: Record<string, unknown> = {}) {
  return {
    request_id: `setup-${Math.random().toString(36).slice(2, 10)}`,
    name: "Retirement",
    start_date: "2026-01-01",
    birth_date: "1985-06-15",
    duration_years: 30,
    retirement_age: 65,
    cash: 20_000,
    retirement_401k: 150_000,
    investments: 40_000,
    stock_percent: 70,
    cash_profile_id: profileId,
    stock_profile_id: profileId,
    bond_profile_id: profileId,
    investment_tax_status: "Taxable",
    annual_income: 120_000,
    retirement_401k_contribution_percent: 10,
    annual_spending: 60_000,
    retirement_spending: 55_000,
    inflation_profile_id: null,
    tax_config_id: null,
    fund_from_investments: true,
    assumptions_confirmed: true,
    ...overrides,
  };
}

/** Polls until `done` answers something other than undefined. */
export async function waitFor<T>(
  what: string,
  done: () => Promise<T | undefined>,
  { timeoutMs = 60_000, everyMs = 15 } = {},
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = await done();
    if (value !== undefined) return value;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, everyMs));
  }
}
