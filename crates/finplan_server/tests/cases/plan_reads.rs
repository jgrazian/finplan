//! `finplan_plan::read` answers the plan-shaped GET routes from a loaded
//! graph. The browser's local store has only a graph, so each reader must
//! return exactly the body the server's route returns for the same plan.
//!
//! One scenario carries every account flavor (bank, taxable and retirement
//! investment, property, loans with and without a repayment schedule), several
//! lots in a reordered order, an unmapped asset, parameters that events use, a
//! reordered return profile library, a nested regime-switching profile and an
//! added tax config and inflation profile. Each body is compared as a
//! `serde_json::Value`, after both sides went through the same JSON text.

use finplan_plan::read::{self, ScenarioExtras};
use finplan_plan::specs::EffectSpec;

use super::*;

/// A value as the test sees a response body: serialised, then parsed by the
/// same `serde_json` that parsed the body, so floats compare as served.
fn as_served<T: serde::Serialize>(value: &T) -> Value {
    serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap()
}

async fn created(app: &TestApp, path: &str, body: Value) -> Value {
    let (status, created) = app.post(path, body).await;
    assert_eq!(status, StatusCode::CREATED, "POST {path}: {created}");
    created
}

#[tokio::test]
async fn every_plan_shaped_read_equals_its_route() {
    let (mut app, state) = results_projection_cases::app_with_state().await;
    app.login_as("plan-reads@example.com").await;
    let (scenario, checking, brokerage) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");

    // ── the library: a custom return profile (nested regimes), an inflation
    // profile and a tax config, then a reordered return profile library.
    let (_, profiles) = app.get("/api/return-profiles").await;
    let profiles = profiles.as_array().unwrap();
    let id_of = |name: &str| {
        profiles.iter().find(|p| p["name"] == name).unwrap()["id"]
            .as_i64()
            .unwrap()
    };
    let equity = id_of("US Total Market");
    let regimes = created(
        &app,
        "/api/return-profiles",
        json!({
            "name": "Two regimes", "description": "bull and bear", "asset_class": "Balanced",
            "distribution": {
                "kind": "RegimeSwitching",
                "bull": {"kind": "Normal", "mean": 0.12, "std_dev": 0.1},
                "bear": {"kind": "Bootstrap", "preset": "sp500", "block_size": 3},
                "bull_to_bear_prob": 0.1, "bear_to_bull_prob": 0.4
            }
        }),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let inflation = created(
        &app,
        "/api/inflation-profiles",
        json!({"name": "Steady", "description": "three percent",
               "distribution": {"kind": "Fixed", "rate": 0.03}}),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let tax = created(
        &app,
        "/api/tax-configs",
        json!({"name": "Flat twenty", "description": "one bracket", "state_rate": 0.04,
               "standard_deduction": 14600.0, "age_65_extra_deduction": 1950.0,
               "federal_brackets": [{"threshold": 0.0, "rate": 0.2},
                                     {"threshold": 50000.0, "rate": 0.3}]}),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let mut order: Vec<i64> = profiles.iter().map(|p| p["id"].as_i64().unwrap()).collect();
    order.push(regimes);
    order.reverse();
    let (status, _) = app
        .post("/api/return-profiles/reorder", json!({"ids": order}))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // ── the scenario's settings
    let (status, patched) = app
        .patch(
            &base,
            json!({"description": "A plan with everything", "collect_ledger": true,
                   "inflation_profile_id": inflation, "tax_config_id": tax}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{patched}");

    // ── assets, one of them unmapped
    let (_, assets) = app.get(&format!("{base}/assets")).await;
    let vti = assets[0]["id"].as_i64().unwrap();
    let home_asset = created(
        &app,
        &format!("{base}/assets"),
        json!({"name": "Home", "description": "The house", "initial_price": 100.0}),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    created(
        &app,
        &format!("{base}/assets"),
        json!({"name": "BND", "initial_price": 80.0, "return_profile_id": regimes,
               "tracking_error": 0.01}),
    )
    .await;

    // ── every account flavor
    let roth = created(
        &app,
        &format!("{base}/accounts"),
        json!({"name": "Roth IRA", "flavor": "Investment", "tax_status": "TaxFree",
               "cash_value": 500.0, "cash_return_profile_id": equity,
               "plan_type": "RothIra", "contribution_limit": 7000.0,
               "contribution_period": "Yearly",
               "catch_up": [{"from_age": 50, "through_age": null, "amount": 1000.0}]}),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    created(
        &app,
        &format!("{base}/accounts"),
        json!({"name": "House", "flavor": "Property", "description": "Primary home",
               "asset_id": home_asset, "value": 250_000.0}),
    )
    .await;
    created(
        &app,
        &format!("{base}/accounts"),
        json!({"name": "Mortgage", "flavor": "Liability",
               "principal": 150_000.0, "interest_rate": 0.065}),
    )
    .await;
    created(
        &app,
        &format!("{base}/accounts"),
        json!({"name": "Car loan", "flavor": "Liability",
               "principal": 20_000.0, "interest_rate": 0.05,
               "repayment": {"from_account_id": checking, "term_months": 60}}),
    )
    .await;

    // ── several lots, put out of purchase-date order by an explicit reorder
    let lots = format!("{base}/accounts/{brokerage}/positions");
    let mut lot_ids = Vec::new();
    for (date, units, basis) in [
        ("2020-03-01", 10.0, 900.0),
        ("2019-01-15", 20.0, 1500.0),
        ("2024-07-04", 5.0, 600.0),
    ] {
        lot_ids.push(
            created(
                &app,
                &lots,
                json!({"asset_id": vti, "purchase_date": date, "units": units,
                       "cost_basis": basis}),
            )
            .await["id"]
                .as_i64()
                .unwrap(),
        );
    }
    let (_, listed) = app.get(&lots).await;
    let mut lot_order: Vec<i64> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();
    lot_order.reverse();
    let (status, _) = app
        .post(&format!("{lots}/reorder"), json!({"ids": lot_order}))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // ── parameters and the events that use them
    let spending = created(
        &app,
        &format!("{base}/parameters"),
        json!({"name": "Spending", "value": {"kind": "Money", "value": 250.0}}),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let retire = created(
        &app,
        &format!("{base}/parameters"),
        json!({"name": "Retirement age", "value": {"kind": "Age", "years": 67, "months": 6}}),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    created(
        &app,
        &format!("{base}/parameters"),
        json!({"name": "Start", "value": {"kind": "Date", "value": "2030-01-01"}}),
    )
    .await;
    created(
        &app,
        &format!("{base}/events"),
        json!({"name": "Spending event", "enabled": true,
               "trigger": {"kind": "AgeParameter", "parameter_id": retire},
               "effects": [{"kind": "Expense", "from_account_id": checking,
                            "amount": {"kind": "Expression", "source": "min($Spending, 400)"}}]}),
    )
    .await;
    created(
        &app,
        &format!("{base}/events"),
        json!({"name": "Salary", "description": "Paid monthly",
               "trigger": {"kind": "And", "children": [
                   {"kind": "Date", "on_date": "2026-06-01"},
                   {"kind": "AccountBalance", "account_id": checking,
                    "comparison": "LessThanOrEqual", "threshold": 100_000.0}]},
               "effects": [{"kind": "Income", "to_account_id": checking,
                            "amount": {"kind": "Fixed", "value": 5_000.0},
                            "income_type": "Taxable"},
                           {"kind": "CashTransfer", "from_account_id": checking,
                            "to_account_id": roth,
                            "amount": {"kind": "Fixed", "value": 500.0}}]}),
    )
    .await;
    created(
        &app,
        &format!("{base}/events"),
        json!({"name": "Idle", "enabled": false, "trigger": {"kind": "Manual"}, "effects": []}),
    )
    .await;
    let _ = spending;

    // ── load the graph the way a route does
    let user_id: String = sqlx::query_scalar("SELECT user_id FROM scenarios WHERE id = ?1")
        .bind(scenario)
        .fetch_one(&state.db)
        .await
        .unwrap();
    let graph = finplan_server::db::graph::load(&state.db, scenario, &user_id)
        .await
        .unwrap();

    // ── the scenario
    let (_, served) = app.get(&base).await;
    let extras = ScenarioExtras::active(served["slug"].as_str().unwrap());
    assert_eq!(served, as_served(&read::scenario(&graph, extras)));

    // ── accounts, with their positions
    let (_, served) = app.get(&format!("{base}/accounts")).await;
    assert_eq!(served, as_served(&read::accounts(&graph).unwrap()));
    assert_eq!(served.as_array().unwrap().len(), 6);
    for account in served.as_array().unwrap() {
        let id = account["id"].as_i64().unwrap();
        let (_, one) = app.get(&format!("{base}/accounts/{id}")).await;
        assert_eq!(one, as_served(&read::account(&graph, id).unwrap()), "{id}");
        let (_, lots) = app.get(&format!("{base}/accounts/{id}/positions")).await;
        assert_eq!(
            lots,
            as_served(&read::positions(&graph, id)),
            "lots of {id}"
        );
    }
    let (_, lots) = app
        .get(&format!("{base}/accounts/{brokerage}/positions"))
        .await;
    assert_eq!(lots.as_array().unwrap().len(), 4, "{lots}");
    let (status, _) = app.get(&format!("{base}/accounts/999999")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        read::account(&graph, 999_999).unwrap_err().to_string(),
        "account not found"
    );

    // ── assets and events and parameters
    let (_, served) = app.get(&format!("{base}/assets")).await;
    assert_eq!(served, as_served(&read::assets(&graph)));
    for asset in served.as_array().unwrap() {
        let id = asset["id"].as_i64().unwrap();
        let (_, one) = app.get(&format!("{base}/assets/{id}")).await;
        assert_eq!(one, as_served(&read::asset(&graph, id).unwrap()));
    }
    let (_, served) = app.get(&format!("{base}/events")).await;
    assert_eq!(served, as_served(&read::events(&graph).unwrap()));
    assert_eq!(served.as_array().unwrap().len(), 3);
    for event in served.as_array().unwrap() {
        let id = event["id"].as_i64().unwrap();
        let (_, one) = app.get(&format!("{base}/events/{id}")).await;
        assert_eq!(one, as_served(&read::event(&graph, id).unwrap()));
    }
    let (_, served) = app.get(&format!("{base}/parameters")).await;
    assert_eq!(served, as_served(&read::parameters(&graph).unwrap()));
    assert!(
        !served[0]["uses"].as_array().unwrap().is_empty(),
        "a parameter in use: {served}"
    );

    // ── the library
    let (_, served) = app.get("/api/return-profiles").await;
    assert_eq!(served, as_served(&read::return_profiles(&graph).unwrap()));
    assert_eq!(served[0]["name"], "Two regimes", "reordered to the front");
    for profile in served.as_array().unwrap() {
        let id = profile["id"].as_i64().unwrap();
        let (_, one) = app.get(&format!("/api/return-profiles/{id}")).await;
        assert_eq!(one, as_served(&read::return_profile(&graph, id).unwrap()));
    }
    let (_, served) = app.get("/api/inflation-profiles").await;
    assert_eq!(
        served,
        as_served(&read::inflation_profiles(&graph).unwrap())
    );
    let (_, served) = app.get("/api/tax-configs").await;
    assert_eq!(served, as_served(&read::tax_configs(&graph)));
    assert!(served.as_array().unwrap().len() > 1);
    for config in served.as_array().unwrap() {
        let id = config["id"].as_i64().unwrap();
        let (_, one) = app.get(&format!("/api/tax-configs/{id}")).await;
        assert_eq!(one, as_served(&read::tax_config(&graph, id).unwrap()));
    }
    let (_, served) = app.get("/api/history-presets").await;
    assert_eq!(served, as_served(&read::history_presets()));

    // ── compile, preflight, expression validation
    let (_, served) = app.post(&format!("{base}/compile"), json!({})).await;
    assert_eq!(served, as_served(&read::compile_report(&graph).unwrap()));
    let (_, served) = app.get(&format!("{base}/preflight")).await;
    assert_eq!(served, as_served(&read::preflight(&graph)));
    for source in ["min($Spending, 400)", "0.2 *"] {
        let effect = json!({"kind": "Expense", "from_account_id": checking,
                            "amount": {"kind": "Expression", "source": source}});
        let (_, served) = app
            .post(
                &format!("{base}/expressions/validate"),
                json!({"effect": effect}),
            )
            .await;
        let effect: EffectSpec = serde_json::from_value(effect).unwrap();
        assert_eq!(
            served,
            as_served(&read::validate_expression(&graph, &effect).unwrap()),
            "{source}"
        );
    }

    // ── archives
    let (_, served) = app.get(&format!("{base}/archive")).await;
    let archive = finplan_plan::archive::pack(vec![graph]).unwrap();
    assert_eq!(served, as_served(&archive));
    let (_, preview) = app.post("/api/archives/preview", served).await;
    assert_eq!(
        preview,
        as_served(&finplan_plan::archive::preview(&archive).unwrap())
    );
    assert_eq!(preview["accounts"], 6);
}
