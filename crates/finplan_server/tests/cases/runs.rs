use super::*;

/// A plan with no market variance ends every iteration in exactly the same
/// place, so every quantile of the real net worth band is the same number. It
/// used to fail at persist (`p5 <= p50 AND p50 <= p95`) when interpolating
/// between equal values rounded an ulp either way.
#[tokio::test]
async fn a_fully_deterministic_plan_runs_and_persists() {
    let mut app = TestApp::new().await;
    app.login_as("deterministic@example.com").await;
    let (_, profiles) = app.get("/api/return-profiles").await;
    let cash = profiles[0]["id"].as_i64().unwrap();
    let (status, _) = app
        .patch(
            &format!("/api/return-profiles/{cash}"),
            json!({"distribution": {"kind": "Fixed", "rate": 0.0271}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, inflation) = app.get("/api/inflation-profiles").await;
    let (status, created) = app
        .post(
            "/api/scenarios/setup",
            json!({
                "request_id": "deterministic-setup", "name": "Deterministic",
                "start_date": "2026-01-01", "birth_date": "1981-01-01", "duration_years": 30,
                "retirement_age": 65, "cash": 123456.789, "retirement_401k": 0, "investments": 0,
                "stock_percent": 60, "cash_profile_id": cash, "stock_profile_id": cash,
                "bond_profile_id": cash, "investment_tax_status": "Taxable",
                "annual_income": 0, "retirement_401k_contribution_percent": 0,
                "annual_spending": 1234.56, "retirement_spending": 0,
                "inflation_profile_id": inflation[0]["id"], "tax_config_id": null,
                "fund_from_investments": false, "assumptions_confirmed": true,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let sid = created["scenario_id"].as_i64().unwrap();
    let (status, run) = app
        .post(
            &format!("/api/scenarios/{sid}/runs"),
            json!({"iterations": 25, "seed": 7}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let run_id = run["id"].as_i64().unwrap();
    let status = app.await_run(run_id).await;
    let (_, detail) = app.get(&format!("/api/runs/{run_id}")).await;
    assert_eq!(status, "succeeded", "{detail}");
}
