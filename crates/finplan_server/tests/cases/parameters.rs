use super::*;

#[tokio::test]
async fn named_parameters_compile_rename_clone_and_guard_deletion() {
    let mut app = TestApp::new().await;
    app.login_as("named-parameters@example.com").await;
    let (scenario, checking, brokerage) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");

    let (status, money) = app
        .post(
            &format!("{base}/parameters"),
            json!({
                "name":"Spending","value":{"kind":"Money","value":250.0}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{money}");
    let money_id = money["id"].as_i64().unwrap();
    let (status, age) = app
        .post(
            &format!("{base}/parameters"),
            json!({
                "name":"Retirement age","value":{"kind":"Age","years":67,"months":6}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{age}");
    let age_id = age["id"].as_i64().unwrap();

    let (_, assets) = app.get(&format!("{base}/assets")).await;
    let asset = assets
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "VTI")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let (status,nested)=app.post(&format!("{base}/events"),json!({
        "name":"Invalid nested amount",
        "trigger":{"kind":"Manual"},
        "effects":[{"kind":"Expense","from_account_id":checking,
            "amount":{"kind":"InflationAdjusted","inner":{"kind":"Expression","source":"$Spending"}}}]
    })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{nested}");
    let (status,event)=app.post(&format!("{base}/events"),json!({
        "name":"Spending event","enabled":true,
        "trigger":{"kind":"AgeParameter","parameter_id":age_id},
        "effects":[{"kind":"Expense","from_account_id":checking,
            "amount":{"kind":"Expression","source":"min($Spending, holding(\"Brokerage\", \"VTI\"))"}}]
    })).await;
    assert_eq!(status, StatusCode::CREATED, "{event}");
    let event_id = event["id"].as_i64().unwrap();
    let (status, report) = app.post(&format!("{base}/compile"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let (_, params) = app.get(&format!("{base}/parameters")).await;
    assert_eq!(params.as_array().unwrap().len(), 2);
    assert_eq!(
        params
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == money_id)
            .unwrap()["uses"][0]["event_id"],
        event_id
    );
    assert_eq!(
        params
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == age_id)
            .unwrap()["uses"][0]["location"],
        "schedule"
    );
    let (status, _) = app
        .patch(
            &format!("{base}/parameters/{money_id}"),
            json!({
                "name":"Spending","value":{"kind":"Rate","value":0.04}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, renamed) = app
        .patch(
            &format!("{base}/parameters/{money_id}"),
            json!({
                "name":"Monthly spending","value":{"kind":"Money","value":300.0}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    let (status, _) = app
        .patch(
            &format!("{base}/accounts/{brokerage}"),
            json!({"name":"Retirement account"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = app
        .patch(
            &format!("{base}/assets/{asset}"),
            json!({"name":"Total market"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, event) = app.get(&format!("{base}/events/{event_id}")).await;
    let source = event["effects"][0]["amount"]["source"].as_str().unwrap();
    assert!(source.contains("$\"Monthly spending\""), "{source}");
    assert!(source.contains("Retirement account"), "{source}");
    assert!(source.contains("Total market"), "{source}");
    let (status, _) = app.delete(&format!("{base}/parameters/{money_id}")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = app.delete(&format!("{base}/accounts/{brokerage}")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = app.delete(&format!("{base}/assets/{asset}")).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, copy) = app
        .post(&format!("{base}/duplicate"), json!({"name":"Copy"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{copy}");
    let copy_id = copy["id"].as_i64().unwrap();
    let (status, report) = app
        .post(&format!("/api/scenarios/{copy_id}/compile"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let (_, copied_params) = app
        .get(&format!("/api/scenarios/{copy_id}/parameters"))
        .await;
    assert_eq!(copied_params.as_array().unwrap().len(), 2);
    assert_ne!(copied_params[0]["id"], params[0]["id"]);
    let (status, archive) = app.get(&format!("{base}/archive")).await;
    assert_eq!(status, StatusCode::OK, "{archive}");
    let (status, imported) = app
        .post(
            "/api/archives/import",
            json!({
                "archive":archive,"name_prefix":"Imported ","request_id":"parameter-import-1"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let import_id = imported["scenario_ids"][0].as_i64().unwrap();
    let (status, report) = app
        .post(&format!("/api/scenarios/{import_id}/compile"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let (_, imported_params) = app
        .get(&format!("/api/scenarios/{import_id}/parameters"))
        .await;
    assert_eq!(imported_params.as_array().unwrap().len(), 2);
    for id in [scenario, copy_id, import_id] {
        let (status, response) = app.delete(&format!("/api/scenarios/{id}")).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "scenario {id}: {response}");
    }
}

#[tokio::test]
async fn calendar_trigger_rejects_another_scenarios_parameter() {
    let mut app = TestApp::new().await;
    app.login_as("parameter-scope@example.com").await;
    let (first, _, _) = app.seed_scenario().await;
    let (status, date) = app
        .post(
            &format!("/api/scenarios/{first}/parameters"),
            json!({
                "name":"Start","value":{"kind":"Date","value":"2035-01-01"}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let parameter_id = date["id"].as_i64().unwrap();
    let (status, second) = app
        .post(
            "/api/scenarios",
            json!({
                "name":"Second plan","start_date":"2026-01-01","birth_date":"1980-01-01"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let second = second["id"].as_i64().unwrap();
    let (status, error) = app
        .post(
            &format!("/api/scenarios/{second}/events"),
            json!({
                "name":"Invalid reference",
                "trigger":{"kind":"DateParameter","parameter_id":parameter_id},
                "effects":[]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let (status, params) = app
        .get(&format!("/api/scenarios/{second}/parameters"))
        .await;
    assert_eq!(status, StatusCode::OK, "{params}");
    assert!(params.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn quoted_parameter_name_renames_by_compiled_identity() {
    let mut app = TestApp::new().await;
    app.login_as("quoted-parameter@example.com").await;
    let (scenario, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");
    let (status, param) = app
        .post(
            &format!("{base}/parameters"),
            json!({
                "name":"Épargne \"Q\"","value":{"kind":"Money","value":100.0}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{param}");
    let id = param["id"].as_i64().unwrap();
    let (status, event) = app
        .post(
            &format!("{base}/events"),
            json!({
                "name":"Quoted amount",
                "trigger":{"kind":"Manual"},
                "effects":[{"kind":"Expense","from_account_id":checking,
                    "amount":{"kind":"Expression","source":"$\"Épargne \\\"Q\\\"\""}}]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{event}");
    let event_id = event["id"].as_i64().unwrap();
    let (status, updated) = app
        .patch(
            &format!("{base}/parameters/{id}"),
            json!({
                "name":"Savings","value":{"kind":"Money","value":100.0}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    let (_, event) = app.get(&format!("{base}/events/{event_id}")).await;
    assert_eq!(event["effects"][0]["amount"]["source"], "$Savings");
    let (status, report) = app.post(&format!("{base}/compile"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
}

#[tokio::test]
async fn expression_validation_distinguishes_constant_errors_from_stateful_previews() {
    let mut app = TestApp::new().await;
    app.login_as("expression-validation@example.com").await;
    let (scenario, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");
    let (_, spend) = app
        .post(
            &format!("{base}/parameters"),
            json!({
                "name":"Spend","value":{"kind":"Money","value":100.0}
            }),
        )
        .await;
    let parameter_id = spend["id"].as_i64().unwrap();
    let validate = |source: &str| {
        json!({"effect":{
            "kind":"Expense","from_account_id":checking,
            "amount":{"kind":"Expression","source":source}
        }})
    };

    let (status, report) = app
        .post(
            &format!("{base}/expressions/validate"),
            validate("$Spend + 10"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["valid"], true);
    assert_eq!(report["parameter_ids"], json!([parameter_id]));
    assert_eq!(report["preview_value"], 110.0);

    for source in [
        "100 / 0",
        "balance(source) / 0",
        "clamp(100, 200, 0)",
        "clamp(balance(source), 200, 0)",
        "100 / 0 + balance(source)",
    ] {
        let (status, report) = app
            .post(&format!("{base}/expressions/validate"), validate(source))
            .await;
        assert_eq!(status, StatusCode::OK, "{report}");
        assert_eq!(report["valid"], false, "{source}: {report}");
        assert!(!report["diagnostics"].as_array().unwrap().is_empty());
    }
    let (_, report) = app
        .post(
            &format!("{base}/expressions/validate"),
            validate("if(false, balance(target), 100)"),
        )
        .await;
    assert_eq!(report["valid"], false, "{report}");
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("target")
    );
    let (_, report) = app
        .post(
            &format!("{base}/expressions/validate"),
            validate("$Spend + 10%"),
        )
        .await;
    assert_eq!(report["valid"], false, "{report}");

    let (_, report) = app
        .post(
            &format!("{base}/expressions/validate"),
            validate("$Spend * (100 / cash(\"Brokerage\"))"),
        )
        .await;
    assert_eq!(report["valid"], true, "{report}");
    assert!(report["preview_value"].is_null());
    assert!(report["preview_label"].as_str().unwrap().contains("state"));

    let (status,_)=app.post(&format!("{base}/events"),json!({
        "name":"Broken saved event","trigger":{"kind":"Manual"},
        "effects":[{"kind":"Expense","from_account_id":checking,"amount":{"kind":"Expression","source":"100"}}]
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    let db = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        app._dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE scenarios SET birth_date=NULL WHERE id=?1")
        .bind(scenario)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE transfer_amounts SET expression_source='if(age() >= 65, 100, 50)' WHERE id IN
        (SELECT amount_id FROM effects WHERE scenario_id=?1)",
    )
    .bind(scenario)
    .execute(&db)
    .await
    .unwrap();
    let (status, report) = app
        .post(
            &format!("{base}/expressions/validate"),
            validate("$Spend + 10"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["valid"], true, "{report}");
    assert_eq!(report["preview_value"], 110.0);

    sqlx::query(
        "UPDATE transfer_amounts SET expression_source='unknown()' WHERE id IN
        (SELECT amount_id FROM effects WHERE scenario_id=?1)",
    )
    .bind(scenario)
    .execute(&db)
    .await
    .unwrap();
    db.close().await;
    let (_, report) = app
        .post(
            &format!("{base}/expressions/validate"),
            validate("$Spend + 10"),
        )
        .await;
    assert_eq!(report["valid"], true, "{report}");
    assert_eq!(report["preview_value"], 110.0);
    let (_, report) = app
        .post(&format!("{base}/expressions/validate"), validate("100 / 0"))
        .await;
    assert_eq!(report["valid"], false, "{report}");
}
