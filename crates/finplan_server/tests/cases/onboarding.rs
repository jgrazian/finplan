use super::*;
#[tokio::test]
async fn guided_setup_retries_preserve_one_reconciled_plan_and_compile() {
    let mut app = TestApp::new().await;
    app.login_as("setup@example.com").await;
    let (_, profiles) = app.get("/api/return-profiles").await;
    let profile = profiles[0]["id"].as_i64().unwrap();
    let body = json!({"request_id":"setup-fixture-1","name":"Guided","start_date":"2026-01-01","birth_date":"1981-01-01","duration_years":50,"retirement_age":65,"cash":50000,"retirement_401k":350000,"investments":150000,"stock_percent":60,"cash_profile_id":profile,"stock_profile_id":profile,"bond_profile_id":profile,"investment_tax_status":"Taxable","annual_income":100000,"retirement_401k_contribution_percent":50,"annual_spending":40000,"retirement_spending":40000,"inflation_profile_id":null,"tax_config_id":null,"fund_from_investments":true,"assumptions_confirmed":true});
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
    let (_, events) = app.get(&format!("/api/scenarios/{sid}/events")).await;
    for name in ["Spending before retirement", "Retirement spending"] {
        let event = events
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["name"] == name)
            .unwrap();
        assert_eq!(event["effects"][0]["kind"], "Sweep");
        assert_eq!(event["effects"][1]["kind"], "Expense");
    }
    let salary = events
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["name"] == "Salary until retirement")
        .unwrap();
    assert_eq!(salary["effects"][0]["kind"], "Income");
    assert_eq!(salary["effects"][1]["kind"], "Income");
    assert_eq!(salary["effects"][0]["amount"]["inner"]["value"], 75_500.);
    assert_eq!(salary["effects"][1]["amount"]["inner"]["value"], 24_500.);
    assert!(
        salary["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|effect| effect["kind"] == "AssetPurchase")
    );
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
        "retirement_401k_contribution_percent": 0,
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

#[tokio::test]
async fn guided_setup_can_start_401k_contributions_without_an_opening_balance() {
    let mut app = TestApp::new().await;
    app.login_as("new-401k-setup@example.com").await;
    let (_, profiles) = app.get("/api/return-profiles").await;
    let profile = profiles[0]["id"].as_i64().unwrap();
    let body = json!({
        "request_id": "new-401k-setup-fixture",
        "name": "Start a 401k",
        "start_date": "2026-01-01",
        "birth_date": "1981-01-01",
        "duration_years": 50,
        "retirement_age": 65,
        "cash": 50000,
        "retirement_401k": 0,
        "investments": 0,
        "stock_percent": 60,
        "cash_profile_id": profile,
        "stock_profile_id": profile,
        "bond_profile_id": profile,
        "investment_tax_status": "Taxable",
        "annual_income": 100000,
        "retirement_401k_contribution_percent": 6,
        "annual_spending": 40000,
        "retirement_spending": 40000,
        "inflation_profile_id": null,
        "tax_config_id": null,
        "fund_from_investments": true,
        "assumptions_confirmed": true
    });

    let (status, created) = app.post("/api/scenarios/setup", body).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let sid = created["scenario_id"].as_i64().unwrap();
    let (_, accounts) = app.get(&format!("/api/scenarios/{sid}/accounts")).await;
    assert_eq!(accounts.as_array().unwrap().len(), 2);
    assert!(
        accounts
            .as_array()
            .unwrap()
            .iter()
            .any(|account| account["name"] == "401(k)")
    );

    let (_, events) = app.get(&format!("/api/scenarios/{sid}/events")).await;
    let salary = events
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["name"] == "Salary until retirement")
        .unwrap();
    assert!(
        salary["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|effect| effect["kind"] == "AssetPurchase")
    );

    let (status, report) = app
        .post(&format!("/api/scenarios/{sid}/compile"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
}

/// The plan a setup produced, with row ids replaced by the names they point at,
/// so two runs (or two implementations) can be compared.
async fn dumped_plan(app: &mut TestApp, sid: i64) -> Value {
    let (_, accounts) = app.get(&format!("/api/scenarios/{sid}/accounts")).await;
    let (_, assets) = app.get(&format!("/api/scenarios/{sid}/assets")).await;
    let (_, events) = app.get(&format!("/api/scenarios/{sid}/events")).await;
    let (_, scenario) = app.get(&format!("/api/scenarios/{sid}")).await;
    let (_, profiles) = app.get("/api/return-profiles").await;
    let (_, inflation) = app.get("/api/inflation-profiles").await;
    let name_of = |list: &Value, id: &Value| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == *id)
            .map(|row| row["name"].clone())
            .unwrap_or_else(|| id.clone())
    };
    fn walk(value: &mut Value, lists: &[&Value; 3], name_of: &dyn Fn(&Value, &Value) -> Value) {
        let [accounts, assets, profiles] = lists;
        match value {
            Value::Object(object) => {
                object.remove("id");
                for (key, child) in object.iter_mut() {
                    if key.ends_with("account_id") {
                        *child = name_of(accounts, child);
                    } else if key == "asset_id" {
                        *child = name_of(assets, child);
                    } else if key.ends_with("return_profile_id") {
                        *child = name_of(profiles, child);
                    } else {
                        walk(child, lists, name_of);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| walk(item, lists, name_of)),
            _ => {}
        }
    }
    let mut plan = json!({
        "scenario": {
            "name": scenario["name"], "start_date": scenario["start_date"],
            "birth_date": scenario["birth_date"], "duration_years": scenario["duration_years"],
            "inflation": name_of(&inflation, &scenario["inflation_profile_id"]),
            "tax_config_id": scenario["tax_config_id"], "description": scenario["description"],
        },
        "accounts": accounts.clone(),
        "assets": assets,
        "events": events,
    });
    for account in plan["accounts"].as_array_mut().unwrap() {
        let id = account["id"].clone();
        let (_, positions) = app
            .get(&format!("/api/scenarios/{sid}/accounts/{id}/positions"))
            .await;
        account["positions"] = positions;
    }
    walk(&mut plan, &[&accounts, &assets, &profiles], &name_of);
    plan
}

/// Guided setup lowers to the same change model the AI drafting uses; its
/// plans must not drift. Fixtures hold the plans the original SQL lowering
/// wrote (`UPDATE_GOLDEN=1` rewrites them).
#[tokio::test]
async fn guided_setup_plans_match_the_recorded_lowering() {
    let mut app = TestApp::new().await;
    let base = json!({
        "request_id": "golden-0", "name": "Golden", "start_date": "2026-01-01",
        "birth_date": "1981-01-01", "duration_years": 50, "retirement_age": 65,
        "cash": 50000, "retirement_401k": 350000, "investments": 150000, "stock_percent": 60,
        "investment_tax_status": "Taxable", "annual_income": 100000,
        "retirement_401k_contribution_percent": 50, "annual_spending": 40000,
        "retirement_spending": 30000, "inflation_profile_id": null, "tax_config_id": null,
        "fund_from_investments": true, "assumptions_confirmed": true,
    });
    let variants: Vec<(&str, Vec<(&str, Value)>)> = vec![
        ("full", vec![]),
        (
            "cash_only",
            vec![
                ("retirement_401k", json!(0)),
                ("investments", json!(0)),
                ("retirement_401k_contribution_percent", json!(0)),
                ("fund_from_investments", json!(false)),
            ],
        ),
        (
            "start_401k_all_stock",
            vec![
                ("retirement_401k", json!(0)),
                ("investments", json!(0)),
                ("retirement_401k_contribution_percent", json!(6)),
                ("stock_percent", json!(100)),
                ("inflation_profile_id", json!("first")),
                ("investment_tax_status", json!("TaxFree")),
            ],
        ),
        (
            "no_salary_over_limit",
            vec![
                ("annual_income", json!(0)),
                ("retirement_401k_contribution_percent", json!(0)),
                ("investments", json!(80000)),
                ("stock_percent", json!(0)),
                ("investment_tax_status", json!("TaxDeferred")),
            ],
        ),
        (
            "capped_contribution",
            vec![
                ("annual_income", json!(400000)),
                ("retirement_401k_contribution_percent", json!(20)),
                ("stock_percent", json!(35.5)),
            ],
        ),
    ];
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases/onboarding_golden");
    for (i, (label, edits)) in variants.into_iter().enumerate() {
        // One editable plan per user on the free tier, and profiles are per user.
        app.login_as(&format!("golden-setup-{i}@example.com")).await;
        let (_, profiles) = app.get("/api/return-profiles").await;
        let (_, inflation) = app.get("/api/inflation-profiles").await;
        let inflation = inflation[0]["id"].as_i64().unwrap();
        let mut body = base.clone();
        body["cash_profile_id"] = profiles[0]["id"].clone();
        body["stock_profile_id"] = profiles[1]["id"].clone();
        body["bond_profile_id"] = profiles[2]["id"].clone();
        body["request_id"] = json!(format!("golden-{i}-{label}"));
        for (key, value) in edits {
            body[key] = value;
        }
        if body["inflation_profile_id"] == "first" {
            body["inflation_profile_id"] = json!(inflation);
        }
        let (status, created) = app.post("/api/scenarios/setup", body).await;
        assert_eq!(status, StatusCode::OK, "{label}: {created}");
        let plan = dumped_plan(&mut app, created["scenario_id"].as_i64().unwrap()).await;
        let path = dir.join(format!("{label}.json"));
        let text = serde_json::to_string_pretty(&plan).unwrap() + "\n";
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &text).unwrap();
        }
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {path:?}")),
        )
        .unwrap();
        assert_eq!(plan, expected, "{label}");
    }
}
