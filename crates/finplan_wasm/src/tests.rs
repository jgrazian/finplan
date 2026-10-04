//! The exports' inner functions, driven natively with JSON in and out at every
//! step, as the browser drives them.

use finplan_core::model::MonteCarloConfig;
use finplan_core::simulation::monte_carlo_simulate_with_config;
use finplan_plan::compile::compile;
use finplan_plan::graph::ScenarioGraph;
use finplan_plan::library::{Library, LibraryInflationProfile, LibraryTaxConfig};
use finplan_plan::results::{RunResults, RunSettings, project};
use finplan_plan::run::CreateRun;
use finplan_plan::specs::taxes::Bracket;
use serde_json::{Value, json};

use crate::error::{EngineError, EngineResult};
use crate::{plans, reads, results, runs};

pub(crate) const DEFAULT_SNAPSHOT: &str =
    include_str!("../../finplan_plan/testdata/default_snapshot.json");
const GOLDEN_JSON: &str = include_str!("../../finplan_plan/testdata/default.snapshot.json");
const GOLDEN_HASH: &str = include_str!("../../finplan_plan/testdata/default.snapshot.sha256");

pub(crate) fn ok<T>(result: EngineResult<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.to_json()))
}

pub(crate) fn value(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// The library an attached graph carries, with the tax config and inflation
/// profile its scenario row names (a serialized graph keeps only the
/// denormalized copy of those two).
fn library_of(graph: &ScenarioGraph) -> Library {
    let mut library = Library::from_graph(graph);
    if let (Some(tax), Some(id)) = (&graph.tax_config, graph.scenario.tax_config_id) {
        library.tax_configs.push(LibraryTaxConfig {
            id,
            name: tax.name.clone(),
            description: None,
            state_rate: tax.state_rate,
            capital_gains_rate: tax.capital_gains_rate,
            early_withdrawal_penalty_rate: tax.early_withdrawal_penalty_rate,
            standard_deduction: tax.standard_deduction,
            age_65_extra_deduction: tax.age_65_extra_deduction,
            federal_brackets: graph
                .tax_brackets
                .iter()
                .map(|b| Bracket {
                    threshold: b.threshold,
                    rate: b.rate,
                })
                .collect(),
        });
    }
    if let (Some(id), Some(name), Some(distribution_id)) = (
        graph.scenario.inflation_profile_id,
        &graph.inflation_profile_name,
        graph.inflation_distribution_id,
    ) {
        library.inflation_profiles.push(LibraryInflationProfile {
            id,
            name: name.clone(),
            description: None,
            distribution_id,
            sort_order: 0,
        });
    }
    library
}

pub(crate) fn library_json_of(graph_json: &str) -> String {
    let graph: ScenarioGraph = serde_json::from_str(graph_json).unwrap();
    serde_json::to_string(&library_of(&graph)).unwrap()
}

// ── the model ──────────────────────────────────────────────────────────────

#[test]
fn the_snapshot_and_its_hash_are_the_servers() {
    let library = library_json_of(GOLDEN_JSON);
    let out = value(&ok(plans::snapshot_json(GOLDEN_JSON, &library)));
    assert_eq!(out["json"].as_str().unwrap(), GOLDEN_JSON.trim_end());
    assert_eq!(out["hash"].as_str().unwrap(), GOLDEN_HASH.trim());
    assert_eq!(
        plans::model_version(),
        finplan_plan::snapshot::MODEL_VERSION
    );
}

// ── plans ──────────────────────────────────────────────────────────────────

fn new_plan(library: &str) -> String {
    ok(plans::new_plan_json(
        &json!({"name": " Test ", "start_date": "2026-01-01", "birth_date": "1990-05-01",
                "duration_years": 40})
        .to_string(),
        library,
        7,
        "2026-10-03 14:05:09",
    ))
}

#[test]
fn a_plan_is_made_edited_and_read_back() {
    let library = ok(plans::library_seed_json());
    let seeded: Library = serde_json::from_str(&library).unwrap();
    assert!(!seeded.return_profiles.is_empty());
    let profile = seeded.return_profiles[0].id;

    let graph = new_plan(&library);
    let scenario = value(&ok(reads::read_json(
        &graph,
        &library,
        &json!({"query": "scenario"}).to_string(),
    )));
    assert_eq!(scenario["name"], "Test");
    assert_eq!(scenario["id"], 7);
    assert_eq!(scenario["slug"], "7");
    assert_eq!(scenario["status"], "active");

    // Edit: a bank account, an asset, a rename.
    let edit = |graph: &str, op: Value, now: Option<&str>| {
        value(&ok(plans::apply_edit_json(
            graph,
            &library,
            &op.to_string(),
            now,
        )))
    };
    let made = edit(
        &graph,
        json!({"op": "create_account", "body": {
            "name": "Checking", "flavor": "Bank", "cash_value": 1500.0,
            "return_profile_id": profile}}),
        Some("2026-10-04 08:00:00"),
    );
    assert_eq!(made["outcome"]["id"], 1);
    let graph = made["graph"].to_string();
    let made = edit(
        &graph,
        json!({"op": "create_asset", "body": {
            "name": "VTI", "initial_price": 250.0, "return_profile_id": profile}}),
        None,
    );
    assert_eq!(made["outcome"]["id"], 1);
    let graph = made["graph"].to_string();
    let renamed = edit(
        &graph,
        json!({"op": "update_scenario", "body": {"name": "Renamed"}}),
        None,
    );
    assert!(renamed["outcome"]["id"].is_null());
    let graph = renamed["graph"].to_string();

    let read = |query: Value| value(&ok(reads::read_json(&graph, &library, &query.to_string())));
    assert_eq!(read(json!({"query": "scenario"}))["name"], "Renamed");
    assert_eq!(
        read(json!({"query": "scenario"}))["updated_at"],
        "2026-10-04 08:00:00"
    );
    let accounts = read(json!({"query": "accounts"}));
    assert_eq!(accounts.as_array().unwrap().len(), 1);
    assert_eq!(accounts[0]["name"], "Checking");
    assert_eq!(
        read(json!({"query": "account", "id": 1}))["cash_value"],
        1500.0
    );
    assert_eq!(read(json!({"query": "assets"}))[0]["name"], "VTI");
    assert_eq!(
        read(json!({"query": "asset", "id": 1}))["initial_price"],
        250.0
    );
    assert!(
        read(json!({"query": "positions", "account_id": 1}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        read(json!({"query": "events"}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        read(json!({"query": "parameters"}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(read(json!({"query": "compile_report"}))["ok"], true);
    assert!(read(json!({"query": "preflight"}))["issues"].is_array());
    assert!(
        !read(json!({"query": "history_presets"}))
            .as_array()
            .unwrap()
            .is_empty()
    );

    // The snapshot is a function of the plan, and moves with an edit.
    let first = value(&ok(plans::snapshot_json(&graph, &library)));
    let again = value(&ok(plans::snapshot_json(&graph, &library)));
    assert_eq!(first["hash"], again["hash"]);
    let deleted = edit(&graph, json!({"op": "delete_asset", "id": 1}), None);
    let second = value(&ok(plans::snapshot_json(
        &deleted["graph"].to_string(),
        &library,
    )));
    assert_ne!(first["hash"], second["hash"]);

    // A copy is a plan of its own.
    let copy = ok(plans::duplicate_plan_json(
        &graph,
        9,
        "Copy",
        "2026-10-05 00:00:00",
    ));
    let copy = value(&copy);
    assert_eq!(copy["scenario"]["id"], 9);
    assert_eq!(copy["scenario"]["name"], "Copy");
}

#[test]
fn refusals_carry_the_servers_status_code_and_body() {
    let library = ok(plans::library_seed_json());
    let graph = new_plan(&library);

    let missing = plans::apply_edit_json(
        &graph,
        &library,
        &json!({"op": "delete_account", "id": 99}).to_string(),
        None,
    )
    .unwrap_err();
    assert_eq!((missing.status, missing.code.as_str()), (404, "not_found"));
    assert_eq!(missing.message, "account not found");
    assert_eq!(
        missing.body,
        Some(json!({"error": {"code": "not_found", "message": "account not found"}}))
    );

    let blank = plans::new_plan_json(
        &json!({"name": " ", "start_date": "2026-01-01", "duration_years": 30}).to_string(),
        &library,
        1,
        "now",
    )
    .unwrap_err();
    assert_eq!((blank.status, blank.code.as_str()), (400, "bad_request"));

    // A bad request names a position, never the text.
    let broken =
        plans::apply_edit_json("{\"scenario\": secret}", &library, "{}", None).unwrap_err();
    assert_eq!(broken.status, 400);
    assert!(broken.message.contains("line 1"), "{}", broken.message);
    assert!(!broken.message.contains("secret"));

    // Thrown as JSON a client reads back.
    let thrown: EngineError = serde_json::from_str(&missing.to_json()).unwrap();
    assert_eq!(thrown, missing);

    let unknown = reads::read_json(&graph, &library, "{\"query\": \"nope\"}").unwrap_err();
    assert_eq!(unknown.status, 400);
}

#[test]
fn the_deferred_tax_rate_is_an_edit_and_a_read() {
    let library = ok(plans::library_seed_json());
    let graph = new_plan(&library);
    let set = |graph: &str, rate: Value| {
        plans::apply_edit_json(
            graph,
            &library,
            &json!({"op": "update_scenario", "body": {"deferred_tax_rate": rate}}).to_string(),
            None,
        )
    };
    let read = |graph: &str| {
        value(&ok(reads::read_json(
            graph,
            &library,
            &json!({"query": "scenario"}).to_string(),
        )))
    };
    assert_eq!(read(&graph)["deferred_tax_rate"], 0.24);
    let set_to = value(&ok(set(&graph, json!(0.3))))["graph"].to_string();
    assert_eq!(read(&set_to)["deferred_tax_rate"], 0.3);

    // Refused with the status the server answers.
    assert_eq!(set(&graph, json!(1.5)).unwrap_err().status, 400);
}

#[test]
fn the_funding_policy_is_an_edit_and_a_read() {
    let library = ok(plans::library_seed_json());
    let graph = new_plan(&library);
    let set = |graph: &str, funding: Value| {
        plans::apply_edit_json(
            graph,
            &library,
            &json!({"op": "set_funding", "body": {"funding": funding}}).to_string(),
            None,
        )
    };
    let on = value(&ok(set(
        &graph,
        json!({"strategy": "TaxFreeFirst", "exclude_accounts": []}),
    )))["graph"]
        .to_string();
    let read = |graph: &str| {
        value(&ok(reads::read_json(
            graph,
            &library,
            &json!({"query": "scenario"}).to_string(),
        )))
    };
    assert_eq!(read(&on)["funding"]["strategy"], "TaxFreeFirst");
    let off = value(&ok(set(&on, Value::Null)))["graph"].to_string();
    assert!(read(&off)["funding"].is_null());

    // Refused with the status the server answers.
    let bad = set(
        &graph,
        json!({"strategy": "ProRata", "bracket_ceiling": 0.1, "exclude_accounts": []}),
    )
    .unwrap_err();
    assert_eq!(bad.status, 400);
}

#[test]
fn the_library_is_edited_and_read_across_plans() {
    let library = ok(plans::library_seed_json());
    let seeded: Library = serde_json::from_str(&library).unwrap();
    let tax = &seeded.tax_configs[0];
    let graph = new_plan(&library);
    let graph = value(&ok(plans::apply_edit_json(
        &graph,
        &library,
        &json!({"op": "update_scenario", "body": {"tax_config_id": tax.id}}).to_string(),
        None,
    )))["graph"]
        .to_string();
    let plans_json = format!("[{graph}]");

    let renamed = value(&ok(plans::apply_library_json(
        &library,
        &plans_json,
        &json!({"op": "update_tax_config", "id": tax.id, "body": {"state_rate": 0.0123}})
            .to_string(),
        Some("2026-10-06 00:00:00"),
    )));
    let new_library = renamed["library"].to_string();
    // The plan holding that tax config follows it, and is stamped.
    let changed = renamed["changed_plans"].as_array().unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["tax_config"]["state_rate"], 0.0123);
    assert_eq!(changed[0]["scenario"]["updated_at"], "2026-10-06 00:00:00");

    let listed = value(&ok(reads::read_library_json(
        &new_library,
        &plans_json,
        &json!({"query": "tax_config", "id": tax.id}).to_string(),
    )));
    assert_eq!(listed["state_rate"], 0.0123);
    let profiles = value(&ok(reads::read_library_json(
        &new_library,
        &plans_json,
        &json!({"query": "return_profiles"}).to_string(),
    )));
    assert_eq!(
        profiles.as_array().unwrap().len(),
        seeded.return_profiles.len()
    );
    assert!(
        !value(&ok(reads::read_library_json(
            &new_library,
            &plans_json,
            &json!({"query": "inflation_profiles"}).to_string(),
        )))
        .as_array()
        .unwrap()
        .is_empty()
    );

    // Unknown ids refuse as the server does.
    let error = reads::read_library_json(
        &new_library,
        &plans_json,
        &json!({"query": "return_profile", "id": 9999}).to_string(),
    )
    .unwrap_err();
    assert_eq!(error.status, 404);
    let error = plans::apply_library_json(
        &new_library,
        &plans_json,
        &json!({"op": "delete_tax_config", "id": 9999}).to_string(),
        None,
    )
    .unwrap_err();
    assert_eq!(error.status, 404);
}

#[test]
fn archives_round_trip() {
    let archive = ok(plans::export_archive_json(&format!("[{DEFAULT_SNAPSHOT}]")));
    let preview = value(&ok(plans::preview_archive_json(&archive)));
    let accounts = value(DEFAULT_SNAPSHOT)["accounts"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(preview["accounts"], accounts);
    assert_eq!(preview["names"].as_array().unwrap().len(), 1);
    let graphs = value(&ok(plans::import_archive_json(&archive)));
    assert_eq!(graphs.as_array().unwrap().len(), 1);
    // What came back is the plan that went in, hash for hash.
    let library = library_json_of(DEFAULT_SNAPSHOT);
    let original = value(&ok(plans::snapshot_json(DEFAULT_SNAPSHOT, &library)))["hash"].clone();
    let mut restored = graphs[0].clone();
    restored["scenario"]["user_id"] = Value::String(String::new());
    let mut original_graph = value(DEFAULT_SNAPSHOT);
    original_graph["scenario"]["user_id"] = Value::String(String::new());
    let a = value(&ok(plans::snapshot_json(&restored.to_string(), &library)))["hash"].clone();
    let b = value(&ok(plans::snapshot_json(
        &original_graph.to_string(),
        &library,
    )))["hash"]
        .clone();
    assert_eq!(a, b);
    assert_ne!(original, Value::Null);

    let error = plans::import_archive_json(r#"{"format": "other", "version": 1, "plans": []}"#)
        .unwrap_err();
    assert_eq!(error.status, 400);
}

// ── runs ───────────────────────────────────────────────────────────────────

pub(crate) fn settings(iterations: i64, seed: i64) -> String {
    json!({"iterations": iterations, "seed": seed}).to_string()
}

/// What the engine and the projection make of the plan in one call.
fn direct(settings: &str) -> String {
    let graph: ScenarioGraph = serde_json::from_str(DEFAULT_SNAPSHOT).unwrap();
    let compiled = compile(&graph).unwrap();
    let run: CreateRun = serde_json::from_str(settings).unwrap();
    let config: MonteCarloConfig = run.mc_config(&run.validate(usize::MAX).unwrap(), run.seed);
    let summary = monte_carlo_simulate_with_config(&compiled.config, &config).unwrap();
    serde_json::to_string(&project(&compiled, &summary, &RunSettings::default())).unwrap()
}

/// The browser's way: a coordinator, a prepared run, JSON at every step, and
/// each round's batches run in reverse order.
pub(crate) fn local(snapshot: &str, settings: &str) -> String {
    let coordinator = ok(runs::coordinator_new(snapshot, settings));
    let prepared = ok(runs::prepare(snapshot, settings));
    let info = value(&ok(runs::coordinator_info_json(coordinator)));
    assert!(info["cost"].as_i64().unwrap() > 0);
    let mut rounds = 0;
    while let Some(specs) = ok(runs::coordinator_next_round(coordinator)) {
        let specs: Vec<Value> = serde_json::from_str(&specs).unwrap();
        // Last batch first: arrival order is not an input.
        let outputs: Vec<String> = specs
            .iter()
            .rev()
            .map(|spec| ok(runs::run_batch_json(prepared, &spec.to_string())))
            .collect();
        ok(runs::coordinator_absorb(
            coordinator,
            &format!("[{}]", outputs.join(",")),
        ));
        rounds += 1;
    }
    assert!(rounds >= 1);
    let results = ok(runs::coordinator_finish(coordinator));
    runs::coordinator_drop(coordinator);
    runs::release(prepared);
    results
}

#[test]
fn a_local_run_equals_the_engines_own() {
    let run = settings(200, 42);
    assert_eq!(local(DEFAULT_SNAPSHOT, &run), direct(&run));
}

#[test]
fn a_local_run_keeps_the_seed_of_each_percentile_path() {
    let results: RunResults =
        serde_json::from_str(&local(DEFAULT_SNAPSHOT, &settings(100, 3))).unwrap();
    for path in &results.paths {
        assert_eq!(
            path.percentile.is_some(),
            path.seed.is_some(),
            "{:?}",
            path.percentile
        );
        if let Some(seed) = &path.seed {
            seed.parse::<u64>().expect("a decimal u64");
        }
    }
    assert!(results.paths.iter().any(|p| p.seed.is_some()));
}

#[test]
fn the_batch_plan_not_the_order_decides_the_result() {
    // A run with another batch shape agrees with the engine run for that shape,
    // and a different seed does not agree with this one.
    let shaped =
        json!({"iterations": 90, "seed": 5, "batch_size": 10, "parallel_batches": 3}).to_string();
    let a = local(DEFAULT_SNAPSHOT, &shaped);
    assert_eq!(a, direct(&shaped));
    assert_ne!(a, local(DEFAULT_SNAPSHOT, &settings(90, 6)));
}

#[test]
fn a_converging_run_asks_for_rounds_until_it_settles() {
    let converging = json!({"iterations": 100, "seed": 11, "converge": true, "batch_size": 50,
        "parallel_batches": 2})
    .to_string();
    let results: RunResults = serde_json::from_str(&local(DEFAULT_SNAPSHOT, &converging)).unwrap();
    assert!(results.stats.num_iterations >= 100);
    assert!(results.stats.converged.is_some());
}

#[test]
fn a_finished_run_answers_the_results_reads() {
    let finished = local(DEFAULT_SNAPSHOT, &settings(100, 3));

    let view = value(&ok(results::results_view_json(&finished, 5, 7, None)));
    assert_eq!(view["run_id"], 5);
    assert_eq!(view["scenario_id"], 7);
    assert_eq!(view["stats"]["num_iterations"], 100);
    assert_eq!(view["path_details"], true);
    let mean = value(&ok(results::results_view_json(
        &finished,
        5,
        7,
        Some("mean"),
    )));
    assert_eq!(mean["series_id"], "mean");

    let page = value(&ok(results::ledger_page_json(
        &finished,
        5,
        &json!({"category": "cash", "limit": 10}).to_string(),
    )));
    assert_eq!(page["run_id"], 5);
    assert!(page["entries"].as_array().unwrap().len() <= 10);

    let error = results::results_view_json(&finished, 5, 7, Some("p50")).unwrap_err();
    assert_eq!((error.status, error.code.as_str()), (400, "bad_request"));
    let error = results::ledger_page_json(&finished, 5, r#"{"category": "bogus"}"#).unwrap_err();
    assert_eq!(error.status, 400);
}

#[test]
fn runs_are_checked_like_the_servers() {
    let cost = value(&ok(runs::run_cost_json(
        DEFAULT_SNAPSHOT,
        &settings(1000, 1),
    )));
    assert_eq!(cost["sample"], 1000);
    assert!(cost["cost"].as_i64().unwrap() > 1000);
    let converging = value(&ok(runs::run_cost_json(
        DEFAULT_SNAPSHOT,
        &json!({"iterations": 100, "converge": true}).to_string(),
    )));
    assert_eq!(converging["sample"], 10_000);

    for (bad, status) in [
        (json!({"iterations": 0, "seed": 1}), 400),
        (json!({"iterations": 10}), 400),
        (json!({"iterations": 10, "seed": -1}), 400),
        (
            json!({"iterations": 10, "seed": 9_007_199_254_740_992_i64}),
            400,
        ),
        (json!({"iterations": 10, "seed": 1, "percentiles": []}), 400),
        (json!({"iterations": 10_000_000_000_i64, "seed": 1}), 400),
    ] {
        let error = runs::coordinator_new(DEFAULT_SNAPSHOT, &bad.to_string()).unwrap_err();
        assert_eq!(error.status, status, "{bad}: {}", error.message);
    }

    // A handle that was never made, or is gone, is not found.
    assert_eq!(runs::coordinator_next_round(999).unwrap_err().status, 404);
    assert_eq!(runs::run_batch_json(999, "{}").unwrap_err().status, 400);
    let spec = r#"{"index": 0, "seed": 1, "iterations": 1}"#;
    assert_eq!(runs::run_batch_json(999, spec).unwrap_err().status, 404);

    // A finished run cannot be finished again, or fed more.
    let snapshot = DEFAULT_SNAPSHOT;
    let handle = ok(runs::coordinator_new(snapshot, &settings(10, 2)));
    let prepared = ok(runs::prepare(snapshot, &settings(10, 2)));
    let specs = ok(runs::coordinator_next_round(handle)).unwrap();
    // Asking twice gives the same round.
    assert_eq!(ok(runs::coordinator_next_round(handle)).unwrap(), specs);
    let specs: Vec<Value> = serde_json::from_str(&specs).unwrap();
    // A short round is refused and merges nothing.
    let one = ok(runs::run_batch_json(prepared, &specs[0].to_string()));
    assert_eq!(
        runs::coordinator_absorb(handle, &format!("[{one}]"))
            .unwrap_err()
            .status,
        400
    );
    assert_eq!(ok(runs::coordinator_completed(handle)), 0);
    let outputs: Vec<String> = specs
        .iter()
        .map(|spec| ok(runs::run_batch_json(prepared, &spec.to_string())))
        .collect();
    ok(runs::coordinator_absorb(
        handle,
        &format!("[{}]", outputs.join(",")),
    ));
    assert_eq!(ok(runs::coordinator_completed(handle)), 10);
    assert!(ok(runs::coordinator_next_round(handle)).is_none());
    ok(runs::coordinator_finish(handle));
    assert_eq!(runs::coordinator_finish(handle).unwrap_err().status, 409);
    runs::coordinator_drop(handle);
    assert_eq!(runs::coordinator_finish(handle).unwrap_err().status, 404);
    runs::release(prepared);
}

#[test]
fn a_reporting_batch_reports_as_it_runs_and_returns_the_same_output() {
    let prepared = ok(runs::prepare(DEFAULT_SNAPSHOT, &settings(120, 1)));
    let spec = r#"{"index": 0, "seed": 7, "iterations": 120}"#;
    let quiet = ok(runs::run_batch_json(prepared, spec));

    let mut reports = Vec::new();
    let loud = ok(runs::run_batch_reporting_json(
        prepared,
        spec,
        &mut |done, total| {
            reports.push((done, total));
            false
        },
    ));
    assert_eq!(loud, quiet, "reporting does not touch the simulation");
    // Every 120 / 50 = 2 simulations, so 60 reports, rising, ending at the total.
    assert_eq!(reports.len(), 60);
    assert!(reports.windows(2).all(|w| w[0].0 < w[1].0));
    assert_eq!(reports.last(), Some(&(120, 120)));

    // Answering true stops the batch: an error, never a short output.
    let stopped = runs::run_batch_reporting_json(prepared, spec, &mut |done, _| done >= 10);
    assert!(stopped.is_err());
    runs::release(prepared);
}
