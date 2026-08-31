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
