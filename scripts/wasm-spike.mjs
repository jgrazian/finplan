#!/usr/bin/env node
// Phase 0 spike, WebAssembly side (spec 19): the local run the native example
// (crates/finplan_wasm/examples/spike_native.rs) drives, in Node's WASM engine
// (V8, the one Chrome and Edge use), single-threaded.
//
//   scripts/build-wasm.sh
//   node scripts/wasm-spike.mjs crates/finplan_plan/testdata/default_snapshot.json 1000 42 [/tmp/wasm.json]
//
// Prints one JSON line: cold start, the run's timings, wasm memory afterwards
// and the statistics to compare with the native run. The optional last argument
// receives the run's RunResults JSON, to diff against the native one.
import { readFileSync, writeFileSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { fileURLToPath, pathToFileURL } from "node:url";

const pkg = (file) => fileURLToPath(new URL(`../web/lib/engine/pkg/${file}`, import.meta.url));
const [snapshotPath, iterationsArg = "1000", seedArg = "42", out] = process.argv.slice(2);
if (!snapshotPath) {
  console.error("usage: wasm-spike.mjs <snapshot.json> [iterations] [seed] [results-out.json]");
  process.exit(2);
}
const iterations = Number(iterationsArg);
const seed = Number(seedArg);
const snapshot = readFileSync(snapshotPath, "utf8");

// Cold start: fetch-free, from bytes already in memory, through the first call.
const bytes = readFileSync(pkg("finplan_wasm_bg.wasm"));
const loaded = performance.now();
const engine = await import(pathToFileURL(pkg("finplan_wasm.js")).href);
const imported = performance.now();
const exports = engine.initSync({ module: bytes });
const instantiated = performance.now();
const library = engine.library_seed();
const seeded = performance.now();
const memoryAfterStart = exports.memory.buffer.byteLength;

const settings = JSON.stringify({ iterations, seed });
const started = performance.now();
const coordinator = engine.coordinator_new(snapshot, settings);
const prepared = engine.prepare(snapshot, settings);
const prepareMs = performance.now() - started;

const batchesStart = performance.now();
let batchOutputBytes = 0;
let peakMemory = exports.memory.buffer.byteLength;
for (let specs = engine.coordinator_next_round(coordinator); specs; specs = engine.coordinator_next_round(coordinator)) {
  // Strings stay strings: iteration seeds are 64-bit.
  const outputs = JSON.parse(specs).map((spec) => engine.run_batch(prepared, JSON.stringify(spec)));
  batchOutputBytes += outputs.reduce((n, o) => n + o.length, 0);
  engine.coordinator_absorb(coordinator, `[${outputs.join(",")}]`);
  peakMemory = Math.max(peakMemory, exports.memory.buffer.byteLength);
}
const batchesMs = performance.now() - batchesStart;

const finishStart = performance.now();
const results = engine.coordinator_finish(coordinator);
const finishMs = performance.now() - finishStart;
const totalMs = performance.now() - started;
peakMemory = Math.max(peakMemory, exports.memory.buffer.byteLength);
engine.coordinator_drop(coordinator);
engine.release(prepared);

const parsed = JSON.parse(results);
const median = parsed.stats.percentile_values.find((v) => v.percentile === 0.5)?.final_net_worth;
console.log(
  JSON.stringify({
    engine: "wasm",
    node: process.version,
    iterations,
    cold_start_ms: {
      import_js: imported - loaded,
      instantiate: instantiated - imported,
      first_call_library_seed: seeded - instantiated,
      total: seeded - loaded,
    },
    memory_after_start_mb: memoryAfterStart / 1048576,
    prepare_ms: prepareMs,
    batches_ms: batchesMs,
    finish_ms: finishMs,
    total_ms: totalMs,
    iterations_per_s: iterations / (batchesMs / 1000),
    batch_output_bytes: batchOutputBytes,
    results_bytes: results.length,
    wasm_memory_mb: exports.memory.buffer.byteLength / 1048576,
    wasm_peak_memory_mb: peakMemory / 1048576,
    library_bytes: library.length,
    success_rate: parsed.stats.success_rate,
    median_final_net_worth: median,
    mean_final_net_worth: parsed.stats.mean_final_net_worth,
  }),
);
if (out) writeFileSync(out, results);
