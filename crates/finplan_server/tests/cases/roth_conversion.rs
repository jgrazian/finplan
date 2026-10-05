use super::*;

/// A yearly Roth conversion round-trips through the API, is refused between
/// the wrong kinds of account, compiles, and survives duplication with its
/// account references (the tax payer's included) rewritten to the copy's.
#[tokio::test]
async fn roth_conversions_round_trip_and_are_checked() {
    let mut app = TestApp::new().await;
    app.login_as("roth-conversion@example.com").await;
    let (scenario, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");

    let (_, profiles) = app.get("/api/return-profiles").await;
    let cash = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Savings Account")
        .unwrap()["id"]
        .clone();
    let mut ids = Vec::new();
    for (name, tax_status) in [("401(k)", "TaxDeferred"), ("Roth IRA", "TaxFree")] {
        let (status, account) = app
            .post(
                &format!("{base}/accounts"),
                json!({"name": name, "flavor": "Investment", "tax_status": tax_status,
                       "cash_value": 20_000.0, "cash_return_profile_id": cash}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{account}");
        ids.push(account["id"].as_i64().unwrap());
    }
    let (k401, roth) = (ids[0], ids[1]);

    let conversion = |name: &str, from: i64, to: i64, pay: Value| {
        json!({"name": name,
               "trigger": {"kind": "Repeating", "interval": "Yearly",
                           "start_condition": {"kind": "Date", "on_date": "2026-12-30"}},
               "effects": [{"kind": "RothConversion", "from_account_id": from,
                            "to_account_id": to, "pay_tax_from_account_id": pay,
                            "amount": {"kind": "Expression", "source": "bracket_room(0.12)"}}]})
    };
    let (status, event) = app
        .post(
            &format!("{base}/events"),
            conversion("Roth conversions", k401, roth, json!(checking)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{event}");
    assert_eq!(event["effects"][0]["kind"], "RothConversion");
    assert_eq!(event["effects"][0]["pay_tax_from_account_id"], checking);
    assert_eq!(
        event["effects"][0]["amount"]["source"],
        "bracket_room(0.12)"
    );

    let (status, report) = app.post(&format!("{base}/compile"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    // Into an account that is not a Roth, or paid by one that is not a bank
    // or taxable account: refused.
    for bad in [
        conversion("Into checking", k401, checking, Value::Null),
        conversion("Paid by the Roth", k401, roth, json!(roth)),
    ] {
        let (status, body) = app.post(&format!("{base}/events"), bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }

    // Duplicating rewrites from, to and the payer to the copy's accounts.
    let (status, clone) = app
        .post(&format!("{base}/duplicate"), json!({"name": "Copy"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{clone}");
    let clone_id = clone["id"].as_i64().unwrap();
    let (_, clone_accounts) = app
        .get(&format!("/api/scenarios/{clone_id}/accounts"))
        .await;
    let id_of = |name: &str| {
        clone_accounts
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    };
    let (_, clone_events) = app.get(&format!("/api/scenarios/{clone_id}/events")).await;
    let effect = &clone_events
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Roth conversions")
        .unwrap()["effects"][0];
    assert_eq!(effect["from_account_id"], id_of("401(k)"));
    assert_eq!(effect["to_account_id"], id_of("Roth IRA"));
    assert_eq!(effect["pay_tax_from_account_id"], id_of("Checking"));
    assert_ne!(id_of("Checking"), checking);
}
