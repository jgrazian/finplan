use super::*;

/// A financed purchase, a sale that pays the loan off, and an amortizing loan
/// the plan opens with: each round-trips through the API, compiles, survives
/// duplication with its account references rewritten, and leaves no orphaned
/// down-payment amount when its event is replaced.
#[tokio::test]
async fn property_purchase_sale_and_amortizing_loans_round_trip() {
    let mut app = TestApp::new().await;
    app.login_as("real-estate@example.com").await;
    let (scenario, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");

    let (status, house_asset) = app
        .post(
            &format!("{base}/assets"),
            json!({"name": "Home", "initial_price": 100.0}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{house_asset}");
    let (status, house) = app
        .post(
            &format!("{base}/accounts"),
            json!({"name": "House", "flavor": "Property",
                   "asset_id": house_asset["id"], "value": 0.0}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{house}");
    let house = house["id"].as_i64().unwrap();
    let (status, mortgage) = app
        .post(
            &format!("{base}/accounts"),
            json!({"name": "Mortgage", "flavor": "Liability",
                   "principal": 0.0, "interest_rate": 0.06}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{mortgage}");
    let mortgage = mortgage["id"].as_i64().unwrap();

    // A loan the plan opens with, already amortizing.
    let (status, car) = app
        .post(
            &format!("{base}/accounts"),
            json!({"name": "Car loan", "flavor": "Liability",
                   "principal": 20_000.0, "interest_rate": 0.05,
                   "repayment": {"from_account_id": checking, "term_months": 60}}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{car}");
    let car = car["id"].as_i64().unwrap();
    let (_, car_read) = app.get(&format!("{base}/accounts/{car}")).await;
    assert_eq!(
        car_read["repayment"],
        json!({"from_account_id": checking, "term_months": 60}),
        "{car_read}"
    );

    // A loan cannot be repaid from a property.
    let (status, _) = app
        .send(
            "PATCH",
            &format!("{base}/accounts/{car}"),
            Some(
                json!({"flavor": "Liability", "principal": 20_000.0, "interest_rate": 0.05,
                        "repayment": {"from_account_id": house, "term_months": 60}}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let buy = json!({"kind": "BuyProperty", "property_account_id": house,
        "from_account_id": checking, "price": {"kind": "Fixed", "value": 500_000.0},
        "financing": {"loan_account_id": mortgage,
                      "down_payment": {"kind": "Fixed", "value": 100_000.0},
                      "term_months": 360}});
    let sell = json!({"kind": "SellProperty", "property_account_id": house,
        "to_account_id": checking, "selling_cost_rate": 0.06, "gain_exclusion": 250_000.0,
        "payoff_account_id": mortgage});
    let (status, event) = app
        .post(
            &format!("{base}/events"),
            json!({"name": "Home", "trigger": {"kind": "Manual"},
                   "effects": [buy.clone(), sell.clone()]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{event}");
    let event_id = event["id"].as_i64().unwrap();
    assert_eq!(event["effects"][0], buy);
    assert_eq!(event["effects"][1], sell);

    let (status, report) = app.post(&format!("{base}/compile"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    // The down payment can be an expression, checked like any other amount.
    let (status, event) = app
        .post(
            &format!("{base}/events"),
            json!({"name": "Expression down", "trigger": {"kind": "Manual"},
                   "effects": [{"kind": "BuyProperty", "property_account_id": house,
                                "from_account_id": checking,
                                "price": {"kind": "Fixed", "value": 500_000.0},
                                "financing": {"loan_account_id": mortgage, "term_months": 360,
                                    "down_payment": {"kind": "Expression",
                                                     "source": "min(0.2 * 500000, cash(\"Checking\"))"}}}]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{event}");
    let (status, report) = app.post(&format!("{base}/compile"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let (status, bad) = app
        .post(
            &format!("{base}/events"),
            json!({"name": "Bad down", "trigger": {"kind": "Manual"},
                   "effects": [{"kind": "BuyProperty", "property_account_id": house,
                                "from_account_id": checking,
                                "price": {"kind": "Fixed", "value": 500_000.0},
                                "financing": {"loan_account_id": mortgage, "term_months": 360,
                                    "down_payment": {"kind": "Expression", "source": "0.2 *"}}}]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");

    // Selling costs are a share of the price.
    let (status, _) = app
        .post(
            &format!("{base}/events"),
            json!({"name": "Bad sale", "trigger": {"kind": "Manual"},
                   "effects": [{"kind": "SellProperty", "property_account_id": house,
                                "to_account_id": checking, "selling_cost_rate": 6.0,
                                "gain_exclusion": 0.0}]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Duplicating rewrites the loan and payer references to the copy's own.
    let (status, clone) = app
        .post(&format!("{base}/duplicate"), json!({"name": "Copy"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{clone}");
    let clone_id = clone["id"].as_i64().unwrap();
    let (_, clone_accounts) = app
        .get(&format!("/api/scenarios/{clone_id}/accounts"))
        .await;
    let clone_id_of = |name: &str| {
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
    assert_eq!(
        clone_events[0]["effects"][0]["financing"]["loan_account_id"],
        clone_id_of("Mortgage")
    );
    assert_eq!(
        clone_events[0]["effects"][1]["payoff_account_id"],
        clone_id_of("Mortgage")
    );
    let clone_car = clone_accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "Car loan")
        .unwrap();
    assert_eq!(
        clone_car["repayment"]["from_account_id"],
        clone_id_of("Checking")
    );

    // Replacing the event drops the down payment's amount row with it.
    let (status, _) = app
        .send(
            "PUT",
            &format!("{base}/events/{event_id}"),
            Some(json!({"name": "Home", "trigger": {"kind": "Manual"}, "effects": []})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, report) = app.post(&format!("{base}/compile"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{report}");
}
