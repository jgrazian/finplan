use super::*;
#[tokio::test]
async fn guided_setup_retries_preserve_one_reconciled_plan_and_compile() {
    let mut app = TestApp::new().await;
    app.login_as("setup@example.com").await;
    let (_, profiles) = app.get("/api/return-profiles").await;
    let profile = profiles[0]["id"].as_i64().unwrap();
    let body = json!({"request_id":"setup-fixture-1","name":"Guided","start_date":"2026-01-01","birth_date":"1981-01-01","duration_years":50,"retirement_age":65,"cash":50000,"retirement_401k":350000,"investments":150000,"stock_percent":60,"cash_profile_id":profile,"stock_profile_id":profile,"bond_profile_id":profile,"investment_tax_status":"Taxable","annual_income":100000,"annual_spending":40000,"retirement_spending":40000,"inflation_profile_id":null,"tax_config_id":null,"fund_from_investments":true,"assumptions_confirmed":true});
    let (status, created) = app.post("/api/scenarios/setup", body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let sid = created["scenario_id"].as_i64().unwrap();
    let (status, retry) = app.post("/api/scenarios/setup", body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created, retry);
    let (_, accounts) = app.get(&format!("/api/scenarios/{sid}/accounts")).await;
    assert_eq!(accounts.as_array().unwrap().len(), 3);
    for (name, expected) in [("401(k)", 350000.), ("Other investments", 150000.)] {
        let investment = accounts
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .unwrap();
        let aid = investment["id"].as_i64().unwrap();
        let (_, positions) = app
            .get(&format!("/api/scenarios/{sid}/accounts/{aid}/positions"))
            .await;
        assert_eq!(
            positions
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["units"].as_f64().unwrap())
                .sum::<f64>(),
            expected
        );
    }
    let (status, report) = app
        .post(&format!("/api/scenarios/{sid}/compile"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let (_, preflight) = app.get(&format!("/api/scenarios/{sid}/preflight")).await;
    assert_eq!(preflight["can_run"], true, "{preflight}");
    let mut changed = body.clone();
    changed["cash"] = json!(60000);
    assert_eq!(
        app.post("/api/scenarios/setup", changed).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut invalid = body;
    invalid["request_id"] = json!("setup-fixture-2");
    invalid["name"] = json!("Bad setup");
    invalid["stock_profile_id"] = json!(999999);
    assert_eq!(
        app.post("/api/scenarios/setup", invalid).await.0,
        StatusCode::BAD_REQUEST
    );
    let (_, scenarios) = app.get("/api/scenarios").await;
    assert_eq!(
        scenarios
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["name"] == "Bad setup")
            .count(),
        0
    );
    app.login_as("other-setup@example.com").await;
    assert_eq!(
        app.get(&format!("/api/scenarios/{sid}/preflight")).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn guided_setup_without_investments_only_needs_a_cash_profile() {
    let mut app = TestApp::new().await;
    app.login_as("cash-only-setup@example.com").await;
    let (_, profiles) = app.get("/api/return-profiles").await;
    let profile = profiles[0]["id"].as_i64().unwrap();
    let body = json!({
        "request_id": "cash-only-setup-fixture",
        "name": "Cash only",
        "start_date": "2026-01-01",
        "birth_date": "1981-01-01",
        "duration_years": 50,
        "retirement_age": 65,
        "cash": 50000,
        "retirement_401k": 0,
        "investments": 0,
        "stock_percent": 60,
        "cash_profile_id": profile,
        "stock_profile_id": 0,
        "bond_profile_id": 0,
        "investment_tax_status": "Taxable",
        "annual_income": 100000,
        "annual_spending": 40000,
        "retirement_spending": 40000,
        "inflation_profile_id": null,
        "tax_config_id": null,
        "fund_from_investments": false,
        "assumptions_confirmed": true
    });

    let (status, created) = app.post("/api/scenarios/setup", body).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let sid = created["scenario_id"].as_i64().unwrap();
    let (_, accounts) = app.get(&format!("/api/scenarios/{sid}/accounts")).await;
    assert_eq!(accounts.as_array().unwrap().len(), 1);
    assert_eq!(accounts[0]["name"], "Checking");
}
