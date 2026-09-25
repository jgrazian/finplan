use super::*;

/// One step for the plan and one per layer, all on the same seed: a large
/// cost lowers success, and a harsh crash on top lowers it further.
#[tokio::test]
async fn what_if_steps_are_cumulative_and_a_harsh_shock_lowers_success() {
    let mut app = TestApp::new().await;
    app.login_as("what-if-analysis@example.com").await;
    let (scenario, _, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");

    let layers = json!([
        {"kind": "one-off", "age": 50, "amount": -60_000.0, "account_id": null},
        {"kind": "market-shock", "age": 42, "drop": 0.85}
    ]);
    let (status, job) = app
        .post(
            &format!("{base}/analyses"),
            json!({"kind": "what-if", "layers": layers, "iterations": 300}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["kind"], "what-if");
    // The budget is for the whole stack: 300 over the plan and two layers.
    assert_eq!(job["total"], 300);
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");

    let (status, result) = app.get(&format!("/api/analyses/{id}/results")).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["kind"], "what-if");
    let steps = result["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 3);
    let success = |i: usize| steps[i]["point"]["success_rate"].as_f64().unwrap();
    assert!(success(1) <= success(0), "{result}");
    assert!(
        success(2) < success(1),
        "a crash should lower success: {} -> {}",
        success(1),
        success(2)
    );
    let median = |i: usize| steps[i]["median_end_real"].as_f64().unwrap();
    assert!(median(2) < median(1) && median(1) < median(0));

    // Fans in today's dollars on the plan's own grid, aged from the birth date.
    let years = result["years"].as_array().unwrap();
    let ages = result["ages"].as_array().unwrap();
    assert_eq!(years.len(), ages.len());
    assert_eq!(
        result["plan_fan"]["p50"].as_array().unwrap().len(),
        years.len()
    );
    assert_eq!(
        result["what_if_fan"]["p50"].as_array().unwrap().len(),
        years.len()
    );
    assert!((ages[0].as_f64().unwrap() - 41.0).abs() < 0.01, "{ages:?}");
    assert!(result["plan_retirement_age"].is_null());

    // The same seed and no layers: one step, identical to the plan.
    let (status, job) = app
        .post(
            &format!("{base}/analyses"),
            json!({"kind": "what-if", "layers": [], "iterations": 100}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");
    let (_, only) = app.get(&format!("/api/analyses/{id}/results")).await;
    assert_eq!(only["steps"].as_array().unwrap().len(), 1);
    assert_eq!(only["steps"][0], steps[0]);

    // Bounds: nine layers, a drop of 100%, an unknown parameter.
    let nine: Vec<Value> = (0..9)
        .map(|_| json!({"kind": "market-shock", "age": 45, "drop": 0.1}))
        .collect();
    for layers in [
        json!(nine),
        json!([{"kind": "market-shock", "age": 45, "drop": 1.0}]),
        json!([{"kind": "parameter", "parameter_id": 999_999, "value": 1.0}]),
    ] {
        let (status, body) = app
            .post(
                &format!("{base}/analyses"),
                json!({"kind": "what-if", "layers": layers}),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }

    // An oversized budget is clamped to the ceiling rather than refused: the
    // screen asks for a fixed refinement size whatever the account allows.
    let (status, job) = app
        .post(
            &format!("{base}/analyses"),
            json!({"kind": "what-if", "layers": [], "iterations": 10_000_000}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert!(job["total"].as_i64().unwrap() <= 2_000, "{job}");
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");
}

/// Age layers need a birth date to mean anything.
#[tokio::test]
async fn age_layers_without_a_birth_date_are_refused() {
    let mut app = TestApp::new().await;
    app.login_as("what-if-no-birth@example.com").await;
    let (status, scenario) = app
        .post(
            "/api/scenarios",
            json!({"name": "Ageless", "start_date": "2026-01-01", "duration_years": 5}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = scenario["id"].as_i64().unwrap();
    let (status, body) = app
        .post(
            &format!("/api/scenarios/{id}/analyses"),
            json!({"kind": "what-if",
                   "layers": [{"kind": "market-shock", "age": 60, "drop": 0.3}]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("birth date"), "{body}");
}

#[tokio::test]
async fn what_if_stack_round_trips_and_is_private() {
    let mut app = TestApp::new().await;
    app.login_as("what-if-stack@example.com").await;
    let (scenario, checking, _) = app.seed_scenario().await;
    let path = format!("/api/scenarios/{scenario}/what-if");

    let (status, empty) = app.get(&path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty, json!({"entries": []}));

    // Stale parameter ids are the client's business; they are stored as-is.
    let stack = json!({"entries": [
        {"id": "a", "enabled": true,
         "layer": {"kind": "parameter", "parameter_id": 424_242, "value": 62.0}},
        {"id": "b", "enabled": false,
         "layer": {"kind": "market-shock", "age": 67, "drop": 0.3}},
        {"id": "c", "enabled": true,
         "layer": {"kind": "one-off", "age": 58, "amount": -40_000.0, "account_id": checking}}
    ]});
    let (status, _) = app.put(&path, stack.clone()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, read) = app.get(&path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read, stack);

    let too_many: Vec<Value> = (0..17)
        .map(|i| {
            json!({"id": format!("e{i}"), "enabled": true,
                        "layer": {"kind": "market-shock", "age": 60, "drop": 0.2}})
        })
        .collect();
    let (status, _) = app.put(&path, json!({"entries": too_many})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    app.login_as("what-if-stranger@example.com").await;
    let (status, _) = app.get(&path).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.put(&path, json!({"entries": []})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Applying to the plan itself sets the parameter (last layer wins), adds a
/// fires-once age event per shock and one-off, and clears the stored stack.
#[tokio::test]
async fn apply_to_self_sets_parameters_and_creates_events() {
    let mut app = TestApp::new().await;
    app.login_as("what-if-apply@example.com").await;
    let (scenario, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");

    let (status, retire) = app
        .post(
            &format!("{base}/parameters"),
            json!({"name": "Retirement age", "value": {"kind": "Age", "years": 65, "months": 0}}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{retire}");
    let retire = retire["id"].as_i64().unwrap();

    let (status, _) = app
        .put(
            &format!("{base}/what-if"),
            json!({"entries": [{"id": "x", "enabled": true,
                "layer": {"kind": "market-shock", "age": 45, "drop": 0.3}}]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The analysis reports the retirement age moving.
    let layers = json!([
        {"kind": "parameter", "parameter_id": retire, "value": 60.0},
        {"kind": "parameter", "parameter_id": retire, "value": 62.5},
        {"kind": "market-shock", "age": 45, "drop": 0.3},
        {"kind": "one-off", "age": 48, "amount": -40_000.0, "account_id": null},
        {"kind": "one-off", "age": 49, "amount": 25_000.0, "account_id": checking}
    ]);
    let (status, job) = app
        .post(
            &format!("{base}/analyses"),
            json!({"kind": "what-if", "layers": layers, "iterations": 25}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");
    let (_, result) = app.get(&format!("/api/analyses/{id}/results")).await;
    assert_eq!(result["steps"].as_array().unwrap().len(), 6);
    assert_eq!(result["plan_retirement_age"], 65.0);
    assert_eq!(result["what_if_retirement_age"], 62.5);

    let (status, applied) = app
        .post(&format!("{base}/what-if/apply"), json!({"layers": layers}))
        .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["id"], scenario);

    let (_, params) = app.get(&format!("{base}/parameters")).await;
    let param = params
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == retire)
        .unwrap();
    assert_eq!(
        param["value"],
        json!({"kind": "Age", "years": 62, "months": 6})
    );

    let (_, events) = app.get(&format!("{base}/events")).await;
    let events = events.as_array().unwrap();
    let named = |name: &str| {
        events
            .iter()
            .find(|e| e["name"] == name)
            .unwrap_or_else(|| panic!("no event {name}: {events:?}"))
    };
    let shock = named("Market shock −30% at 45");
    assert_eq!(shock["fires_once"], true);
    assert_eq!(
        shock["trigger"],
        json!({"kind": "Age", "years": 45, "months": null})
    );
    assert_eq!(
        shock["effects"],
        json!([{"kind": "MarketShock", "drop": 0.3}])
    );
    let cost = named("One-off cost $40k at 48");
    assert_eq!(cost["effects"][0]["kind"], "Expense");
    assert_eq!(cost["effects"][0]["from_account_id"], checking);
    let windfall = named("Windfall $25k at 49");
    assert_eq!(windfall["effects"][0]["kind"], "Income");
    assert_eq!(windfall["effects"][0]["to_account_id"], checking);

    // The plan still compiles and runs with the new effect in it.
    let (status, report) = app.post(&format!("{base}/compile"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    let (_, stack) = app.get(&format!("{base}/what-if")).await;
    assert_eq!(stack, json!({"entries": []}));
}

/// Applying into a new scenario duplicates first and writes through the id
/// maps: the copy's parameter and account, never the original's.
#[tokio::test]
async fn apply_to_new_scenario_maps_ids_onto_the_copy() {
    let mut app = TestApp::new().await;
    app.login_as("what-if-copy@example.com").await;
    let (scenario, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");

    let (status, spending) = app
        .post(
            &format!("{base}/parameters"),
            json!({"name": "Spending", "value": {"kind": "Money", "value": 50_000.0}}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let spending = spending["id"].as_i64().unwrap();

    let (status, _) = app
        .put(
            &format!("{base}/what-if"),
            json!({"entries": [{"id": "keep", "enabled": true,
                "layer": {"kind": "parameter", "parameter_id": spending, "value": 42_000.0}}]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, copy) = app
        .post(
            &format!("{base}/what-if/apply"),
            json!({
                "layers": [
                    {"kind": "parameter", "parameter_id": spending, "value": 42_000.0},
                    {"kind": "one-off", "age": 50, "amount": -10_000.0, "account_id": checking}
                ],
                "new_scenario_name": "Plan (what-if)"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{copy}");
    let copy_id = copy["id"].as_i64().unwrap();
    assert_ne!(copy_id, scenario);
    assert_eq!(copy["name"], "Plan (what-if)");

    // The original is untouched, and keeps its stack.
    let (_, params) = app.get(&format!("{base}/parameters")).await;
    assert_eq!(params[0]["value"]["value"], 50_000.0);
    let (_, events) = app.get(&format!("{base}/events")).await;
    assert!(events.as_array().unwrap().is_empty());
    let (_, stack) = app.get(&format!("{base}/what-if")).await;
    assert_eq!(stack["entries"].as_array().unwrap().len(), 1);

    // The copy has the new value on its own parameter row and the event on
    // its own checking account.
    let copy_base = format!("/api/scenarios/{copy_id}");
    let (_, params) = app.get(&format!("{copy_base}/parameters")).await;
    assert_ne!(params[0]["id"], spending);
    assert_eq!(params[0]["value"]["value"], 42_000.0);
    let (_, accounts) = app.get(&format!("{copy_base}/accounts")).await;
    let copy_checking = accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "Checking")
        .unwrap()["id"]
        .clone();
    assert_ne!(copy_checking, checking);
    let (_, events) = app.get(&format!("{copy_base}/events")).await;
    assert_eq!(events[0]["name"], "One-off cost $10k at 50");
    assert_eq!(events[0]["effects"][0]["from_account_id"], copy_checking);

    // A name clash is a conflict, and leaves nothing behind.
    let (status, _) = app
        .post(
            &format!("{base}/what-if/apply"),
            json!({"layers": [], "new_scenario_name": "Plan (what-if)"}),
        )
        .await;
    assert!(status.is_client_error(), "{status}");
    let (_, scenarios) = app.get("/api/scenarios").await;
    assert_eq!(scenarios.as_array().unwrap().len(), 2);
}

/// The quick route answers in the response, and on the same seed and budget
/// it is the same answer the job route gives.
#[tokio::test]
async fn quick_what_if_answers_inline_and_matches_the_job() {
    let mut app = TestApp::new().await;
    app.login_as("what-if-quick@example.com").await;
    let (scenario, _, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");
    let layers = json!([{"kind": "one-off", "age": 50, "amount": -60_000.0, "account_id": null}]);

    let (status, quick) = app
        .post(
            &format!("{base}/what-if/quick"),
            json!({"layers": layers, "iterations": 200}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{quick}");
    assert_eq!(quick["steps"].as_array().unwrap().len(), 2);

    let (status, job) = app
        .post(
            &format!("{base}/analyses"),
            json!({"kind": "what-if", "layers": layers, "iterations": 200}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");
    let (_, result) = app.get(&format!("/api/analyses/{id}/results")).await;
    assert_eq!(quick["steps"], result["steps"]);

    // Same validation as the job route.
    let (status, body) = app
        .post(
            &format!("{base}/what-if/quick"),
            json!({"layers": [{"kind": "market-shock", "age": 45, "drop": 1.0}]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}
