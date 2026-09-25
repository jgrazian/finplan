#[tokio::test]
async fn analysis_round_trips_rate_and_date_bounds_in_their_display_units() {
    let mut app = TestApp::new().await;
    app.login_as("named-analysis@example.com").await;
    let (scenario_id, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario_id}");
    let (status, rate) = app.post(&format!("{base}/parameters"), json!({
        "name":"Rate","value":{"kind":"Rate","value":0.2}
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, date) = app.post(&format!("{base}/parameters"), json!({
        "name":"PaymentDate","value":{"kind":"Date","value":"2027-01-02"}
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = app.post(&format!("{base}/events"), json!({
        "name":"Payment","trigger":{"kind":"DateParameter","parameter_id":date["id"]},
        "effects":[{"kind":"Expense","from_account_id":checking,
            "amount":{"kind":"Expression","source":"1000 * $Rate"}}]
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    let rate_id = format!("parameter:{}", rate["id"]);
    let date_id = format!("parameter:{}", date["id"]);
    let start = (jiff::civil::Date::constant(2027,1,1) - jiff::civil::Date::constant(1970,1,1)).get_days();
    let (status, job) = app.post(&format!("{base}/analyses"), json!({
        "kind":"sweep","iterations":25,"axes":[
            {"parameter_id":rate_id,"min":0.1,"max":0.3,"steps":3},
            {"parameter_id":date_id,"min":start,"max":start+2,"steps":12}
        ]
    })).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let job_id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(job_id).await, "succeeded");
    let (_, result) = app.get(&format!("/api/analyses/{job_id}/results")).await;
    assert_eq!(result["axes"][0]["kind"], "rate");
    assert_eq!(result["axes"][0]["values"], json!([0.1,0.2,0.3]));
    assert_eq!(result["axes"][1]["values"], json!([f64::from(start),f64::from(start+1),f64::from(start+2)]));
    assert_eq!(result["cells"].as_array().unwrap().len(), 9);
    assert_eq!(result["plan_indices"], json!([1,1]));

    let (status, job) = app.post(&format!("{base}/analyses"), json!({
        "kind":"solve","iterations":25,"objective":"max-parameter","min_value":0.0,
        "vary":[{"parameter_id":date_id,"min":start,"max":start+2,"steps":3}]
    })).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let job_id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(job_id).await, "succeeded");
    let (_, result) = app.get(&format!("/api/analyses/{job_id}/results")).await;
    assert_eq!(result["method"], "grid-search");
    assert_eq!(result["best"]["values"], json!([f64::from(start+2)]));
}
