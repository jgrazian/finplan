//! Phase 0 spike, native side (spec 19): the same local run `scripts/wasm-spike.mjs`
//! drives in WebAssembly, single-threaded, so the two are comparable.
//!
//! ```text
//! cargo run --release -p finplan_wasm --example spike_native -- \
//!     crates/finplan_plan/testdata/default_snapshot.json 1000 42 /tmp/native.json
//! ```
//!
//! Prints one JSON line of timings and statistics and writes the run's
//! `RunResults` JSON to the last argument (to compare with the wasm run's).

use std::time::Instant;

use finplan_wasm::runs;
use serde_json::Value;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let snapshot = std::fs::read_to_string(&args[1]).expect("snapshot");
    let iterations: i64 = args[2].parse().expect("iterations");
    let seed: i64 = args[3].parse().expect("seed");
    let out = args.get(4);
    // Batches of 100 in rounds of 4, as the server defaults.
    let settings = format!(r#"{{"iterations": {iterations}, "seed": {seed}}}"#);

    let started = Instant::now();
    let coordinator = runs::coordinator_new(&snapshot, &settings).expect("coordinator");
    let prepared = runs::prepare(&snapshot, &settings).expect("prepare");
    let prepare_ms = started.elapsed().as_secs_f64() * 1000.0;

    let batches = Instant::now();
    let mut batch_bytes = 0usize;
    while let Some(specs) = runs::coordinator_next_round(coordinator).expect("round") {
        let specs: Vec<Value> = serde_json::from_str(&specs).unwrap();
        let outputs: Vec<String> = specs
            .iter()
            .map(|spec| runs::run_batch_json(prepared, &spec.to_string()).expect("batch"))
            .collect();
        batch_bytes += outputs.iter().map(String::len).sum::<usize>();
        runs::coordinator_absorb(coordinator, &format!("[{}]", outputs.join(","))).expect("absorb");
    }
    let batches_ms = batches.elapsed().as_secs_f64() * 1000.0;

    let finish = Instant::now();
    let results = runs::coordinator_finish(coordinator).expect("finish");
    let finish_ms = finish.elapsed().as_secs_f64() * 1000.0;
    let total_ms = started.elapsed().as_secs_f64() * 1000.0;

    let value: Value = serde_json::from_str(&results).unwrap();
    let stats = &value["stats"];
    let median = stats["percentile_values"]
        .as_array()
        .and_then(|p| p.iter().find(|v| v["percentile"] == 0.5))
        .map(|v| v["final_net_worth"].clone());
    println!(
        "{}",
        serde_json::json!({
            "engine": "native",
            "iterations": iterations,
            "prepare_ms": prepare_ms,
            "batches_ms": batches_ms,
            "finish_ms": finish_ms,
            "total_ms": total_ms,
            "iterations_per_s": iterations as f64 / (batches_ms / 1000.0),
            "batch_output_bytes": batch_bytes,
            "results_bytes": results.len(),
            "success_rate": stats["success_rate"],
            "median_final_net_worth": median,
            "mean_final_net_worth": stats["mean_final_net_worth"],
        })
    );
    if let Some(out) = out {
        std::fs::write(out, results).expect("write results");
    }
}
