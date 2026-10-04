//! Opened results, analyses, what-ifs and the local review, driven natively
//! with JSON at every step, as the browser drives them.

use serde_json::{Value, json};

use crate::tests::{DEFAULT_SNAPSHOT, library_json_of, local, ok, settings, value};
use crate::{analysis, plans, reads, results};

// ── opened results ─────────────────────────────────────────────────────────

#[test]
fn an_opened_run_answers_what_the_stored_one_does() {
    let finished = local(DEFAULT_SNAPSHOT, &settings(60, 4));
    let handle = ok(results::results_open(&finished));
    for series in [None, Some("mean"), Some("0.9")] {
        assert_eq!(
            ok(results::results_view_open(handle, 5, 7, series)),
            ok(results::results_view_json(&finished, 5, 7, series)),
        );
    }
    let query = json!({"category": "cash", "limit": 10}).to_string();
    assert_eq!(
        ok(results::ledger_page_open(handle, 5, &query)),
        ok(results::ledger_page_json(&finished, 5, &query)),
    );
    assert_eq!(
        results::results_view_open(handle, 5, 7, Some("p50"))
            .unwrap_err()
            .status,
        400
    );
    results::results_close(handle);
    assert_eq!(
        results::results_view_open(handle, 5, 7, None)
            .unwrap_err()
            .status,
        404
    );
    assert_eq!(results::results_open("{").unwrap_err().status, 400);
}

// ── guided setup ───────────────────────────────────────────────────────────

#[test]
fn guided_setup_makes_a_plan_that_runs() {
    let library = ok(plans::library_seed_json());
    let profile = value(&library)["return_profiles"][0]["id"]
        .as_i64()
        .unwrap();
    let answers = json!({
        "request_id": "setup-request-1", "name": "Guided", "start_date": "2026-01-01",
        "birth_date": "1985-06-15", "duration_years": 30, "retirement_age": 65,
        "cash": 20000.0, "retirement_401k": 150000.0, "investments": 0.0,
        "stock_percent": 70.0, "cash_profile_id": profile, "stock_profile_id": profile,
        "bond_profile_id": profile, "investment_tax_status": "Taxable",
        "annual_income": 100000.0, "retirement_401k_contribution_percent": 10.0,
        "annual_spending": 50000.0, "retirement_spending": 45000.0,
        "inflation_profile_id": null, "tax_config_id": null,
        "fund_from_investments": true, "assumptions_confirmed": true
    });
    let graph = ok(plans::setup_plan_json(
        &answers.to_string(),
        &library,
        4,
        "2026-10-03 12:00:00",
    ));
    let report = value(&ok(reads::read_json(
        &graph,
        &library,
        &json!({"query": "compile_report"}).to_string(),
    )));
    assert_eq!(report["ok"], true);
    let accounts = value(&ok(reads::read_json(
        &graph,
        &library,
        &json!({"query": "accounts"}).to_string(),
    )));
    assert_eq!(accounts.as_array().unwrap().len(), 2);

    let mut refused = answers.clone();
    refused["assumptions_confirmed"] = json!(false);
    let error = plans::setup_plan_json(&refused.to_string(), &library, 4, "2026-10-03 12:00:00")
        .unwrap_err();
    assert_eq!(error.status, 400);
}

// ── restoring an imported plan ─────────────────────────────────────────────

#[test]
fn an_imported_plan_brings_its_assumptions_into_the_library() {
    let archive = ok(plans::export_archive_json(&format!("[{DEFAULT_SNAPSHOT}]")));
    let graphs = value(&ok(plans::import_archive_json(&archive)));
    let source = graphs[0].to_string();
    let library = ok(plans::library_seed_json());
    let before = value(&library);

    let restored = value(&ok(plans::restore_plan_json(
        &library,
        &source,
        12,
        " Imported ",
        "2026-10-04 09:00:00",
        "ab12cd34",
    )));
    let after = &restored["library"];
    let grew =
        |key: &str| after[key].as_array().unwrap().len() - before[key].as_array().unwrap().len();
    // The starter inflation profile is already there, by name and by value:
    // used, not copied.
    assert_eq!(grew("inflation_profiles"), 0);
    // The snapshot's tax config predates standard deductions, so it is not the
    // starter one despite the name: added beside it, tagged, never substituted.
    assert_eq!(grew("tax_configs"), 1);
    assert_eq!(
        after["tax_configs"].as_array().unwrap().last().unwrap()["name"],
        "US Federal 2024 (single) [ab12cd34]"
    );
    assert!(grew("return_profiles") >= 1);
    assert!(grew("distributions") >= 1);
    // Nothing already in the library was touched.
    for key in [
        "return_profiles",
        "distributions",
        "tax_configs",
        "inflation_profiles",
    ] {
        let kept = before[key].as_array().unwrap().len();
        assert_eq!(
            after[key].as_array().unwrap()[..kept],
            before[key].as_array().unwrap()[..]
        );
    }

    let graph = restored["graph"].to_string();
    let scenario = value(&ok(reads::read_json(
        &graph,
        &restored["library"].to_string(),
        &json!({"query": "scenario"}).to_string(),
    )));
    assert_eq!(scenario["id"], 12);
    assert_eq!(scenario["name"], "Imported");
    // Every profile the plan names is one of the library's own, so it compiles.
    let report = value(&ok(reads::read_json(
        &graph,
        &restored["library"].to_string(),
        &json!({"query": "compile_report"}).to_string(),
    )));
    assert_eq!(report["ok"], true);
    assert!(
        value(&graph)["scenario"]["user_id"]
            .as_str()
            .unwrap()
            .is_empty()
    );

    // A second import (a plan moved away and back) finds the first's rows and
    // adds none: the library does not grow a copy per trip.
    let again = value(&ok(plans::restore_plan_json(
        &restored["library"].to_string(),
        &source,
        13,
        "Imported again",
        "2026-10-04 09:00:00",
        "ef56ab78",
    )));
    assert_eq!(again["library"], *after);
    assert!(!again["library"].to_string().contains("ef56ab78"));

    assert_eq!(
        plans::restore_plan_json(&library, &source, 1, "  ", "now", "x")
            .unwrap_err()
            .status,
        400
    );
}

// ── analysis, what-if, review ──────────────────────────────────────────────

/// The default plan, ten years long, with a retirement age and a spending
/// parameter and the event that follows them: what the analysis tests vary.
fn analysable() -> (String, String, String) {
    let library = library_json_of(DEFAULT_SNAPSHOT);
    let mut graph: Value = value(DEFAULT_SNAPSHOT);
    graph["scenario"]["duration_years"] = json!(10);
    let account = graph["accounts"][0]["id"].as_i64().unwrap();
    let mut graph = graph.to_string();
    let mut edit = |op: Value| -> Option<i64> {
        let out = value(&ok(plans::apply_edit_json(
            &graph,
            &library,
            &op.to_string(),
            Some("2026-10-03 12:00:00"),
        )));
        graph = out["graph"].to_string();
        out["outcome"]["id"].as_i64()
    };
    let retire = edit(json!({"op": "create_parameter", "body": {
        "name": "Retirement age", "value": {"kind": "Age", "years": 45, "months": 0}}}))
    .unwrap();
    edit(json!({"op": "create_parameter", "body": {
        "name": "Spending", "value": {"kind": "Money", "value": 2000.0}}}));
    edit(json!({"op": "create_event", "body": {
        "name": "Retire", "fires_once": true, "enabled": true,
        "trigger": {"kind": "AgeParameter", "parameter_id": retire},
        "effects": [{"kind": "Expense", "from_account_id": account,
            "amount": {"kind": "Expression", "source": "$Spending"}}]}}));
    let parameters = value(&ok(reads::read_json(
        &graph,
        &library,
        &json!({"query": "analysis_parameters"}).to_string(),
    )));
    let spending = parameters
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Spending")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    (graph, library, spending)
}

/// Runs `body` as an analysis, recording every progress report.
fn analysis(graph: &str, library: &str, body: &Value) -> (Value, Vec<(usize, usize)>) {
    let mut reports = Vec::new();
    let out = ok(analysis::analysis_run_json(
        graph,
        library,
        &body.to_string(),
        &mut |done, total| {
            reports.push((done, total));
            false
        },
    ));
    (value(&out), reports)
}

fn sweep_body(spending: &str) -> Value {
    json!({"kind": "sweep", "iterations": 25, "axes": [
        {"parameter_id": spending, "min": 1000.0, "max": 3000.0, "steps": 3}]})
}

#[test]
fn a_sweep_reports_progress_and_answers_for_the_grid() {
    let (graph, library, spending) = analysable();
    let body = sweep_body(&spending);
    let plan = value(&ok(analysis::analysis_plan_json(
        &graph,
        &library,
        &body.to_string(),
    )));
    assert_eq!(plan["kind"], "sweep");
    // Three cells and the plan itself, 25 iterations each.
    assert_eq!(plan["total"], 100);

    let (outcome, reports) = analysis(&graph, &library, &body);
    assert_eq!(outcome["kind"], "sweep");
    assert_eq!(outcome["cells"].as_array().unwrap().len(), 3);
    assert_eq!(reports.first(), Some(&(0, 75)));
    assert_eq!(reports.last(), Some(&(100, 100)));
    assert!(reports.windows(2).all(|pair| pair[0].0 <= pair[1].0));

    // The same request on the same plan gives the same grid.
    assert_eq!(analysis(&graph, &library, &body).0, outcome);
}

#[test]
fn an_analysis_is_stopped_by_its_progress_callback() {
    let (graph, library, spending) = analysable();
    let body = sweep_body(&spending);
    let mut calls = 0;
    let error = analysis::analysis_run_json(&graph, &library, &body.to_string(), &mut |_, _| {
        calls += 1;
        calls >= 2
    })
    .unwrap_err();
    assert_eq!((error.status, error.code.as_str()), (409, "conflict"));
    assert!(calls < 5, "stopped after {calls} reports");
}

#[test]
fn analysis_requests_are_refused_as_the_servers_are() {
    let (graph, library, _) = analysable();
    for body in [
        json!({"kind": "sweep", "axes": []}),
        json!({"kind": "sweep", "axes": [{"parameter_id": "nope"}]}),
    ] {
        let error = analysis::analysis_plan_json(&graph, &library, &body.to_string()).unwrap_err();
        assert!(
            error.status == 400 || error.status == 404,
            "{body}: {}",
            error.to_json()
        );
    }
    assert_eq!(
        analysis::analysis_plan_json(&graph, &library, "{")
            .unwrap_err()
            .status,
        400
    );
}

#[test]
fn a_quick_what_if_answers_in_one_call() {
    let (graph, library, _) = analysable();
    let body = json!({"layers": [{"kind": "market-shock", "age": 40, "drop": 0.4}],
        "iterations": 50})
    .to_string();
    let outcome = value(&ok(analysis::quick_what_if_json(
        &graph,
        &library,
        &body,
        &mut |_, _| false,
    )));
    // The plan and one layer.
    assert_eq!(outcome["steps"].as_array().unwrap().len(), 2);
    assert!(outcome["plan_fan"]["p50"].as_array().unwrap().len() > 2);
}

#[test]
fn a_what_if_applies_to_the_plan_or_to_a_copy() {
    let (graph, library, _) = analysable();
    let layers = json!([{"kind": "market-shock", "age": 40, "drop": 0.4}]);
    let events = |g: &str| value(g)["events"].as_array().unwrap().len();
    let before = events(&graph);

    let in_place = ok(analysis::apply_what_if_json(
        &graph,
        &library,
        &json!({"layers": layers}).to_string(),
        99,
        "2026-10-04 08:00:00",
    ));
    assert_eq!(events(&in_place), before + 1);
    assert_eq!(
        value(&in_place)["scenario"]["updated_at"],
        "2026-10-04 08:00:00"
    );
    assert_ne!(value(&in_place)["scenario"]["id"], 99);

    let copy = ok(analysis::apply_what_if_json(
        &graph,
        &library,
        &json!({"layers": layers, "new_scenario_name": " With a crash "}).to_string(),
        99,
        "2026-10-04 08:00:00",
    ));
    assert_eq!(events(&copy), before + 1);
    assert_eq!(value(&copy)["scenario"]["id"], 99);
    assert_eq!(value(&copy)["scenario"]["name"], "With a crash");

    let error = analysis::apply_what_if_json(
        &graph,
        &library,
        &json!({"layers": [{"kind": "parameter", "parameter_id": "parameter:9999", "value": 1.0}]})
            .to_string(),
        99,
        "2026-10-04 08:00:00",
    )
    .unwrap_err();
    assert!(error.status >= 400, "{}", error.to_json());
}

#[test]
fn a_note_s_path_applies_to_the_plan_once_and_then_it_is_stale() {
    let library = library_json_of(DEFAULT_SNAPSHOT);
    let finished = local(DEFAULT_SNAPSHOT, &settings(100, 3));
    let review = value(&ok(analysis::local_review_json(
        DEFAULT_SNAPSHOT,
        &library,
        &finished,
        5,
        "2026-10-03 12:00:00",
        "[]",
    )));
    let mut applied = 0;
    for note in review["review"]["suggestions"].as_array().unwrap() {
        let Some(path) = note["paths"].as_array().and_then(|paths| paths.first()) else {
            continue;
        };
        let steps = json!(
            path["steps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|step| json!({"key": step["key"], "changes": step["changes"]}))
                .collect::<Vec<_>>()
        )
        .to_string();
        let key = path["key"].as_str().unwrap();
        let Ok(written) = analysis::apply_note_json(
            DEFAULT_SNAPSHOT,
            &library,
            key,
            &steps,
            None,
            "2026-10-04 08:00:00",
        ) else {
            continue;
        };
        applied += 1;
        assert_eq!(
            value(&written)["scenario"]["updated_at"],
            "2026-10-04 08:00:00"
        );
        // The plan is now what the note said, so the same note no longer fits.
        let again = analysis::apply_note_json(&written, &library, key, &steps, None, "now");
        if let Err(error) = again {
            assert!(
                error.status == 409 || error.status == 422,
                "{}",
                error.to_json()
            );
            assert!(error.body.unwrap()["by_step"].is_array());
        }
        // To a copy, the original is untouched.
        let copy = ok(analysis::apply_note_json(
            DEFAULT_SNAPSHOT,
            &library,
            key,
            &steps,
            Some((77, "A copy")),
            "2026-10-04 08:00:00",
        ));
        assert_eq!(value(&copy)["scenario"]["id"], 77);
        break;
    }
    assert_eq!(
        applied, 1,
        "the default plan has a note that can be applied"
    );
    assert_eq!(
        analysis::apply_note_json(DEFAULT_SNAPSHOT, &library, "k", "[{", None, "now")
            .unwrap_err()
            .status,
        400
    );
}

#[test]
fn a_what_if_stack_is_checked_before_it_is_stored() {
    let stack = json!({"entries": [{"id": "a", "enabled": true,
        "layer": {"kind": "market-shock", "age": 50, "drop": 0.3}}]})
    .to_string();
    assert_eq!(
        value(&ok(analysis::check_what_if_stack_json(&stack))),
        value(&stack)
    );
    let unkeyed = json!({"entries": [{"id": "", "enabled": true,
        "layer": {"kind": "market-shock", "age": 50, "drop": 0.3}}]})
    .to_string();
    assert_eq!(
        analysis::check_what_if_stack_json(&unkeyed)
            .unwrap_err()
            .status,
        400
    );
}

#[test]
fn a_local_review_notes_a_run_and_silences_what_was_set_aside() {
    let library = library_json_of(DEFAULT_SNAPSHOT);
    let finished = local(DEFAULT_SNAPSHOT, &settings(100, 3));
    let review = |silenced: &[String]| {
        value(&ok(analysis::local_review_json(
            DEFAULT_SNAPSHOT,
            &library,
            &finished,
            5,
            "2026-10-03 12:00:00",
            &serde_json::to_string(silenced).unwrap(),
        )))
    };
    let first = review(&[]);
    assert_eq!(first["review"]["run_id"], 5);
    let notes = first["review"]["suggestions"].as_array().unwrap();
    let prints = first["fingerprints"].as_array().unwrap();
    assert_eq!(notes.len(), prints.len());
    assert!(
        !notes.is_empty(),
        "the default plan draws at least one rule note"
    );
    assert!(notes.iter().all(|note| note["source"] == "rules"));

    let silenced = vec![prints[0].as_str().unwrap().to_string()];
    let second = review(&silenced);
    assert_eq!(
        second["review"]["suggestions"].as_array().unwrap().len(),
        notes.len() - 1
    );
    assert!(
        !second["fingerprints"]
            .as_array()
            .unwrap()
            .contains(&prints[0])
    );
}

/// `body` split across `shards` workers, then finished: what the pool does.
fn sharded(graph: &str, library: &str, body: &Value, shards: usize) -> (Value, usize) {
    let mut simulated = 0;
    let parts: Vec<String> = (0..shards)
        .map(|shard| {
            let mut last = 0;
            let part = ok(analysis::analysis_shard_json(
                graph,
                library,
                &body.to_string(),
                shard,
                shards,
                &mut |done, _| {
                    last = done;
                    false
                },
            ));
            simulated += last;
            part
        })
        .collect();
    let mut extra = 0;
    let out = ok(analysis::analysis_finish_json(
        graph,
        library,
        &body.to_string(),
        &serde_json::to_string(&parts).unwrap(),
        &mut |done, _| {
            extra = done;
            false
        },
    ));
    assert_eq!(extra, 0, "the finish pass simulated nothing");
    (value(&out), simulated)
}

#[test]
fn a_sweep_split_across_workers_is_the_same_sweep() {
    let (graph, library, spending) = analysable();
    let body = json!({"kind": "sweep", "iterations": 25, "axes": [
        {"parameter_id": spending, "min": 1000.0, "max": 3000.0, "steps": 5}]});
    let (whole, reports) = analysis(&graph, &library, &body);
    // Reported as simulations finish, not once per 25-iteration point.
    assert!(reports.len() > 6 * 2, "{} reports", reports.len());
    for shards in 1..=4 {
        let (split, simulated) = sharded(&graph, &library, &body, shards);
        assert_eq!(split, whole, "{shards} shards");
        // Five cells and the plan: each simulated once, by one shard.
        assert_eq!(simulated, 6 * 25, "{shards} shards");
    }
}

#[test]
fn a_sensitivity_split_across_workers_is_the_same_ranking() {
    let (graph, library, _) = analysable();
    let body = json!({"kind": "sensitivity", "iterations": 25});
    let (whole, _) = analysis(&graph, &library, &body);
    assert_eq!(whole["kind"], "sensitivity");
    for shards in [2, 3] {
        assert_eq!(sharded(&graph, &library, &body, shards).0, whole);
    }
}

#[test]
fn answers_that_do_not_match_their_call_are_simulated_again() {
    let (graph, library, spending) = analysable();
    let body = sweep_body(&spending);
    let (whole, _) = analysis(&graph, &library, &body);
    // Shards of a different sweep: every key differs, so the finish pass
    // simulates the whole grid itself and still answers this one.
    let other = json!({"kind": "sweep", "iterations": 25, "axes": [
        {"parameter_id": spending, "min": 1500.0, "max": 2500.0, "steps": 3}]});
    let parts: Vec<String> = (0..2)
        .map(|shard| {
            ok(analysis::analysis_shard_json(
                &graph,
                &library,
                &other.to_string(),
                shard,
                2,
                &mut |_, _| false,
            ))
        })
        .collect();
    let mut simulated = 0;
    let out = ok(analysis::analysis_finish_json(
        &graph,
        &library,
        &body.to_string(),
        &serde_json::to_string(&parts).unwrap(),
        &mut |done, _| {
            simulated = done;
            false
        },
    ));
    assert_eq!(value(&out), whole);
    assert!(simulated > 0);
}

#[test]
fn the_local_runner_answers_as_the_engines_own_runner() {
    use finplan_core::analysis::ProgressRunner;
    use finplan_plan::analysis::{CreateAnalysis, analyze};
    let (graph, library, spending) = analysable();
    let body = sweep_body(&spending);
    let (local, _) = analysis(&graph, &library, &body);
    let mut attached: finplan_plan::graph::ScenarioGraph = serde_json::from_str(&graph).unwrap();
    let lib: finplan_plan::library::Library = serde_json::from_str(&library).unwrap();
    lib.attach(&mut attached);
    let request: CreateAnalysis = serde_json::from_value(body).unwrap();
    let limits = finplan_plan::analysis::Limits {
        iteration_cap: analysis::LOCAL_ITERATION_CAP,
        parallel_batches: analysis::LOCAL_PARALLEL_BATCHES,
    };
    let engine = analyze(&attached, request, &limits, &mut ProgressRunner::new(None)).unwrap();
    assert_eq!(serde_json::to_value(&engine).unwrap(), local);
}

// ── drawdown ───────────────────────────────────────────────────────────────

/// The seed a finished local run kept for its median path.
fn median_seed(results: &str) -> String {
    value(results)["paths"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["percentile"] == 0.5)
        .and_then(|p| p["seed"].as_str())
        .expect("the median path keeps its seed")
        .to_string()
}

#[test]
fn drawdown_replays_the_median_path_of_a_local_run() {
    let finished = local(DEFAULT_SNAPSHOT, &settings(40, 4));
    let seed = median_seed(&finished);
    let body = value(&ok(analysis::drawdown_json(DEFAULT_SNAPSHOT, &seed, "{}")));
    assert_eq!(body["seed"], seed.as_str());
    assert_eq!(body["choices"].as_array().unwrap().len(), 7);
    assert_eq!(body["choices"][0]["choice"]["kind"], "AsPlanned");
    assert!(!body["choices"][1]["years"].as_array().unwrap().is_empty());

    let one = json!({"strategies": [{"kind": "Strategy", "strategy": "ProRata"}],
                     "retirement_year": 2050})
    .to_string();
    let one = value(&ok(analysis::drawdown_json(DEFAULT_SNAPSHOT, &seed, &one)));
    assert_eq!(one["retirement"]["source"], "request");
    assert_eq!(one["choices"][0]["overlay"], true);

    // The refusals are the server's.
    let refused = |seed: &str, request: &str| {
        analysis::drawdown_json(DEFAULT_SNAPSHOT, seed, request)
            .unwrap_err()
            .status
    };
    assert_eq!(refused("not a seed", "{}"), 400);
    assert_eq!(refused(&seed, r#"{"strategies": []}"#), 400);
    assert_eq!(refused(&seed, "["), 400);
}

#[test]
fn drawdown_compares_strategies_and_reports_progress() {
    let mut reports = 0;
    let body = json!({"request": {"strategies": [
        {"kind": "AsPlanned"}, {"kind": "Strategy", "strategy": "TaxFreeFirst"}]},
        "iterations": 25})
    .to_string();
    let comparison = value(&ok(analysis::drawdown_compare_json(
        DEFAULT_SNAPSHOT,
        &body,
        &mut |_, _| {
            reports += 1;
            false
        },
    )));
    assert!(reports > 0);
    assert_eq!(comparison["iterations"], 25);
    assert_eq!(comparison["rows"].as_array().unwrap().len(), 2);

    let too_many = json!({"iterations": 100_000}).to_string();
    assert_eq!(
        analysis::drawdown_compare_json(DEFAULT_SNAPSHOT, &too_many, &mut |_, _| false)
            .unwrap_err()
            .status,
        400
    );
    // A host that cancels gets the server's "canceled" refusal.
    assert_eq!(
        analysis::drawdown_compare_json(DEFAULT_SNAPSHOT, &body, &mut |_, _| true)
            .unwrap_err()
            .status,
        409
    );
}
