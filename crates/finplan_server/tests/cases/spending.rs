use super::*;

/// A guardrails `Spending` effect (spec 22) round-trips through the API,
/// compiles, and survives duplication and an archive round trip with its
/// portfolio's accounts rewritten to the copy's.
#[tokio::test]
async fn spending_effects_round_trip_duplicate_and_archive() {
    let mut app = TestApp::new().await;
    app.login_as("spending@example.com").await;
    let (scenario, checking, brokerage) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");

    let event = json!({"name": "Retirement spending",
        "trigger": {"kind": "Repeating", "interval": "Monthly",
                    "start_condition": {"kind": "Date", "on_date": "2027-01-01"}},
        "effects": [{"kind": "Spending", "from_account_id": checking,
                     "rule": {"rule": "Guardrails",
                              "initial": {"kind": "Amount", "amount": 30000.0},
                              "floor": 20000.0, "ceiling": null},
                     "portfolio": {"kind": "Accounts", "account_ids": [brokerage]}}]});
    let (status, created) = app.post(&format!("{base}/events"), event).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let effect = &created["effects"][0];
    assert_eq!(effect["kind"], "Spending");
    assert_eq!(effect["rule"]["rule"], "Guardrails");
    assert_eq!(effect["rule"]["floor"], 20000.0);
    assert_eq!(effect["pass_through"], true);
    assert_eq!(effect["portfolio"]["account_ids"], json!([brokerage]));

    let (status, report) = app.post(&format!("{base}/compile"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    // VPW past the owner's age (born 1985) is refused.
    let vpw = json!({"name": "VPW",
        "trigger": {"kind": "Repeating", "interval": "Yearly"},
        "effects": [{"kind": "Spending", "from_account_id": checking,
                     "rule": {"rule": "Vpw", "final_age": 30}}]});
    let (status, body) = app.post(&format!("{base}/events"), vpw).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let portfolio_of = |events: &Value| {
        events
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == "Retirement spending")
            .unwrap()["effects"][0]["portfolio"]["account_ids"]
            .clone()
    };
    let account_id = |accounts: &Value, name: &str| {
        accounts
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    };

    // Duplicating rewrites the portfolio to the copy's brokerage.
    let (status, clone) = app
        .post(&format!("{base}/duplicate"), json!({"name": "Copy"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{clone}");
    let clone_id = clone["id"].as_i64().unwrap();
    let (_, accounts) = app
        .get(&format!("/api/scenarios/{clone_id}/accounts"))
        .await;
    let (_, events) = app.get(&format!("/api/scenarios/{clone_id}/events")).await;
    let copied = account_id(&accounts, "Brokerage");
    assert_ne!(copied, brokerage);
    assert_eq!(portfolio_of(&events), json!([copied]));

    // And so does an archive restored as a new plan.
    let (status, archive) = app.get(&format!("{base}/archive")).await;
    assert_eq!(status, StatusCode::OK, "{archive}");
    let (status, imported) = app
        .post(
            "/api/archives/import",
            json!({"archive": archive, "name_prefix": "Restored ", "request_id": "spending-1"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let restored = imported["scenario_ids"][0].as_i64().unwrap();
    let (_, accounts) = app
        .get(&format!("/api/scenarios/{restored}/accounts"))
        .await;
    let (_, events) = app.get(&format!("/api/scenarios/{restored}/events")).await;
    assert_eq!(
        portfolio_of(&events),
        json!([account_id(&accounts, "Brokerage")])
    );
    let (status, report) = app
        .post(&format!("/api/scenarios/{restored}/compile"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
}

/// `POST /runs/{id}/spending/compare` runs the default rule list against the
/// run's inputs.
#[tokio::test]
async fn spending_rules_compare_against_a_run() {
    let mut app = TestApp::new().await;
    app.login_as("spending-compare@example.com").await;
    let (scenario, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");
    let (status, body) = app
        .put(
            &format!("{base}/funding"),
            json!({"funding": {"strategy": "TaxEfficientEarly", "exclude_accounts": []}}),
        )
        .await;
    assert!(status.is_success(), "{body}");
    let (status, event) = app
        .post(
            &format!("{base}/events"),
            json!({"name": "Retirement spending",
                   "trigger": {"kind": "Repeating", "interval": "Monthly"},
                   "effects": [{"kind": "Spending", "from_account_id": checking,
                                "rule": {"rule": "Guardrails",
                                         "initial": {"kind": "Amount", "amount": 2400.0}}}]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{event}");
    let (status, run) = app
        .post(
            &format!("{base}/runs"),
            json!({"iterations": 20, "seed": 5}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let run_id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");

    let (status, comparison) = app
        .post(
            &format!("/api/runs/{run_id}/spending/compare"),
            json!({"iterations": 25}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{comparison}");
    assert_eq!(comparison["event_name"], "Retirement spending");
    assert_eq!(comparison["rows"].as_array().unwrap().len(), 6);
    assert_eq!(comparison["rows"][0]["label"], "As planned");
    assert!(comparison["rows"][0]["effect"].is_null());
    assert_eq!(comparison["rows"][3]["effect"]["kind"], "Spending");
}
