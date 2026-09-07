//! End-to-end tests driving the real router in-process.
//!
//! Each test gets its own temporary SQLite file, so they are independent and can
//! run in parallel.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use finplan_server::config::ServerConfig;
use serde_json::{Value, json};
use tower::ServiceExt;

struct TestApp {
    router: Router,
    cookie: Option<String>,
    _dir: tempfile::TempDir,
}

impl TestApp {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");

        let config = ServerConfig {
            bind: "127.0.0.1:0".into(),
            database_url: format!("sqlite://{}", db_path.display()),
            db_pool_size: 4,
            sim_workers: 1,
            max_iterations: 50_000,
            secure_cookies: false,
            cors_origins: vec!["http://localhost:3000".into()],
        };

        let (router, _state) = finplan_server::build(config).await.expect("build app");
        TestApp {
            router,
            cookie: None,
            _dir: dir,
        }
    }

    async fn send(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(path);

        if let Some(cookie) = &self.cookie {
            request = request.header(header::COOKIE, cookie);
        }

        let request = match body {
            Some(json) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&json).unwrap()))
                .unwrap(),
            None => request.body(Body::empty()).unwrap(),
        };

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("body");

        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.send("GET", path, None).await
    }

    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send("POST", path, Some(body)).await
    }

    async fn put(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send("PUT", path, Some(body)).await
    }

    async fn patch(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send("PATCH", path, Some(body)).await
    }

    async fn delete(&self, path: &str) -> (StatusCode, Value) {
        self.send("DELETE", path, None).await
    }

    async fn delete_with(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send("DELETE", path, Some(body)).await
    }

    /// Poll a queued run until it settles, and report how it settled.
    async fn await_run(&self, run_id: i64) -> String {
        for _ in 0..200 {
            let (_, current) = self.get(&format!("/api/runs/{run_id}")).await;
            let status = current["status"].as_str().unwrap_or_default().to_string();
            if matches!(status.as_str(), "succeeded" | "failed" | "canceled") {
                return status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("run {run_id} never finished");
    }

    /// Register a user and keep the session cookie for subsequent calls.
    async fn login_as(&mut self, email: &str) {
        let request = Request::builder()
            .method("POST")
            .uri("/api/auth/register")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "email": email,
                    "password": "correct-horse-battery-staple",
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("register");
        assert_eq!(
            response.status(),
            StatusCode::CREATED,
            "registration failed"
        );

        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .expect("session cookie")
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();

        self.cookie = Some(cookie);
    }

    /// Build a small but complete scenario; returns its id and its account ids.
    async fn seed_scenario(&self) -> (i64, i64, i64) {
        let (_, profiles) = self.get("/api/return-profiles").await;
        let equity = profiles
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "US Total Market")
            .unwrap()["id"]
            .as_i64()
            .unwrap();
        let cash = profiles
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "Savings Account")
            .unwrap()["id"]
            .as_i64()
            .unwrap();

        let (status, scenario) = self
            .post(
                "/api/scenarios",
                json!({
                    "name": "Test plan",
                    "start_date": "2026-01-01",
                    "birth_date": "1985-01-01",
                    "duration_years": 10
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let scenario_id = scenario["id"].as_i64().unwrap();

        let (_, asset) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/assets"),
                json!({"name": "VTI", "initial_price": 100.0, "return_profile_id": equity}),
            )
            .await;
        let asset_id = asset["id"].as_i64().unwrap();

        let (_, checking) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/accounts"),
                json!({
                    "name": "Checking", "flavor": "Bank",
                    "cash_value": 10_000.0, "return_profile_id": cash
                }),
            )
            .await;
        let checking_id = checking["id"].as_i64().unwrap();

        let (_, brokerage) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/accounts"),
                json!({
                    "name": "Brokerage", "flavor": "Investment", "tax_status": "Taxable",
                    "cash_value": 0.0, "cash_return_profile_id": cash
                }),
            )
            .await;
        let brokerage_id = brokerage["id"].as_i64().unwrap();

        // 500 units at $100 = $50,000, so opening net worth is $60,000.
        let (status, _) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/accounts/{brokerage_id}/positions"),
                json!({"asset_id": asset_id, "units": 500.0, "cost_basis": 40_000.0}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);

        (scenario_id, checking_id, brokerage_id)
    }
}

#[tokio::test]
async fn funding_results_distinguish_shortfalls_from_positive_terminal_wealth() {
    let mut app = TestApp::new().await;
    app.login_as("funding@example.com").await;
    let (scenario_id, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario_id}");
    let (status, _) = app.patch(&base, json!({"duration_years": 1})).await;
    assert_eq!(status, StatusCode::OK);
    // Deterministic balances make both the old and new metric exact.
    let (_, profiles) = app.get("/api/return-profiles").await;
    for profile in profiles.as_array().unwrap() {
        let (status, _) = app
            .patch(
                &format!("/api/return-profiles/{}", profile["id"]),
                json!({"distribution": {"kind": "Fixed", "rate": 0.0}}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, run) = app
        .post(
            &format!("{base}/runs"),
            json!({"iterations": 4, "seed": 42}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run_id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");
    let (_, healthy) = app.get(&format!("/api/runs/{run_id}/results")).await;
    assert_eq!(healthy["stats"]["funding_success_rate"], 1.0);

    // A single expense overdraws checking, even though brokerage keeps net worth positive.
    let (status, _) = app
        .post(
            &format!("{base}/events"),
            json!({
                "name": "Unfunded spending", "enabled": true, "fires_once": true,
                "trigger": {"kind": "Date", "on_date": "2026-02-01"},
                "effects": [{"kind": "Expense", "from_account_id": checking,
                    "amount": {"kind": "Fixed", "value": 20000.0}}]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, run) = app
        .post(
            &format!("{base}/runs"),
            json!({"iterations": 4, "seed": 42}),
        )
        .await;
    let run_id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");
    for series in ["0.05", "0.5", "0.95"] {
        let (status, results) = app
            .get(&format!("/api/runs/{run_id}/results?series={series}"))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(results["stats"]["success_rate"], 1.0);
        assert_eq!(results["stats"]["funding_success_rate"], 0.0);
        assert_eq!(results["series_id"], series);
        let real = &results["real_net_worth"];
        assert_eq!(real["terminal"]["num_iterations"], 4);
        assert_eq!(real["terminal"]["base_date"], "2026-01-01");
        // These deterministic paths all include a cash shortfall. They still
        // contribute to EVERY real quantile, not just the funding metric.
        let path = results["bands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["path_id"] == series)
            .unwrap();
        for point in real["points"].as_array().unwrap() {
            let i = path["dates"]
                .as_array()
                .unwrap()
                .iter()
                .rposition(|d| *d == point["date"])
                .unwrap();
            let expected =
                path["net_worth"][i].as_f64().unwrap() / path["inflation"][i].as_f64().unwrap();
            for rank in ["p5", "p50", "p95"] {
                assert!((point[rank].as_f64().unwrap() - expected).abs() < 1e-8);
            }
        }
        assert!(
            results["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w["kind"] == "CashShortfall" && w["date"] == "2026-02-01")
        );
    }

    // Historical rows must remain explicitly unmeasured, not inferred from success_rate.
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        app._dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE run_stats SET funding_success_rate = NULL WHERE run_id = ?1")
        .bind(run_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM run_real_stats WHERE run_id = ?1")
        .bind(run_id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, legacy) = app.get(&format!("/api/runs/{run_id}/results")).await;
    assert_eq!(legacy["real_net_worth"], Value::Null);
    assert!(!legacy["bands"].as_array().unwrap().is_empty());
    assert_eq!(legacy["stats"]["funding_success_rate"], Value::Null);
    assert_eq!(legacy["stats"]["success_rate"], 1.0);
    // The engine's persisted statistics also accept old serialized results.
    let mut stats = legacy["stats"].clone();
    stats
        .as_object_mut()
        .unwrap()
        .remove("funding_success_rate");
    stats["percentile_values"] = json!([]);
    let old: finplan_core::model::MonteCarloStats = serde_json::from_value(stats).unwrap();
    assert_eq!(old.funding_success_rate, None);
    pool.close().await;
}

#[tokio::test]
async fn registration_seeds_a_usable_library() {
    let mut app = TestApp::new().await;
    app.login_as("seed@example.com").await;

    let (status, profiles) = app.get("/api/return-profiles").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        profiles.as_array().unwrap().len() >= 8,
        "new users should get a starter return-profile library"
    );

    let (_, taxes) = app.get("/api/tax-configs").await;
    let brackets = taxes[0]["federal_brackets"].as_array().unwrap();
    assert_eq!(brackets.len(), 7);
    assert_eq!(brackets[0]["threshold"], 0.0);
}

#[tokio::test]
async fn unauthenticated_requests_are_rejected() {
    let app = TestApp::new().await;
    let (status, _) = app.get("/api/scenarios").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_user_cannot_reach_another_users_scenario() {
    let mut owner = TestApp::new().await;
    owner.login_as("owner@example.com").await;
    let (scenario_id, _, _) = owner.seed_scenario().await;

    // Same database, different session.
    let mut intruder = TestApp {
        router: owner.router.clone(),
        cookie: None,
        _dir: tempfile::tempdir().unwrap(),
    };
    intruder.login_as("intruder@example.com").await;

    for path in [
        format!("/api/scenarios/{scenario_id}"),
        format!("/api/scenarios/{scenario_id}/accounts"),
        format!("/api/scenarios/{scenario_id}/events"),
    ] {
        let (status, _) = intruder.get(&path).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{path} leaked to another user"
        );
    }
}

#[tokio::test]
async fn scenario_compiles_with_every_account_flavor() {
    let mut app = TestApp::new().await;
    app.login_as("compile@example.com").await;
    let (scenario_id, _, _) = app.seed_scenario().await;

    let (status, report) = app
        .post(&format!("/api/scenarios/{scenario_id}/compile"), json!({}))
        .await;

    assert_eq!(status, StatusCode::OK, "compile failed: {report}");
    assert_eq!(report["accounts"], 2);
    assert_eq!(report["assets"], 1);
    assert_eq!(report["duration_years"], 10);
}

#[tokio::test]
async fn an_unmapped_asset_compiles_at_flat_zero_growth() {
    let mut app = TestApp::new().await;
    app.login_as("unmapped@example.com").await;
    let (scenario_id, _, brokerage) = app.seed_scenario().await;

    // No return profile: a ticker jotted down mid-sentence, to be mapped later.
    let (status, asset) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/assets"),
            json!({"name": "TBD", "initial_price": 50.0}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "create failed: {asset}");
    assert!(
        asset["return_profile_id"].is_null(),
        "expected unmapped: {asset}"
    );
    let asset_id = asset["id"].as_i64().unwrap();

    // Held, so the compiler has to give it a return one way or another.
    let (status, _) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/accounts/{brokerage}/positions"),
            json!({"asset_id": asset_id, "units": 10.0, "cost_basis": 500.0}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, report) = app
        .post(&format!("/api/scenarios/{scenario_id}/compile"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "compile failed: {report}");
    assert_eq!(report["assets"], 2);

    // Mapping it afterwards is a PATCH, and unmapping it again is the same
    // PATCH with an explicit null — which is why the field is doubly optional.
    let (_, profiles) = app.get("/api/return-profiles").await;
    let profile_id = profiles.as_array().unwrap()[0]["id"].as_i64().unwrap();

    let (status, mapped) = app
        .patch(
            &format!("/api/scenarios/{scenario_id}/assets/{asset_id}"),
            json!({"return_profile_id": profile_id}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "map failed: {mapped}");
    assert_eq!(mapped["return_profile_id"], profile_id);

    // A PATCH that says nothing about the mapping leaves it alone.
    let (_, renamed) = app
        .patch(
            &format!("/api/scenarios/{scenario_id}/assets/{asset_id}"),
            json!({"name": "STILL-TBD"}),
        )
        .await;
    assert_eq!(renamed["return_profile_id"], profile_id);

    let (_, unmapped) = app
        .patch(
            &format!("/api/scenarios/{scenario_id}/assets/{asset_id}"),
            json!({"return_profile_id": null}),
        )
        .await;
    assert!(
        unmapped["return_profile_id"].is_null(),
        "expected unmapped: {unmapped}"
    );
}

#[tokio::test]
async fn an_age_trigger_without_a_birth_date_is_rejected() {
    let mut app = TestApp::new().await;
    app.login_as("nobirth@example.com").await;

    let (_, scenario) = app
        .post(
            "/api/scenarios",
            json!({"name": "No birth date", "start_date": "2026-01-01"}),
        )
        .await;
    let scenario_id = scenario["id"].as_i64().unwrap();

    app.post(
        &format!("/api/scenarios/{scenario_id}/events"),
        json!({"name": "Retire", "trigger": {"kind": "Age", "years": 65}}),
    )
    .await;

    let (status, body) = app
        .post(&format!("/api/scenarios/{scenario_id}/compile"), json!({}))
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("birth_date"),
        "unexpected message: {body}"
    );
}

#[tokio::test]
async fn nested_triggers_and_effects_round_trip() {
    let mut app = TestApp::new().await;
    app.login_as("trees@example.com").await;
    let (scenario_id, checking, _) = app.seed_scenario().await;

    let payload = json!({
        "name": "Complex",
        "trigger": {
            "kind": "And",
            "children": [
                {"kind": "Age", "years": 70, "months": 6},
                {"kind": "NetWorth", "comparison": "LessThanOrEqual", "threshold": 500_000.0}
            ]
        },
        "effects": [{
            "kind": "Random",
            "probability": 0.25,
            "on_true": {
                "kind": "Expense",
                "from_account_id": checking,
                "amount": {
                    "kind": "Scale", "factor": 0.5,
                    "inner": {"kind": "AccountCashBalance", "account_id": checking}
                }
            },
            "on_false": {
                "kind": "Income", "to_account_id": checking, "income_type": "TaxFree",
                "amount": {"kind": "Fixed", "value": 1000.0}
            }
        }]
    });

    let (status, created) = app
        .post(&format!("/api/scenarios/{scenario_id}/events"), payload)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let event_id = created["id"].as_i64().unwrap();
    let (_, fetched) = app
        .get(&format!("/api/scenarios/{scenario_id}/events/{event_id}"))
        .await;

    // The nested shape must survive the trip through the flat tables.
    assert_eq!(fetched["trigger"]["kind"], "And");
    assert_eq!(fetched["trigger"]["children"][0]["years"], 70);
    assert_eq!(fetched["trigger"]["children"][0]["months"], 6);
    assert_eq!(fetched["trigger"]["children"][1]["kind"], "NetWorth");

    let effect = &fetched["effects"][0];
    assert_eq!(effect["kind"], "Random");
    assert_eq!(effect["probability"], 0.25);
    assert_eq!(effect["on_true"]["amount"]["kind"], "Scale");
    assert_eq!(effect["on_true"]["amount"]["factor"], 0.5);
    assert_eq!(
        effect["on_true"]["amount"]["inner"]["kind"],
        "AccountCashBalance"
    );
    assert_eq!(effect["on_false"]["kind"], "Income");
}

#[tokio::test]
async fn replacing_an_event_leaves_no_orphan_rows() {
    let mut app = TestApp::new().await;
    app.login_as("orphans@example.com").await;
    let (scenario_id, checking, _) = app.seed_scenario().await;

    // A tree with a nested amount and a Repeating end-condition, both of which
    // are reachable only from this event.
    let original = json!({
        "name": "Sweep",
        "trigger": {
            "kind": "Repeating", "interval": "Monthly",
            "end_condition": {"kind": "Age", "years": 80}
        },
        "effects": [{
            "kind": "Expense", "from_account_id": checking,
            "amount": {
                "kind": "Max",
                "left": {"kind": "Fixed", "value": 0.0},
                "right": {"kind": "Sub",
                          "left": {"kind": "AccountCashBalance", "account_id": checking},
                          "right": {"kind": "Fixed", "value": 5000.0}}
            }
        }]
    });

    let (_, event) = app
        .post(&format!("/api/scenarios/{scenario_id}/events"), original)
        .await;
    let event_id = event["id"].as_i64().unwrap();

    // Replace it with something far simpler; the old sub-tree is now unreachable.
    let (status, _) = app
        .send(
            "PUT",
            &format!("/api/scenarios/{scenario_id}/events/{event_id}"),
            Some(json!({
                "name": "Sweep",
                "trigger": {"kind": "Manual"},
                "effects": []
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // The scenario must still compile, which it cannot do with dangling rows.
    let (status, report) = app
        .post(&format!("/api/scenarios/{scenario_id}/compile"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["events"], 1);
}

#[tokio::test]
async fn duplicating_a_scenario_rewrites_every_reference() {
    let mut app = TestApp::new().await;
    app.login_as("clone@example.com").await;
    let (scenario_id, checking, _) = app.seed_scenario().await;

    app.post(
        &format!("/api/scenarios/{scenario_id}/events"),
        json!({
            "name": "Spend",
            "trigger": {"kind": "Repeating", "interval": "Monthly"},
            "effects": [{
                "kind": "Expense", "from_account_id": checking,
                "amount": {"kind": "AccountCashBalance", "account_id": checking}
            }]
        }),
    )
    .await;

    let (status, clone) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/duplicate"),
            json!({"name": "Copy"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{clone}");
    let clone_id = clone["id"].as_i64().unwrap();

    let (_, clone_accounts) = app
        .get(&format!("/api/scenarios/{clone_id}/accounts"))
        .await;
    let clone_account_ids: Vec<i64> = clone_accounts
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_i64().unwrap())
        .collect();

    let (_, clone_events) = app.get(&format!("/api/scenarios/{clone_id}/events")).await;
    let referenced = clone_events[0]["effects"][0]["from_account_id"]
        .as_i64()
        .unwrap();

    assert!(
        clone_account_ids.contains(&referenced),
        "clone's event points at account {referenced}, which is not one of its own {clone_account_ids:?}"
    );
    assert_ne!(referenced, checking, "clone still points at the original");

    // Deleting the source must leave the clone intact and runnable.
    let (status, _) = app.delete(&format!("/api/scenarios/{scenario_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, report) = app
        .post(&format!("/api/scenarios/{clone_id}/compile"), json!({}))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "clone broke when its source went away: {report}"
    );
}

#[tokio::test]
async fn a_monte_carlo_run_completes_and_stores_results() {
    let mut app = TestApp::new().await;
    app.login_as("runner@example.com").await;
    let (scenario_id, checking, _) = app.seed_scenario().await;

    app.post(
        &format!("/api/scenarios/{scenario_id}/events"),
        json!({
            "name": "Salary",
            "trigger": {"kind": "Repeating", "interval": "Monthly"},
            "effects": [{
                "kind": "Income", "to_account_id": checking, "income_type": "Taxable",
                "amount": {"kind": "Fixed", "value": 5000.0}
            }]
        }),
    )
    .await;

    let (status, run) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/runs"),
            json!({"iterations": 50, "seed": 1, "percentiles": [0.5]}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let run_id = run["id"].as_i64().unwrap();

    let mut status_text = String::new();
    for _ in 0..200 {
        let (_, current) = app.get(&format!("/api/runs/{run_id}")).await;
        status_text = current["status"].as_str().unwrap_or_default().to_string();
        if matches!(status_text.as_str(), "succeeded" | "failed" | "canceled") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(status_text, "succeeded", "run did not succeed");

    let (status, results) = app.get(&format!("/api/runs/{run_id}/results")).await;
    assert_eq!(status, StatusCode::OK, "{results}");

    assert_eq!(results["stats"]["num_iterations"], 50);

    // Opening net worth is $10,000 cash + 500 units at $100 = $60,000, and the
    // engine's own snapshot must agree with what we persisted.
    let opening = results["bands"][0]["net_worth"][0].as_f64().unwrap();
    assert!(
        (opening - 60_000.0).abs() < 1.0,
        "opening net worth was {opening}, expected 60000"
    );

    // Per-account series must be attributed back to real database rows.
    let labels: Vec<&str> = results["account_series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["label"].as_str().unwrap())
        .collect();
    assert!(labels.contains(&"Checking"), "got {labels:?}");
    assert!(labels.contains(&"Brokerage"), "got {labels:?}");
}

#[tokio::test]
async fn the_same_seed_produces_the_same_answer() {
    let mut app = TestApp::new().await;
    app.login_as("determinism@example.com").await;
    let (scenario_id, _, _) = app.seed_scenario().await;

    let mut means = Vec::new();
    for _ in 0..2 {
        let (_, run) = app
            .post(
                &format!("/api/scenarios/{scenario_id}/runs"),
                json!({"iterations": 40, "seed": 99, "percentiles": [0.5]}),
            )
            .await;
        let run_id = run["id"].as_i64().unwrap();

        for _ in 0..200 {
            let (_, current) = app.get(&format!("/api/runs/{run_id}")).await;
            if current["status"] == "succeeded" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let (_, results) = app.get(&format!("/api/runs/{run_id}/results")).await;
        means.push(results["stats"]["mean_final_net_worth"].as_f64().unwrap());
    }

    assert_eq!(
        means[0], means[1],
        "a fixed seed must give identical results across runs"
    );
}

#[tokio::test]
async fn invalid_tax_brackets_are_rejected() {
    let mut app = TestApp::new().await;
    app.login_as("taxes@example.com").await;

    // Rates are fractions; 22 means 2200%.
    let (status, _) = app
        .post(
            "/api/tax-configs",
            json!({"name": "Percent", "federal_brackets": [{"threshold": 0.0, "rate": 22.0}]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The engine assumes a bracket starting at zero.
    let (status, _) = app
        .post(
            "/api/tax-configs",
            json!({"name": "Gap", "federal_brackets": [{"threshold": 1000.0, "rate": 0.1}]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_profiles_asset_class_is_stored_seeded_and_clearable() {
    let mut app = TestApp::new().await;
    app.login_as("classes@example.com").await;

    let (_, profiles) = app.get("/api/return-profiles").await;
    let by_name = |name: &str| -> Value {
        profiles
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()
            .clone()
    };

    // The starter library is what a ticker resolves against, so its classes are
    // the part that has to be right without anyone setting them.
    assert_eq!(by_name("US Total Market")["asset_class"], "UsEquity");
    assert_eq!(by_name("US Aggregate Bonds")["asset_class"], "Bonds");
    assert_eq!(by_name("Cash / T-Bills")["asset_class"], "Cash");
    // Neither describes a holding, so neither should ever be auto-selected.
    assert!(by_name("Savings Account")["asset_class"].is_null());
    assert!(by_name("No Growth")["asset_class"].is_null());

    let id = by_name("US Total Market")["id"].as_i64().unwrap();
    let path = format!("/api/return-profiles/{id}");

    // A rename says nothing about the class, which is the whole point of
    // storing it rather than reading it back off the name.
    let (status, renamed) = app
        .patch(&path, json!({"name": "Equities (my assumptions)"}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["asset_class"], "UsEquity");

    let (status, moved) = app
        .patch(&path, json!({"asset_class": "GlobalEquity"}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["asset_class"], "GlobalEquity");

    // An explicit null unclassifies; absent would have left it alone.
    let (status, cleared) = app.patch(&path, json!({"asset_class": null})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(cleared["asset_class"].is_null());

    let (status, _) = app.patch(&path, json!({"asset_class": "Nonsense"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Created with one, and it comes back on the row.
    let (status, made) = app
        .post(
            "/api/return-profiles",
            json!({
                "name": "My bonds", "asset_class": "Bonds",
                "distribution": {"kind": "Fixed", "rate": 0.03}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(made["asset_class"], "Bonds");
}

#[tokio::test]
async fn inflation_profiles_reject_unsupported_distributions() {
    let mut app = TestApp::new().await;
    app.login_as("inflation@example.com").await;

    // `InflationProfile` has no StudentT variant.
    let (status, _) = app
        .post(
            "/api/inflation-profiles",
            json!({
                "name": "Fat tails",
                "distribution": {"kind": "StudentT", "mean": 0.03, "scale": 0.02, "df": 5.0}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn positions_are_confined_to_investment_accounts() {
    let mut app = TestApp::new().await;
    app.login_as("lots@example.com").await;
    let (scenario_id, checking, _) = app.seed_scenario().await;

    let (_, assets) = app
        .get(&format!("/api/scenarios/{scenario_id}/assets"))
        .await;
    let asset_id = assets[0]["id"].as_i64().unwrap();

    let (status, _) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/accounts/{checking}/positions"),
            json!({"asset_id": asset_id, "units": 1.0, "cost_basis": 1.0}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_lot_can_be_resized_and_removed() {
    let mut app = TestApp::new().await;
    app.login_as("resize@example.com").await;
    let (scenario_id, checking, brokerage) = app.seed_scenario().await;

    let (_, account) = app
        .get(&format!(
            "/api/scenarios/{scenario_id}/accounts/{brokerage}"
        ))
        .await;
    let position = account["positions"][0]["id"].as_i64().unwrap();
    let path = format!("/api/scenarios/{scenario_id}/accounts/{brokerage}/positions/{position}");

    // Units alone: everything absent is left as it was stored.
    let (status, resized) = app.patch(&path, json!({"units": 250.0})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resized["units"], 250.0);
    assert_eq!(resized["cost_basis"], 40_000.0);

    let (status, _) = app.patch(&path, json!({"units": -1.0})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The account in the path owns the lot; another one does not, even under
    // the same scenario.
    let (status, _) = app
        .patch(
            &format!("/api/scenarios/{scenario_id}/accounts/{checking}/positions/{position}"),
            json!({"units": 1.0}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = app.delete(&path).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app.patch(&path, json!({"units": 1.0})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_account_can_be_renamed_but_not_to_nothing() {
    let mut app = TestApp::new().await;
    app.login_as("rename@example.com").await;
    let (scenario_id, checking, _) = app.seed_scenario().await;
    let path = format!("/api/scenarios/{scenario_id}/accounts/{checking}");

    let (status, renamed) = app
        .patch(&path, json!({"name": "  Joint checking  "}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["name"], "Joint checking");
    // The flavor detail is untouched by a rename that does not carry one.
    assert_eq!(renamed["cash_value"], 10_000.0);

    let (status, _) = app.patch(&path, json!({"name": "   "})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_effect_naming_a_missing_event_is_a_bad_request() {
    let mut app = TestApp::new().await;
    app.login_as("dangling@example.com").await;
    let (scenario_id, _, _) = app.seed_scenario().await;

    // A `PauseEvent` whose target was never chosen: the id is not a row, so the
    // insert trips a foreign key. That is the caller's mistake, and reporting it
    // as a 500 would leave a client with nothing to act on.
    let (status, body) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/events"),
            json!({
                "name": "pauses nothing",
                "trigger": {"kind": "Date", "on_date": "2030-01-01"},
                "effects": [{"kind": "PauseEvent", "target_event_id": 0}]
            }),
        )
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not exist"),
        "message should say what was wrong: {body}"
    );
}

#[tokio::test]
async fn the_profile_form_can_clear_a_field_it_previously_set() {
    let mut app = TestApp::new().await;
    app.login_as("profile@example.com").await;

    let (status, user) = app
        .put(
            "/api/auth/profile",
            json!({
                "email": "profile@example.com",
                "display_name": "Dana A.",
                "birth_date": "1981-04-12"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["display_name"], "Dana A.");
    assert_eq!(user["birth_date"], "1981-04-12");

    // The whole block is replaced, not merged, so an emptied field actually
    // empties — the thing a COALESCE-style PATCH cannot express.
    let (status, user) = app
        .put(
            "/api/auth/profile",
            json!({"email": "profile@example.com", "display_name": "", "birth_date": null}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["display_name"], Value::Null);
    assert_eq!(user["birth_date"], Value::Null);

    let (_, me) = app.get("/api/auth/me").await;
    assert_eq!(me["birth_date"], Value::Null);
}

#[tokio::test]
async fn preferences_are_bounded_by_the_servers_own_limits() {
    let mut app = TestApp::new().await;
    app.login_as("prefs@example.com").await;

    let (_, me) = app.get("/api/auth/me").await;
    assert_eq!(me["default_iterations"], 2000, "seeded default");
    assert_eq!(me["auto_run"], false);

    let (status, user) = app
        .put(
            "/api/auth/preferences",
            json!({"default_iterations": 5000, "default_duration_years": 45, "auto_run": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["default_iterations"], 5000);
    assert_eq!(user["auto_run"], true);

    let (status, _) = app
        .put(
            "/api/auth/preferences",
            json!({"default_iterations": 999_999, "default_duration_years": 45, "auto_run": true}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "over --max-iterations");

    let (status, _) = app
        .put(
            "/api/auth/preferences",
            json!({"default_iterations": 5000, "default_duration_years": 0, "auto_run": false}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a zero-year horizon");
}

#[tokio::test]
async fn changing_the_password_ends_every_other_session() {
    let mut app = TestApp::new().await;
    app.login_as("rotate@example.com").await;
    let first = app.cookie.clone().expect("session cookie");

    // A second sign-in from somewhere else.
    let (status, _) = app
        .post(
            "/api/auth/login",
            json!({"email": "rotate@example.com", "password": "correct-horse-battery-staple"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (_, sessions) = app.get("/api/auth/sessions").await;
    assert_eq!(sessions.as_array().unwrap().len(), 2);
    assert_eq!(
        sessions
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["current"] == true)
            .count(),
        1,
        "exactly one row is this device"
    );

    let (status, body) = app
        .post(
            "/api/auth/password",
            json!({"current_password": "wrong-one-entirely", "new_password": "a-much-longer-one"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let (status, _) = app
        .post(
            "/api/auth/password",
            json!({
                "current_password": "correct-horse-battery-staple",
                "new_password": "a-much-longer-one"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The session that made the change survives; the other one is gone.
    assert_eq!(app.cookie.as_ref(), Some(&first));
    let (status, sessions) = app.get("/api/auth/sessions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sessions.as_array().unwrap().len(), 1);
    assert_eq!(sessions[0]["current"], true);
}

#[tokio::test]
async fn a_session_can_be_revoked_by_its_opaque_id() {
    let mut app = TestApp::new().await;
    app.login_as("devices@example.com").await;
    let (status, _) = app
        .post(
            "/api/auth/login",
            json!({"email": "devices@example.com", "password": "correct-horse-battery-staple"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (_, sessions) = app.get("/api/auth/sessions").await;
    let other = sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["current"] == false)
        .expect("the other device");
    let id = other["id"].as_str().unwrap().to_string();
    assert!(!id.is_empty(), "the id is opaque, not the token hash");

    let (status, _) = app.delete(&format!("/api/auth/sessions/{id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, sessions) = app.get("/api/auth/sessions").await;
    assert_eq!(sessions.as_array().unwrap().len(), 1);

    let (status, _) = app.delete(&format!("/api/auth/sessions/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "already gone");
}

#[tokio::test]
async fn deleting_an_account_takes_its_scenarios_with_it() {
    let mut app = TestApp::new().await;
    app.login_as("goodbye@example.com").await;
    let (scenario_id, _, _) = app.seed_scenario().await;

    let (status, _) = app
        .delete_with("/api/auth/me", json!({"password": "not-the-password"}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the password is re-typed");

    let (status, _) = app
        .delete_with(
            "/api/auth/me",
            json!({"password": "correct-horse-battery-staple"}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The session went with the user, so the cookie no longer resolves.
    let (status, _) = app.get(&format!("/api/scenarios/{scenario_id}")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // And the email is free again.
    let (status, _) = app
        .post(
            "/api/auth/register",
            json!({"email": "goodbye@example.com", "password": "correct-horse-battery-staple"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn a_scenario_row_carries_its_last_successful_run() {
    let mut app = TestApp::new().await;
    app.login_as("history@example.com").await;
    let (scenario_id, _, _) = app.seed_scenario().await;

    let (_, list) = app.get("/api/scenarios").await;
    assert_eq!(list[0]["last_run_at"], Value::Null, "nothing has run yet");
    assert_eq!(list[0]["last_success_rate"], Value::Null);

    let (status, run) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/runs"),
            json!({"iterations": 20, "seed": 7, "percentiles": [0.5]}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let run_id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");

    let (_, list) = app.get("/api/scenarios").await;
    assert!(list[0]["last_run_at"].is_string(), "{}", list[0]);
    assert!(
        list[0]["last_success_rate"].is_number(),
        "the figure the Data list shows: {}",
        list[0]
    );
}

/// Names in a list's order, so an assertion reads as the list does.
fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap().to_string())
        .collect()
}

fn ids(list: &Value) -> Vec<i64> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn every_list_can_be_dragged_into_a_new_order() {
    let mut app = TestApp::new().await;
    app.login_as("order@example.com").await;
    let (scenario_id, _, _) = app.seed_scenario().await;

    // A new row lands at the end of the list rather than in front of it — the
    // whole point of an order the user chose.
    for name in ["BND", "VXUS"] {
        let (status, _) = app
            .post(
                &format!("/api/scenarios/{scenario_id}/assets"),
                json!({"name": name, "initial_price": 50.0}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (_, assets) = app
        .get(&format!("/api/scenarios/{scenario_id}/assets"))
        .await;
    assert_eq!(names(&assets), ["VTI", "BND", "VXUS"], "created in order");

    let asset_ids = ids(&assets);
    let (status, _) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/assets/reorder"),
            json!({"ids": [asset_ids[2], asset_ids[0], asset_ids[1]]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, assets) = app
        .get(&format!("/api/scenarios/{scenario_id}/assets"))
        .await;
    assert_eq!(names(&assets), ["VXUS", "VTI", "BND"], "dragged to the top");

    // Accounts, the same way.
    let (_, accounts) = app
        .get(&format!("/api/scenarios/{scenario_id}/accounts"))
        .await;
    let account_ids = ids(&accounts);
    let reversed: Vec<i64> = account_ids.iter().rev().copied().collect();
    let before = names(&accounts);
    let (status, _) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/accounts/reorder"),
            json!({"ids": reversed}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, accounts) = app
        .get(&format!("/api/scenarios/{scenario_id}/accounts"))
        .await;
    let after = names(&accounts);
    assert_eq!(after, before.iter().rev().cloned().collect::<Vec<_>>());

    // The library is the user's rather than the scenario's, and reorders whole.
    let (_, profiles) = app.get("/api/return-profiles").await;
    let profile_ids = ids(&profiles);
    let last = *profile_ids.last().unwrap();
    let moved: Vec<i64> = std::iter::once(last)
        .chain(profile_ids.iter().copied().filter(|id| *id != last))
        .collect();
    let (status, _) = app
        .post("/api/return-profiles/reorder", json!({"ids": moved}))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, profiles) = app.get("/api/return-profiles").await;
    assert_eq!(ids(&profiles)[0], last, "the bottom profile is now the top");

    // An id the collection does not hold is dropped rather than refused: a list
    // a beat out of date should still reorder under the user's hand.
    let (status, _) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/assets/reorder"),
            json!({"ids": [asset_ids[1], 999_999, asset_ids[0], asset_ids[2]]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, assets) = app
        .get(&format!("/api/scenarios/{scenario_id}/assets"))
        .await;
    assert_eq!(names(&assets), ["BND", "VTI", "VXUS"]);

    // A partial list places what it names and leaves the rest behind it.
    let (status, _) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/assets/reorder"),
            json!({"ids": [asset_ids[2]]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, assets) = app
        .get(&format!("/api/scenarios/{scenario_id}/assets"))
        .await;
    assert_eq!(names(&assets), ["VXUS", "BND", "VTI"]);
}

#[tokio::test]
async fn lots_keep_the_order_they_were_dragged_into() {
    let mut app = TestApp::new().await;
    app.login_as("lots@example.com").await;
    let (scenario_id, _, brokerage) = app.seed_scenario().await;

    let (_, assets) = app
        .get(&format!("/api/scenarios/{scenario_id}/assets"))
        .await;
    let asset_id = ids(&assets)[0];

    // Deliberately out of date order, to prove the list is not just re-sorting
    // by purchase date behind the caller's back.
    for date in ["2024-06-01", "2022-01-01", "2023-03-01"] {
        let (status, _) = app
            .post(
                &format!("/api/scenarios/{scenario_id}/accounts/{brokerage}/positions"),
                json!({"asset_id": asset_id, "purchase_date": date, "units": 1.0, "cost_basis": 10.0}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let path = format!("/api/scenarios/{scenario_id}/accounts/{brokerage}/positions");
    let (_, lots) = app.get(&path).await;
    let dates: Vec<&str> = lots
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["purchase_date"].as_str().unwrap())
        .collect();
    let seeded = lots.as_array().unwrap().len();
    assert_eq!(
        &dates[seeded - 3..],
        ["2024-06-01", "2022-01-01", "2023-03-01"],
        "added in the order they were added, not sorted by date"
    );

    let lot_ids = ids(&lots);
    let reversed: Vec<i64> = lot_ids.iter().rev().copied().collect();
    let (status, _) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/accounts/{brokerage}/positions/reorder"),
            json!({"ids": reversed}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, lots) = app.get(&path).await;
    assert_eq!(ids(&lots), reversed);

    // And the account's own row carries the same order the list does.
    let (_, account) = app
        .get(&format!(
            "/api/scenarios/{scenario_id}/accounts/{brokerage}"
        ))
        .await;
    assert_eq!(ids(&account["positions"]), reversed);
}

#[tokio::test]
async fn a_reordered_list_survives_being_duplicated() {
    let mut app = TestApp::new().await;
    app.login_as("dup-order@example.com").await;
    let (scenario_id, _, _) = app.seed_scenario().await;

    let (_, accounts) = app
        .get(&format!("/api/scenarios/{scenario_id}/accounts"))
        .await;
    let reversed: Vec<i64> = ids(&accounts).iter().rev().copied().collect();
    let expected: Vec<String> = names(&accounts).into_iter().rev().collect();
    let (status, _) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/accounts/reorder"),
            json!({"ids": reversed}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, copy) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/duplicate"),
            json!({"name": "Copy"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{copy}");
    let copy_id = copy["id"].as_i64().unwrap();

    let (_, copied) = app.get(&format!("/api/scenarios/{copy_id}/accounts")).await;
    assert_eq!(
        names(&copied),
        expected,
        "the copy reads as the original did"
    );
}

/// The cash-flow table's drawer: a year index on `results`, and the entries
/// behind one year on `/ledger`.
#[tokio::test]
async fn a_year_of_the_ledger_can_be_read_back_and_filtered() {
    let mut app = TestApp::new().await;
    app.login_as("ledger@example.com").await;
    let (scenario_id, checking, _) = app.seed_scenario().await;

    app.post(
        &format!("/api/scenarios/{scenario_id}/events"),
        json!({
            "name": "Salary",
            "trigger": {"kind": "Repeating", "interval": "Monthly"},
            "effects": [{
                "kind": "Income", "to_account_id": checking, "income_type": "Taxable",
                "amount": {"kind": "Fixed", "value": 5000.0}
            }]
        }),
    )
    .await;

    // Born 1985 and starting in 2026, so this fires in 2030 — the one year in
    // the run that is not like the others.
    app.post(
        &format!("/api/scenarios/{scenario_id}/events"),
        json!({
            "name": "Buy the boat",
            "fires_once": true,
            "trigger": {"kind": "Age", "years": 45},
            "effects": [{
                "kind": "Expense", "from_account_id": checking,
                "amount": {"kind": "Fixed", "value": 20_000.0}
            }]
        }),
    )
    .await;

    let (status, run) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/runs"),
            json!({"iterations": 20, "seed": 7, "percentiles": [0.5]}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let run_id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");

    let (status, results) = app.get(&format!("/api/runs/{run_id}/results")).await;
    assert_eq!(status, StatusCode::OK, "{results}");

    // Deflating anything needs the path's own inflation, which starts at 1.0
    // in the plan's first year.
    let inflation = results["inflation"].as_array().unwrap();
    assert!(!inflation.is_empty(), "no inflation was recorded");
    assert_eq!(inflation[0]["factor"], 1.0);

    let years = results["ledger_years"].as_array().unwrap();
    assert!(!years.is_empty(), "no ledger was recorded");
    let counted: i64 = years.iter().map(|y| y["total"].as_i64().unwrap()).sum();
    assert!(counted > 0, "the ledger index counted nothing");

    // The tag marks the year an event *started*, so the monthly salary tags
    // only its first year, not all ten. A tag on every year would mark nothing.
    let tagged: Vec<(i64, &str)> = years
        .iter()
        .filter_map(|y| Some((y["year"].as_i64()?, y["tag"].as_str()?)))
        .collect();
    assert_eq!(
        tagged,
        vec![(2026, "Salary"), (2030, "Buy the boat")],
        "got {tagged:?}"
    );

    // A year reads back exactly the entries the index counted for it.
    let (status, page) = app
        .get(&format!("/api/runs/{run_id}/ledger?year=2030"))
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let entries = page["entries"].as_array().unwrap();
    let expected = years.iter().find(|y| y["year"] == 2030).unwrap()["total"]
        .as_i64()
        .unwrap();
    assert_eq!(page["total"], expected);
    assert_eq!(entries.len() as i64, expected);
    assert!(
        entries.iter().all(|e| e["year"] == 2030),
        "the year filter leaked other years"
    );
    assert!(
        entries
            .iter()
            .any(|e| e["kind"] == "Triggered" && e["detail"] == "Buy the boat"),
        "the triggering event is missing from its own year"
    );

    // And the filter chips scope it to one bucket.
    let (status, cash) = app
        .get(&format!(
            "/api/runs/{run_id}/ledger?year=2030&category=cash"
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{cash}");
    assert!(
        cash["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["category"] == "cash"),
        "the category filter leaked other buckets"
    );
    assert_eq!(
        cash["total"],
        years.iter().find(|y| y["year"] == 2030).unwrap()["cash"],
        "the page total disagrees with the index the table drew its count from"
    );
}

/// The chart's P5 / P50 / P95 switch is a `series` query, because the fan is
/// stored per percentile but the per-account series and cash flows describe one
/// path at a time. A percentile the run did not store resolves to the nearest
/// one it did, so a client naming "the bad case" as 0.05 gets a path back
/// whatever marks the run happened to be started with.
#[tokio::test]
async fn results_follow_the_percentile_the_caller_asks_for() {
    let mut app = TestApp::new().await;
    app.login_as("series@example.com").await;
    let (scenario_id, _, _) = app.seed_scenario().await;

    let (status, run) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/runs"),
            json!({"iterations": 120, "seed": 7, "percentiles": [0.05, 0.5, 0.95]}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let run_id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");

    /// Final worth of every account on the path a `series` query names.
    async fn final_worth(app: &TestApp, run_id: i64, query: &str) -> f64 {
        let (status, results) = app.get(&format!("/api/runs/{run_id}/results{query}")).await;
        assert_eq!(status, StatusCode::OK, "{results}");
        let series = results["account_series"].as_array().unwrap();
        assert!(!series.is_empty(), "no account series for {query}");
        series
            .iter()
            .map(|s| {
                let values = s["values"].as_array().unwrap();
                values.last().unwrap().as_f64().unwrap()
            })
            .sum()
    }

    let low = final_worth(&app, run_id, "?series=0.05").await;
    let median = final_worth(&app, run_id, "?series=0.5").await;
    let high = final_worth(&app, run_id, "?series=0.95").await;

    assert!(
        low < median && median < high,
        "the series query returned the same path for every percentile: \
         {low} / {median} / {high}"
    );
    assert_eq!(
        final_worth(&app, run_id, "").await,
        median,
        "the default series is no longer the median path"
    );

    // 0.9 was never stored, so the 0.95 path answers for it.
    assert_eq!(
        final_worth(&app, run_id, "?series=0.9").await,
        high,
        "an unstored percentile did not fall back to the nearest stored path"
    );

    let (_, low_result) = app
        .get(&format!("/api/runs/{run_id}/results?series=0.05"))
        .await;
    let (_, high_result) = app
        .get(&format!("/api/runs/{run_id}/results?series=0.9"))
        .await;
    assert_eq!(high_result["series_id"], "0.95");
    assert_eq!(
        low_result["real_net_worth"], high_result["real_net_worth"],
        "selecting a path must not change the envelope"
    );
    let real = &high_result["real_net_worth"];
    assert_eq!(real["terminal"]["num_iterations"], 120);
    for point in real["points"].as_array().unwrap() {
        let p5 = point["p5"].as_f64().unwrap();
        let p50 = point["p50"].as_f64().unwrap();
        let p95 = point["p95"].as_f64().unwrap();
        assert!(p5 <= p50 && p50 <= p95);
    }
    let (_, ledger) = app
        .get(&format!("/api/runs/{run_id}/ledger?series=0.9"))
        .await;
    assert_eq!(ledger["run_id"], run_id);
    assert_eq!(ledger["series_id"], high_result["series_id"]);

    let (status, bad) = app
        .get(&format!("/api/runs/{run_id}/results?series=later"))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    for invalid in ["NaN", "inf", "-1", "1.1"] {
        assert_eq!(
            app.get(&format!("/api/runs/{run_id}/results?series={invalid}"))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    let owner = app.cookie.clone();
    app.login_as("other-series@example.com").await;
    for endpoint in ["results", "ledger"] {
        assert_eq!(
            app.get(&format!("/api/runs/{run_id}/{endpoint}")).await.0,
            StatusCode::NOT_FOUND
        );
    }
    app.cookie = owner;
    assert_eq!(
        app.delete(&format!("/api/runs/{run_id}")).await.0,
        StatusCode::NO_CONTENT
    );
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        app._dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    for table in ["run_real_stats", "run_real_quantiles"] {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE run_id = ?1"))
                .bind(run_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0, "deleting a run must delete its real measurements");
    }
    pool.close().await;
}
