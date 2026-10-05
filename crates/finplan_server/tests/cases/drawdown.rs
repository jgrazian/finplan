use super::results_projection_cases::app_with_state;
use super::*;

/// The anonymised default plan, imported and run on a fixed seed.
async fn a_finished_run(app: &mut TestApp) -> i64 {
    let graph: serde_json::Value = serde_json::from_str(include_str!(
        "../../../finplan_plan/testdata/default_snapshot.json"
    ))
    .unwrap();
    let (status, imported) = app
        .post(
            "/api/archives/import",
            json!({
                "archive": {"format": "finplan.inputs", "version": 3, "plans": [graph]},
                "name_prefix": "Drawdown ",
                "request_id": "drawdown-default",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let scenario_id = imported["scenario_ids"][0].as_i64().unwrap();
    let (status, run) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/runs"),
            json!({"iterations": 40, "seed": 3, "percentiles": [0.1, 0.5, 0.9]}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let run_id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");
    run_id
}

fn sum(values: &serde_json::Value) -> f64 {
    values
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .sum()
}

#[tokio::test]
async fn a_run_projects_its_median_path_under_each_strategy() {
    let (mut app, state) = app_with_state().await;
    app.login_as("drawdown@example.com").await;
    let run_id = a_finished_run(&mut app).await;

    let (status, body) = app
        .post(&format!("/api/runs/{run_id}/drawdown"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let seed = finplan_server::db::median_seed(&state.db, run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(body["seed"], seed.to_string());
    assert_eq!(body["retirement"]["source"], "income");
    let choices = body["choices"].as_array().unwrap();
    assert_eq!(choices.len(), 7);
    assert_eq!(choices[0]["choice"]["kind"], "AsPlanned");
    assert_eq!(choices[0]["overlay"], false);
    assert_eq!(choices[1]["overlay"], true);
    assert_eq!(body["accounts"].as_array().unwrap().len(), 6);
    for choice in choices {
        let years = choice["years"].as_array().unwrap();
        assert!(!years.is_empty());
        for y in years {
            let inflow = sum(&y["income"])
                + sum(&y["withdrawals"])
                + y["cash"].as_f64().unwrap()
                + y["shortfall"].as_f64().unwrap();
            let outflow = y["spending"].as_f64().unwrap()
                + y["withdrawal_taxes"].as_f64().unwrap()
                + y["conversion_tax"].as_f64().unwrap()
                + y["surplus"].as_f64().unwrap();
            assert!((inflow - outflow).abs() <= 1.0, "{y}");
        }
    }

    // Asked for one choice and a retirement year.
    let (status, one) = app
        .post(
            &format!("/api/runs/{run_id}/drawdown"),
            json!({"strategies": [{"kind": "Strategy", "strategy": "BracketFilling",
                                    "bracket_ceiling": 0.22}],
                   "retirement_year": 2060}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{one}");
    assert_eq!(one["retirement"]["source"], "request");
    assert_eq!(one["choices"][0]["years"][0]["year"], 2060);

    // The conversion toggle: the plan has none, so a rate adds the overlay.
    assert_eq!(body["conversions"]["events"], json!([]));
    assert!(body["conversions"]["overlay"].is_object());
    let (status, converting) = app
        .post(
            &format!("/api/runs/{run_id}/drawdown"),
            json!({"strategies": [{"kind": "AsPlanned",
                                    "conversion": {"kind": "UpTo", "ceiling_rate": 0.22}}]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{converting}");
    let choice = &converting["choices"][0];
    assert_eq!(choice["choice"]["conversion"]["kind"], "UpTo");
    assert_eq!(choice["conversion_overlay"], true);
    let years = choice["years"].as_array().unwrap();
    assert!(
        years
            .iter()
            .any(|y| y["conversion"].as_f64().unwrap() > 1.0)
    );

    let (status, error) = app
        .post(
            &format!("/api/runs/{run_id}/drawdown"),
            json!({"strategies": []}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    // The comparison runs a Monte Carlo per strategy.
    let (status, comparison) = app
        .post(
            &format!("/api/runs/{run_id}/drawdown/compare"),
            json!({"request": {"strategies": [
                {"kind": "AsPlanned"},
                {"kind": "Strategy", "strategy": "TaxFreeFirst"}]}, "iterations": 25}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{comparison}");
    assert_eq!(comparison["iterations"], 25);
    let rows = comparison["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[1]["median_final_net_worth"].as_f64().is_some());
    assert!(rows[1]["success_rate"].as_f64().unwrap() >= 0.0);

    let (status, error) = app
        .post(
            &format!("/api/runs/{run_id}/drawdown/compare"),
            json!({"iterations": 100_000}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
}

#[tokio::test]
async fn a_run_without_a_stored_seed_asks_to_be_run_again() {
    let (mut app, state) = app_with_state().await;
    app.login_as("drawdown-stale@example.com").await;
    let run_id = a_finished_run(&mut app).await;
    sqlx::query("UPDATE run_percentiles SET seed = NULL WHERE run_id = ?1")
        .bind(run_id)
        .execute(&state.db)
        .await
        .unwrap();

    for path in ["drawdown", "drawdown/compare"] {
        let (status, error) = app
            .post(&format!("/api/runs/{run_id}/{path}"), json!({}))
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{error}");
        assert_eq!(
            error["error"]["message"],
            "Run the plan again to see drawdown."
        );
    }
}

#[tokio::test]
async fn another_users_run_is_not_found() {
    let (mut app, _state) = app_with_state().await;
    app.login_as("drawdown-owner@example.com").await;
    let run_id = a_finished_run(&mut app).await;
    app.login_as("drawdown-other@example.com").await;
    for path in ["drawdown", "drawdown/compare"] {
        let (status, _) = app
            .post(&format!("/api/runs/{run_id}/{path}"), json!({}))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    let (status, _) = app.post("/api/runs/999999/drawdown", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
