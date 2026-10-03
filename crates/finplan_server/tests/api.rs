//! End-to-end tests driving the real router in-process.
//!
//! Each test gets its own temporary SQLite file, so they are independent and can
//! run in parallel.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use finplan_server::config::ServerConfig;
use finplan_server::suggest::ai::AiClient;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

struct TestApp {
    router: Router,
    cookie: Option<String>,
    _dir: tempfile::TempDir,
}

impl TestApp {
    async fn new() -> Self {
        Self::with_review_ai(None).await
    }

    /// An app whose review model is `review_ai` (a scripted client), or none.
    async fn with_review_ai(review_ai: Option<Arc<AiClient>>) -> Self {
        Self::with_draft_config(review_ai, |_| {}).await
    }

    /// [`with_review_ai`](Self::with_review_ai), with the draft settings
    /// adjusted (limits, say). Held originals go under the app's own temp
    /// directory, so a test can look at what is on disk.
    async fn with_draft_config(
        review_ai: Option<Arc<AiClient>>,
        adjust: impl FnOnce(&mut finplan_server::suggest::ai::DraftConfig),
    ) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut draft = finplan_server::suggest::ai::DraftConfig {
            temp_dir: Some(dir.path().join("draft-files")),
            ..Default::default()
        };
        adjust(&mut draft);
        let router = Self::router(&dir, review_ai, draft, Default::default()).await;
        TestApp {
            router,
            cookie: None,
            _dir: dir,
        }
    }

    /// [`with_review_ai`](Self::with_review_ai), with the plan chat allowance
    /// set.
    async fn with_plan_chat(
        review_ai: Option<Arc<AiClient>>,
        plan_chat: finplan_server::suggest::ai::PlanChatConfig,
    ) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let draft = finplan_server::suggest::ai::DraftConfig {
            temp_dir: Some(dir.path().join("draft-files")),
            ..Default::default()
        };
        let router = Self::router(&dir, review_ai, draft, plan_chat).await;
        TestApp {
            router,
            cookie: None,
            _dir: dir,
        }
    }

    async fn router(
        dir: &tempfile::TempDir,
        review_ai: Option<Arc<AiClient>>,
        draft: finplan_server::suggest::ai::DraftConfig,
        plan_chat: finplan_server::suggest::ai::PlanChatConfig,
    ) -> Router {
        let db_path = dir.path().join("test.db");

        let config = ServerConfig {
            mail: Default::default(),
            review_ai: Default::default(),
            draft,
            plan_chat,
            log_format: Default::default(),
            metrics_bind: None,
            bind: "127.0.0.1:0".into(),
            database_url: format!("sqlite://{}", db_path.display()),
            db_pool_size: 4,
            sim_workers: 1,
            max_iterations: 50_000,
            secure_cookies: false,
            hosted: false,
            access_mode: Default::default(),
            registration_open: true,
            guest_access: true,
            guest_max_iterations: 100,
            guest_retention_days: 30,
            local_mail_sink: None,
            cors_origins: vec!["http://localhost:3000".into()],
        };

        let (router, _state) = finplan_server::build_with(config, review_ai)
            .await
            .expect("build app");
        router
    }

    /// A new process over the same database, as after a restart; the session
    /// cookie survives because sessions live in the database.
    async fn restart(&mut self) {
        let draft = finplan_server::suggest::ai::DraftConfig {
            temp_dir: Some(self._dir.path().join("draft-files")),
            ..Default::default()
        };
        self.router = Self::router(&self._dir, None, draft, Default::default()).await;
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
                    "password_confirmation": "correct-horse-battery-staple",
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
async fn contact_messages_are_private_validated_and_rate_limited() {
    let mut app = TestApp::new().await;

    let (status, _) = app
        .post(
            "/api/contact-messages",
            json!({"topic":"question", "message":"Can you help?"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    app.login_as("contact@example.com").await;

    for message in ["", "   "] {
        let (status, error) = app
            .post(
                "/api/contact-messages",
                json!({"topic":"feedback", "message":message}),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    }
    let (status, error) = app
        .post(
            "/api/contact-messages",
            json!({"topic":"feedback", "message":"x".repeat(2_001)}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    for number in 1..=5 {
        let (status, receipt) = app
            .post(
                "/api/contact-messages",
                json!({
                    "topic":"bug_report",
                    "message":format!("  Private message {number}  ")
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{receipt}");
        assert_eq!(receipt["topic"], "bug_report");
        assert_eq!(receipt["status"], "new");
        assert!(receipt.get("message").is_none());
    }

    let (status, error) = app
        .post(
            "/api/contact-messages",
            json!({"topic":"question", "message":"One too many"}),
        )
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{error}");
    assert_eq!(error["error"]["code"], "rate_limited");

    let db = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        app._dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let messages: Vec<String> =
        sqlx::query_scalar("SELECT message FROM contact_messages ORDER BY id")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[0], "Private message 1");

    let (status, _) = app
        .delete_with(
            "/api/auth/me",
            json!({"password":"correct-horse-battery-staple"}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contact_messages")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(
        remaining, 0,
        "messages follow the account retention boundary"
    );
}

#[tokio::test]
async fn a_scenario_can_be_renamed_but_not_to_nothing() {
    let mut app = TestApp::new().await;
    app.login_as("scenario-rename@example.com").await;
    let (status, scenario) = app
        .post(
            "/api/scenarios",
            json!({"name":"Original", "start_date":"2026-01-01"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{scenario}");
    let id = scenario["id"].as_i64().unwrap();
    let path = format!("/api/scenarios/{id}");

    let (status, renamed) = app.patch(&path, json!({"name":"  Renamed  "})).await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "Renamed");

    let (status, error) = app.patch(&path, json!({"name":"   "})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(app.get(&path).await.1["name"], "Renamed");
}

#[tokio::test]
async fn a_retirement_account_keeps_its_plan_type_and_catch_up() {
    let mut app = TestApp::new().await;
    app.login_as("plan-type@example.com").await;
    let (scenario_id, _, _) = app.seed_scenario().await;
    let (_, profiles) = app.get("/api/return-profiles").await;
    let cash = profiles[0]["id"].as_i64().unwrap();
    let accounts = format!("/api/scenarios/{scenario_id}/accounts");
    let k401 = |extra: Value| {
        let mut body = json!({
            "name": "Work 401(k)", "flavor": "Investment", "tax_status": "TaxDeferred",
            "cash_value": 0.0, "cash_return_profile_id": cash,
            "plan_type": "Traditional401k",
            "contribution_limit": 24500.0, "contribution_period": "Yearly",
            "catch_up": [
                {"from_age": 50, "through_age": null, "amount": 8000.0},
                {"from_age": 60, "through_age": 63, "amount": 11250.0}
            ]
        });
        for (key, value) in extra.as_object().unwrap() {
            body[key] = value.clone();
        }
        body
    };

    let (status, created) = app.post(&accounts, k401(json!({}))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let path = format!("{accounts}/{}", created["id"]);
    let (_, fetched) = app.get(&path).await;
    assert_eq!(fetched["plan_type"], "Traditional401k");
    assert_eq!(fetched["catch_up"][1]["through_age"], 63);
    assert_eq!(fetched["catch_up"][1]["amount"], 11250.0);

    // The plan decides the tax treatment; a Roth-taxed traditional 401(k) is refused.
    let (status, error) = app
        .post(
            &accounts,
            k401(json!({"name": "Bad", "tax_status": "TaxFree"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    // A catch-up adds to a limit, so it needs one.
    let (status, error) = app
        .post(
            &accounts,
            k401(json!({"name": "Bad", "contribution_limit": null, "contribution_period": null})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    // Accounts written before plan types existed read back with neither.
    let (_, listed) = app.get(&accounts).await;
    let brokerage = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "Brokerage")
        .unwrap();
    assert_eq!(brokerage["plan_type"], Value::Null);
    assert_eq!(brokerage["catch_up"], json!([]));
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
    let healthy_run = run_id;
    let (_, healthy) = app.get(&format!("/api/runs/{run_id}/results")).await;
    assert_eq!(healthy["stats"]["funding_success_rate"], 1.0);
    let clean = &healthy["funding_diagnostics"];
    assert_eq!(clean["iterations"], 4);
    assert_eq!(clean["failed"], 0);
    assert_eq!(clean["shortfall_accounts"], json!([]));
    assert_eq!(clean["worst_seed"], Value::Null);

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
    // The default example runs sit at the fan's outer band.
    for series in ["0.1", "0.5", "0.9"] {
        let (status, results) = app
            .get(&format!("/api/runs/{run_id}/results?series={series}"))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(results["stats"]["success_rate"], 1.0);
        assert_eq!(results["stats"]["funding_success_rate"], 0.0);
        assert_eq!(results["series_id"], series);
        let real = &results["real_net_worth"];
        // Both fan bands are stored, nested around the median.
        for point in real["points"].as_array().unwrap() {
            let q = |k: &str| point[k].as_f64().unwrap_or_else(|| panic!("{k}: {point}"));
            assert!(q("p5") <= q("p10") && q("p10") <= q("p25") && q("p25") <= q("p50"));
            assert!(q("p50") <= q("p75") && q("p75") <= q("p90") && q("p90") <= q("p95"));
        }
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
                .any(|w| w["kind"] == "CashShortfall"
                    && w["date"] == "2026-02-01"
                    && w["account_id"] == checking)
        );
    }

    // The run-wide diagnostics say where every failing path ran short, in row ids.
    let (_, results) = app.get(&format!("/api/runs/{run_id}/results")).await;
    let funding = &results["funding_diagnostics"];
    assert_eq!(funding["iterations"], 4);
    assert_eq!(funding["failed"], 4);
    assert_eq!(funding["cash_shortfall"], 4);
    assert_eq!(funding["event_failure"], 0);
    assert_eq!(funding["failed_solvent"], 4);
    assert_eq!(
        funding["first_shortfall_years"],
        json!([{"year": 2026, "count": 4}])
    );
    assert_eq!(funding["median_first_shortfall_year"], 2026);
    assert_eq!(
        funding["shortfall_accounts"],
        json!([{"account_id": checking, "count": 4}])
    );
    assert!(funding["median_max_deficit"].as_f64().unwrap() > 0.0);
    // A u64 seed, carried as a string so JavaScript cannot round it.
    assert!(
        funding["worst_seed"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .is_ok()
    );
    // The report embeds the same results.
    let (_, report) = app.get(&format!("/api/runs/{run_id}/report")).await;
    assert_eq!(report["results"]["funding_diagnostics"], *funding);
    // Superseded runs lose their paths but keep the diagnostics.
    let (_, superseded) = app.get(&format!("/api/runs/{healthy_run}/results")).await;
    assert_eq!(superseded["path_details"], false);
    assert_eq!(superseded["funding_diagnostics"]["failed"], 0);

    // Historical rows must remain explicitly unmeasured, not inferred from success_rate.
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        app._dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    sqlx::query(
        "UPDATE run_stats SET funding_success_rate = NULL, funding_diagnostics = NULL
          WHERE run_id = ?1",
    )
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
    assert_eq!(legacy["funding_diagnostics"], Value::Null);
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
    // The 2024 single deduction goes with the 2024 single brackets.
    assert_eq!(taxes[0]["standard_deduction"], 14_600.0);
    assert_eq!(taxes[0]["age_65_extra_deduction"], 1_950.0);
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
async fn a_converging_run_stops_short_of_its_ceiling() {
    let mut app = TestApp::new().await;
    app.login_as("converge@example.com").await;
    let (scenario_id, _, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario_id}");
    let (status, _) = app.patch(&base, json!({"duration_years": 1})).await;
    assert_eq!(status, StatusCode::OK);

    // Fixed returns settle the median immediately, so the run has to stop on
    // the metric rather than by exhausting its ceiling.
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
            json!({
                "iterations": 20, "seed": 42, "percentiles": [0.5], "converge": true,
                "batch_size": 10, "parallel_batches": 2
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    assert_eq!(run["converge"], true);
    assert_eq!(run["iterations"], 20, "the minimum sample, not the count");
    let ceiling = run["max_iterations"].as_i64().expect("a ceiling");
    assert_eq!(ceiling, 10_000);

    let run_id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");

    let (_, results) = app.get(&format!("/api/runs/{run_id}/results")).await;
    let taken = results["stats"]["num_iterations"].as_i64().unwrap();
    assert!(taken >= 20, "at least the minimum sample, took {taken}");
    assert!(taken < ceiling, "stopped on the metric, took {taken}");
}

#[tokio::test]
async fn a_scenario_retains_its_prior_runs() {
    let mut app = TestApp::new().await;
    app.login_as("one-run@example.com").await;
    let (scenario_id, checking, _) = app.seed_scenario().await;

    // Guarantee both retained-detail tables have rows to exercise. The
    // scenario records a monthly ledger entry and an annual cash-flow summary.
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

    let path = format!("/api/scenarios/{scenario_id}/runs");
    let body = |iterations: i64| json!({"iterations": iterations, "seed": 7, "percentiles": [0.5]});

    let (_, first) = app.post(&path, body(30)).await;
    let first_id = first["id"].as_i64().unwrap();
    assert_eq!(app.await_run(first_id).await, "succeeded");
    let (_, first_results) = app.get(&format!("/api/runs/{first_id}/results")).await;
    assert!(!first_results["cash_flows"].as_array().unwrap().is_empty());
    assert!(!first_results["ledger_years"].as_array().unwrap().is_empty());
    assert_eq!(first_results["path_details"], true);

    let (status, second) = app.post(&path, body(40)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    let second_id = second["id"].as_i64().unwrap();
    assert_eq!(app.await_run(second_id).await, "succeeded");

    // Both runs remain, newest first.
    let (_, runs) = app.get(&format!("/api/scenarios/{scenario_id}/runs")).await;
    let rows = runs.as_array().unwrap();
    assert_eq!(rows.len(), 2, "a scenario retains history: {runs}");
    assert_eq!(rows[0]["id"].as_i64().unwrap(), second_id);
    assert_eq!(rows[0]["iterations"], 40);

    assert_ne!(first_id, second_id);
    let (status, results) = app.get(&format!("/api/runs/{first_id}/results")).await;
    assert_eq!(status, StatusCode::OK);
    // The superseded run keeps its summary and nothing path-shaped.
    assert_eq!(results["stats"]["num_iterations"], 30);
    assert_eq!(results["path_details"], false);
    assert!(results["real_net_worth"]["terminal"]["mean"].is_number());
    for field in [
        "bands",
        "account_series",
        "cash_flows",
        "inflation",
        "warnings",
        "ledger_years",
    ] {
        assert_eq!(results[field], json!([]), "{field} survived supersession");
    }
    assert_eq!(results["real_net_worth"]["points"], json!([]));
    let (status, _) = app
        .get(&format!("/api/runs/{first_id}/results?series=0.9"))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a series query still serves the summary"
    );

    let (status, results) = app.get(&format!("/api/runs/{second_id}/results")).await;
    assert_eq!(status, StatusCode::OK, "{results}");
    assert_eq!(results["path_details"], true);
    assert!(!results["bands"].as_array().unwrap().is_empty());
    assert_eq!(results["stats"]["num_iterations"], 40);
    assert!(!results["cash_flows"].as_array().unwrap().is_empty());
    assert!(!results["ledger_years"].as_array().unwrap().is_empty());
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
async fn a_tax_configs_deductions_round_trip_and_reject_negatives() {
    let mut app = TestApp::new().await;
    app.login_as("deduction@example.com").await;
    let brackets = json!([{"threshold": 0.0, "rate": 0.1}]);

    // Left out, a config has no deduction: the engine taxes as before.
    let (status, plain) = app
        .post(
            "/api/tax-configs",
            json!({"name": "Plain", "federal_brackets": brackets}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(plain["standard_deduction"], 0.0);
    assert_eq!(plain["age_65_extra_deduction"], 0.0);

    let path = format!("/api/tax-configs/{}", plain["id"]);
    let (status, updated) = app
        .patch(
            &path,
            json!({"standard_deduction": 29_200.0, "age_65_extra_deduction": 3_100.0}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["standard_deduction"], 29_200.0);
    assert_eq!(updated["age_65_extra_deduction"], 3_100.0);

    // A patch that leaves them out keeps them.
    let (_, renamed) = app.patch(&path, json!({"name": "Joint"})).await;
    assert_eq!(renamed["standard_deduction"], 29_200.0);

    let (status, _) = app.patch(&path, json!({"standard_deduction": -1.0})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = app
        .post(
            "/api/tax-configs",
            json!({"name": "Negative", "age_65_extra_deduction": -5.0, "federal_brackets": brackets}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
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
            json!({"display_name": "", "birth_date": null}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["display_name"], Value::Null);
    assert_eq!(user["birth_date"], Value::Null);

    let (_, me) = app.get("/api/auth/me").await;
    assert_eq!(me["birth_date"], Value::Null);
}

#[tokio::test]
async fn profile_email_is_not_an_editable_field() {
    let mut app = TestApp::new().await;
    app.login_as("fixed-email@example.com").await;

    let (status, _) = app
        .put(
            "/api/auth/profile",
            json!({
                "email": "other@example.com",
                "display_name": "Dana A.",
                "birth_date": "1981-04-12"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (_, me) = app.get("/api/auth/me").await;
    assert_eq!(me["email"], "fixed-email@example.com");
}

#[tokio::test]
async fn preferences_are_bounded_by_the_servers_own_limits() {
    let mut app = TestApp::new().await;
    app.login_as("prefs@example.com").await;

    let (_, me) = app.get("/api/auth/me").await;
    assert_eq!(me["default_iterations"], 2000, "seeded default");
    assert_eq!(me["auto_run"], false);
    assert!(
        me.get("theme_mode").is_none() && me.get("accent").is_none(),
        "appearance is kept on the device, not the account"
    );

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

    // …and neither refusal half-wrote the row.
    let (_, me) = app.get("/api/auth/me").await;
    assert_eq!(me["default_iterations"], 5000);
    assert_eq!(me["default_duration_years"], 45);
    assert_eq!(me["auto_run"], true);
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
            json!({
                "current_password": "wrong-one-entirely",
                "new_password": "a-much-longer-one",
                "new_password_confirmation": "a-much-longer-one"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let (status, _) = app
        .post(
            "/api/auth/password",
            json!({
                "current_password": "correct-horse-battery-staple",
                "new_password": "a-much-longer-one",
                "new_password_confirmation": "a-much-longer-one"
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
async fn registration_and_password_change_require_matching_confirmation() {
    let mut app = TestApp::new().await;

    let (status, _) = app
        .post(
            "/api/auth/register",
            json!({"email": "match@example.com", "password": "a-long-enough-password"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, body) = app
        .post(
            "/api/auth/register",
            json!({
                "email": "match@example.com",
                "password": "a-long-enough-password",
                "password_confirmation": "a-different-password"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["message"], "passwords do not match");

    app.login_as("match@example.com").await;
    let (status, body) = app
        .post(
            "/api/auth/password",
            json!({
                "current_password": "correct-horse-battery-staple",
                "new_password": "a-different-long-password",
                "new_password_confirmation": "a-third-long-password"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["message"], "passwords do not match");
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
            json!({
                "email": "goodbye@example.com",
                "password": "correct-horse-battery-staple",
                "password_confirmation": "correct-horse-battery-staple"
            }),
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

    // A second one-off in the same year: both have to be named, not just the
    // one that happened to fire first.
    app.post(
        &format!("/api/scenarios/{scenario_id}/events"),
        json!({
            "name": "Sell the car",
            "fires_once": true,
            "trigger": {"kind": "Age", "years": 45},
            "effects": [{
                "kind": "Income", "to_account_id": checking, "income_type": "TaxFree",
                "amount": {"kind": "Fixed", "value": 8_000.0}
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

    // A tag marks the year an event *started*, so the monthly salary tags
    // only its first year, not all ten. A tag on every year would mark nothing.
    // Two events starting in one year are both named, in the order they fired.
    let tagged: Vec<(i64, Vec<&str>)> = years
        .iter()
        .filter_map(|y| {
            let tags: Vec<&str> = y["tags"]
                .as_array()?
                .iter()
                .filter_map(|t| t.as_str())
                .collect();
            (!tags.is_empty()).then(|| (y["year"].as_i64().unwrap(), tags))
        })
        .collect();
    assert_eq!(
        tagged,
        vec![
            (2026, vec!["Salary"]),
            (2030, vec!["Buy the boat", "Sell the car"])
        ],
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

// ───────────────────────────── analysis ─────────────────────────────
//
// Sweeps, sensitivity and solves are one route family over an in-memory job
// registry, so these drive it the same way the screen does: read the plan's
// parameters, POST a question, poll, read the answer.

impl TestApp {
    /// Poll a queued analysis until it settles, and report how it settled.
    async fn await_analysis(&self, id: i64) -> String {
        for _ in 0..200 {
            let (_, current) = self.get(&format!("/api/analyses/{id}")).await;
            let status = current["status"].as_str().unwrap_or_default().to_string();
            if matches!(status.as_str(), "succeeded" | "failed" | "canceled") {
                return status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("analysis {id} never finished");
    }

    /// A plan with a retirement age and a monthly expense — one parameter of
    /// each kind, which is what every mode below picks from.
    async fn seed_analysable(&self) -> i64 {
        let (scenario_id, checking, _) = self.seed_scenario().await;
        // Fixed returns: a sweep cell then differs from its neighbour by the
        // parameter alone, so the assertions are about the plan, not sampling.
        let (_, profiles) = self.get("/api/return-profiles").await;
        for profile in profiles.as_array().unwrap() {
            self.patch(
                &format!("/api/return-profiles/{}", profile["id"]),
                json!({"distribution": {"kind": "Fixed", "rate": 0.0}}),
            )
            .await;
        }
        let (status, age) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/parameters"),
                json!({"name":"RetirementAge","value":{"kind":"Age","years":45,"months":0}}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/parameters"),
                json!({"name":"Spending","value":{"kind":"Money","value":1000.0}}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/events"),
                json!({
                    "name": "Retirement spending", "enabled": true,
                    "trigger": {
                        "kind": "Repeating", "interval": "Monthly",
                        "start_condition": {"kind": "AgeParameter", "parameter_id": age["id"]}
                    },
                    "effects": [{"kind": "Expense", "from_account_id": checking,
                        "amount": {"kind": "Expression", "source": "$Spending"}}]
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        scenario_id
    }
}

#[tokio::test]
async fn analysis_lists_only_explicit_named_parameters() {
    let mut app = TestApp::new().await;
    app.login_as("params@example.com").await;
    let scenario_id = app.seed_analysable().await;

    let (status, params) = app
        .get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let rows = params.as_array().unwrap();

    // The one event offers both of its numbers, and nothing else does.
    let ids: Vec<&str> = rows.iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().all(|id| id.starts_with("parameter:")), "{ids:?}");

    let age = rows.iter().find(|p| p["kind"] == "age").unwrap();
    assert_eq!(age["current"], 45.0);
    assert_eq!(age["name"], "RetirementAge");
    // The suggested range brackets the plan's own value.
    assert!(age["min"].as_f64().unwrap() < 45.0);
    assert!(age["max"].as_f64().unwrap() > 45.0);
}

#[tokio::test]
async fn a_sweep_returns_a_grid_the_client_can_index() {
    let mut app = TestApp::new().await;
    app.login_as("sweep@example.com").await;
    let scenario_id = app.seed_analysable().await;
    let (_, params) = app
        .get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
        .await;
    let age = params.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let amount = params.as_array().unwrap()[1]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({
                "kind": "sweep",
                "iterations": 25,
                "axes": [
                    {"parameter_id": age, "min": 40, "max": 50, "steps": 3},
                    {"parameter_id": amount, "min": 500, "max": 2500, "steps": 4}
                ]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(job["kind"], "sweep");
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");

    let (status, results) = app.get(&format!("/api/analyses/{id}/results")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(results["kind"], "sweep");

    let axes = results["axes"].as_array().unwrap();
    assert_eq!(axes.len(), 2);
    assert_eq!(axes[0]["values"].as_array().unwrap().len(), 3);
    assert_eq!(axes[1]["values"].as_array().unwrap().len(), 4);

    // Row-major over the axes, one cell per combination, indices in range.
    let cells = results["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 12);
    for (position, cell) in cells.iter().enumerate() {
        let indices = cell["indices"].as_array().unwrap();
        assert_eq!(
            indices[0].as_u64().unwrap() * 4 + indices[1].as_u64().unwrap(),
            position as u64
        );
        let rate = cell["success_rate"].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&rate), "{rate}");
    }

    // The plan sits at age 45 and $1,000, which is on both axes.
    assert_eq!(results["plan_indices"], json!([1, 1]));
    assert_eq!(results["iterations"], 25);
}

#[tokio::test]
async fn the_newest_sweep_is_kept_so_a_reload_finds_it() {
    let mut app = TestApp::new().await;
    app.login_as("sweep-cache@example.com").await;
    let scenario_id = app.seed_analysable().await;
    let cached = format!("/api/scenarios/{scenario_id}/analyses/sweep");

    // Nothing swept yet is an ordinary answer, not a 404.
    let (status, empty) = app.get(&cached).await;
    assert_eq!(status, StatusCode::OK);
    assert!(empty.is_null(), "{empty}");

    let (_, params) = app
        .get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
        .await;
    let age = params.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let amount = params.as_array().unwrap()[1]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (_, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({"kind": "sweep", "iterations": 25, "axes": [
                {"parameter_id": age, "min": 40, "max": 50, "steps": 3}
            ]}),
        )
        .await;
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");

    // The grid the job returned, readable without the job's id — which is what
    // a reloaded page has lost.
    let (_, live) = app.get(&format!("/api/analyses/{id}/results")).await;
    let (status, restored) = app.get(&cached).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["scenario_id"].as_i64().unwrap(), scenario_id);
    assert!(!restored["created_at"].as_str().unwrap().is_empty());
    assert_eq!(restored["results"]["axes"], live["axes"]);
    assert_eq!(restored["results"]["cells"], live["cells"]);
    assert_eq!(restored["results"]["iterations"], 25);

    // A second sweep replaces it rather than joining it.
    let (_, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({"kind": "sweep", "iterations": 25, "axes": [
                {"parameter_id": amount, "min": 500, "max": 2500, "steps": 4}
            ]}),
        )
        .await;
    assert_eq!(
        app.await_analysis(job["id"].as_i64().unwrap()).await,
        "succeeded"
    );

    let (_, restored) = app.get(&cached).await;
    let axes = restored["results"]["axes"].as_array().unwrap();
    assert_eq!(axes.len(), 1);
    assert_eq!(axes[0]["parameter_id"].as_str().unwrap(), amount);
    assert_eq!(restored["results"]["cells"].as_array().unwrap().len(), 4);

    // It belongs to its owner: another account sweeping the same ids sees none.
    app.login_as("sweep-cache-other@example.com").await;
    let (status, _) = app.get(&cached).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "another user's scenario");
}

#[tokio::test]
async fn the_graphs_arranged_over_a_sweep_are_kept_with_it() {
    let mut app = TestApp::new().await;
    app.login_as("sweep-layout@example.com").await;
    let scenario_id = app.seed_analysable().await;
    let cached = format!("/api/scenarios/{scenario_id}/analyses/sweep");
    let layout = format!("{cached}/layout");

    let (_, params) = app
        .get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
        .await;
    let age = params.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (_, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({"kind": "sweep", "iterations": 25, "axes": [
                {"parameter_id": age, "min": 40, "max": 50, "steps": 3}
            ]}),
        )
        .await;
    assert_eq!(
        app.await_analysis(job["id"].as_i64().unwrap()).await,
        "succeeded"
    );

    // A fresh sweep carries no arrangement: the client opens on its defaults.
    let (_, restored) = app.get(&cached).await;
    assert!(restored["layout"].is_null(), "{restored}");

    // The graphs are the client's own shapes, stored as sent.
    let graphs = json!([
        {"id": "g1", "kind": "line", "metric": "success", "x": age, "held": {}, "wide": true},
        {"id": "g2", "kind": "surface", "metric": "p50", "x": age, "y": null,
         "held": {}, "wide": false, "azimuth": 62, "elevation": 24}
    ]);
    let (status, _) = app.put(&layout, graphs.clone()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, restored) = app.get(&cached).await;
    assert_eq!(
        restored["layout"], graphs,
        "the layout must come back as it went in"
    );

    // Editing it replaces rather than appends, and re-running the sweep leaves
    // the arrangement alone — it is reconciled onto the new axes, not lost.
    let edited = json!([
        {"id": "g1", "kind": "heatmap", "metric": "funding", "x": age, "held": {}, "wide": false}
    ]);
    let (status, _) = app.put(&layout, edited.clone()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({"kind": "sweep", "iterations": 25, "axes": [
                {"parameter_id": age, "min": 42, "max": 48, "steps": 4}
            ]}),
        )
        .await;
    assert_eq!(
        app.await_analysis(job["id"].as_i64().unwrap()).await,
        "succeeded"
    );

    let (_, restored) = app.get(&cached).await;
    assert_eq!(restored["layout"], edited);
    assert_eq!(
        restored["results"]["axes"][0]["values"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    // Only an array is a layout, and only the owner may store one.
    let (status, _) = app.put(&layout, json!({"graphs": []})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    app.login_as("sweep-layout-other@example.com").await;
    let (status, _) = app.put(&layout, json!([])).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_sweep_carries_more_variables_than_any_one_graph_draws() {
    let mut app = TestApp::new().await;
    app.login_as("workspace@example.com").await;
    let scenario_id = app.seed_analysable().await;
    let (status, _) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/parameters"),
            json!({"name":"TravelBudget","value":{"kind":"Money","value":400.0}}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, accounts) = app
        .get(&format!("/api/scenarios/{scenario_id}/accounts"))
        .await;
    let checking = accounts.as_array().unwrap()[0]["id"].as_i64().unwrap();
    let (status, _) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/events"),
            json!({
                "name": "Travel", "enabled": true,
                "trigger": {
                    "kind": "Repeating", "interval": "Monthly",
                    "start_condition": {"kind": "Age", "years": 50}
                },
                "effects": [{"kind": "Expense", "from_account_id": checking,
                    "amount": {"kind": "Expression", "source": "$TravelBudget"}}]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, params) = app
        .get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
        .await;
    let ids: Vec<String> = params
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    assert!(
        ids.len() >= 3,
        "expected at least three parameters, got {ids:?}"
    );

    let base = format!("/api/scenarios/{scenario_id}/analyses");
    let (status, job) = app
        .post(
            &base,
            json!({
                "kind": "sweep",
                "iterations": 25,
                "axes": [
                    {"parameter_id": ids[0], "steps": 3},
                    {"parameter_id": ids[1], "steps": 3},
                    {"parameter_id": ids[2], "steps": 2}
                ]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");

    let (_, results) = app.get(&format!("/api/analyses/{id}/results")).await;
    let axes = results["axes"].as_array().unwrap();
    assert_eq!(axes.len(), 3);

    // Row-major over all three, so a client can index the grid by strides.
    let cells = results["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 3 * 3 * 2);
    for (position, cell) in cells.iter().enumerate() {
        let at = cell["indices"].as_array().unwrap();
        let flat =
            at[0].as_u64().unwrap() * 6 + at[1].as_u64().unwrap() * 2 + at[2].as_u64().unwrap();
        assert_eq!(flat, position as u64);
    }
    // One plan index per swept variable, or none at all.
    if let Some(plan) = results["plan_indices"].as_array() {
        assert_eq!(plan.len(), 3);
    }

    // The ceiling is the product, not the count: three axes at full resolution
    // is more combinations than a sweep will evaluate.
    let (status, body) = app
        .post(
            &base,
            json!({
                "kind": "sweep",
                "axes": [
                    {"parameter_id": ids[0], "steps": 12},
                    {"parameter_id": ids[1], "steps": 12},
                    {"parameter_id": ids[2], "steps": 12}
                ]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("combinations"),
        "{body}"
    );
}

#[tokio::test]
async fn spending_more_never_raises_the_success_rate() {
    let mut app = TestApp::new().await;
    app.login_as("monotone@example.com").await;
    let scenario_id = app.seed_analysable().await;
    let (_, params) = app
        .get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
        .await;
    let amount = params
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["kind"] == "amount")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (_, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({
                "kind": "sweep",
                "iterations": 25,
                "axes": [{"parameter_id": amount, "min": 200, "max": 6000, "steps": 6}]
            }),
        )
        .await;
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");
    let (_, results) = app.get(&format!("/api/analyses/{id}/results")).await;

    let rates: Vec<f64> = results["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["success_rate"].as_f64().unwrap())
        .collect();
    assert_eq!(rates.len(), 6);
    for pair in rates.windows(2) {
        assert!(pair[1] <= pair[0], "success rose with spending: {rates:?}");
    }
    assert!(rates[0] > rates[5], "the sweep moved nothing: {rates:?}");
}

#[tokio::test]
async fn a_solve_answers_with_the_boundary_and_shows_its_working() {
    let mut app = TestApp::new().await;
    app.login_as("solve@example.com").await;
    let scenario_id = app.seed_analysable().await;
    let (_, params) = app
        .get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
        .await;
    let amount = params
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["kind"] == "amount")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({
                "kind": "solve",
                "constraint": "success-rate",
                "iterations": 25,
                "objective": "max-parameter",
                "min_value": 0.95,
                "vary": [{"parameter_id": amount, "min": 200, "max": 6000}]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");

    let (_, results) = app.get(&format!("/api/analyses/{id}/results")).await;
    assert_eq!(results["kind"], "solve");
    // One parameter and an objective that is that parameter: bisection.
    assert_eq!(results["method"], "bisection");

    let best = &results["best"];
    assert!(!best.is_null(), "no answer found: {results}");
    assert!(best["feasible"].as_bool().unwrap());
    assert!(best["success_rate"].as_f64().unwrap() >= 0.95);

    // Every probe is recorded, and the answer is the best feasible one.
    let steps = results["steps"].as_array().unwrap();
    assert!(steps.len() >= 3, "expected a bracket: {steps:?}");
    let answer = best["values"][0].as_f64().unwrap();
    for step in steps.iter().filter(|s| s["feasible"] == json!(true)) {
        assert!(step["values"][0].as_f64().unwrap() <= answer + 1e-9);
    }
    // Brackets narrow rather than wander.
    let widths: Vec<f64> = steps
        .iter()
        .filter_map(|s| Some(s["bracket_high"].as_f64()? - s["bracket_low"].as_f64()?))
        .collect();
    for pair in widths.windows(2) {
        assert!(pair[1] <= pair[0] + 1e-9, "bracket widened: {widths:?}");
    }
    assert_eq!(results["constraint"], "success-rate");
    // The same plan can end above zero despite event warnings. Omitting a
    // constraint now uses funding, which must not borrow terminal success.
    let (status, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({"kind": "solve", "iterations": 25, "objective": "max-parameter",
            "min_value": 0.95, "vary": [{"parameter_id": amount, "min": 200, "max": 6000}]}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");
    let (_, funding) = app.get(&format!("/api/analyses/{id}/results")).await;
    assert_eq!(funding["constraint"], "funding-success-rate");
    assert!(funding["best"].is_null());
    assert!(funding["std_error"].is_null());
    assert!(
        funding["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["success_rate"] == 1.0 && step["funding_success_rate"] == 0.0)
    );
}

#[tokio::test]
async fn a_sensitivity_ranking_puts_the_biggest_mover_first() {
    let mut app = TestApp::new().await;
    app.login_as("sensitivity@example.com").await;
    let scenario_id = app.seed_analysable().await;

    let (status, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({"kind": "sensitivity", "iterations": 25, "fraction": 0.5}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");

    let (_, results) = app.get(&format!("/api/analyses/{id}/results")).await;
    assert_eq!(results["kind"], "sensitivity");
    let rows = results["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    let spans: Vec<f64> = rows.iter().map(|r| r["span"].as_f64().unwrap()).collect();
    for pair in spans.windows(2) {
        assert!(pair[1] <= pair[0], "ranking is out of order: {spans:?}");
    }
    for row in rows {
        assert!(row["low_value"].as_f64().unwrap() < row["high_value"].as_f64().unwrap());
    }
}

#[tokio::test]
async fn an_analysis_refuses_a_question_it_cannot_answer() {
    let mut app = TestApp::new().await;
    app.login_as("refuse@example.com").await;
    let scenario_id = app.seed_analysable().await;
    let base = format!("/api/scenarios/{scenario_id}/analyses");
    let (_, params) = app
        .get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
        .await;
    let amount = params
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["kind"] == "amount")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // A parameter this plan does not have.
    let (status, _) = app
        .post(
            &base,
            json!({"kind": "sweep", "axes": [{"parameter_id": "event:999:amount"}]}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The same parameter on both axes would be a line, not a grid.
    let (status, _) = app
        .post(
            &base,
            json!({"kind": "sweep", "axes": [
                {"parameter_id": amount}, {"parameter_id": amount}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // An inverted range, and a step count past the ceiling.
    for axis in [
        json!({"parameter_id": amount, "min": 5000, "max": 1000}),
        json!({"parameter_id": amount, "steps": 50}),
    ] {
        let (status, _) = app
            .post(&base, json!({"kind": "sweep", "axes": [axis]}))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // Iterations outside what an analysis will spend.
    let (status, _) = app
        .post(
            &base,
            json!({"kind": "sweep", "iterations": 100_000,
                   "axes": [{"parameter_id": amount}]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A constraint that is not a fraction.
    let (status, _) = app
        .post(
            &base,
            json!({"kind": "solve", "objective": "max-parameter", "min_value": 95.0,
                   "vary": [{"parameter_id": amount}]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_analysis_belongs_to_the_user_who_started_it() {
    let mut app = TestApp::new().await;
    app.login_as("owner@example.com").await;
    let scenario_id = app.seed_analysable().await;
    let (_, params) = app
        .get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
        .await;
    let amount = params.as_array().unwrap()[1]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({"kind": "sweep", "iterations": 25,
                   "axes": [{"parameter_id": amount, "steps": 2}]}),
        )
        .await;
    let id = job["id"].as_i64().unwrap();
    assert_eq!(app.await_analysis(id).await, "succeeded");

    app.login_as("intruder@example.com").await;
    for path in [
        format!("/api/analyses/{id}"),
        format!("/api/analyses/{id}/results"),
    ] {
        assert_eq!(app.get(&path).await.0, StatusCode::NOT_FOUND);
    }
    assert_eq!(
        app.post(&format!("/api/analyses/{id}/cancel"), json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // The scenario's own parameters are equally out of reach.
    assert_eq!(
        app.get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn results_are_only_available_once_the_analysis_has_finished() {
    let mut app = TestApp::new().await;
    app.login_as("pending@example.com").await;
    let scenario_id = app.seed_analysable().await;
    let (_, params) = app
        .get(&format!("/api/scenarios/{scenario_id}/analysis/parameters"))
        .await;
    let amount = params.as_array().unwrap()[1]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (_, job) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/analyses"),
            json!({"kind": "sweep", "iterations": 2000,
                   "axes": [{"parameter_id": amount, "steps": 12}]}),
        )
        .await;
    let id = job["id"].as_i64().unwrap();

    // Reading results before it finishes is a refusal, not an empty grid.
    let (status, _) = app.get(&format!("/api/analyses/{id}/results")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, canceled) = app
        .post(&format!("/api/analyses/{id}/cancel"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(matches!(
        canceled["status"].as_str().unwrap(),
        "queued" | "running" | "canceled"
    ));
    assert_eq!(app.await_analysis(id).await, "canceled");
    let (status, _) = app.get(&format!("/api/analyses/{id}/results")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_plan_whose_schedule_follows_the_market_runs_and_analyses() {
    // An event triggered off net worth fires on a date that follows the market,
    // which used to move the year's wealth snapshot with it and leave the
    // iterations of one run disagreeing about what dates they had measured.
    // Both halves are checked here: the run that reads the real envelope, and
    // the analyses that do not.
    let mut app = TestApp::new().await;
    app.login_as("market-schedule@example.com").await;
    let (scenario_id, checking, brokerage) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario_id}");
    let (status, _) = app
        .post(
            &format!("{base}/parameters"),
            json!({"name":"Spending","value":{"kind":"Money","value":1000.0}}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = app
        .post(
            &format!("{base}/events"),
            json!({
                "name": "Retirement spending", "enabled": true,
                "trigger": {
                    "kind": "Repeating", "interval": "Monthly",
                    "start_condition": {"kind": "Age", "years": 45}
                },
                "effects": [{"kind": "Expense", "from_account_id": checking,
                    "amount": {"kind": "Expression", "source": "$Spending"}}]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = app
        .post(
            &format!("{base}/events"),
            json!({
                "name": "Top up when rich", "enabled": true,
                "trigger": {
                    "kind": "Repeating", "interval": "Monthly",
                    "start_condition": {
                        "kind": "NetWorth",
                        "comparison": "GreaterThanOrEqual", "threshold": 61_000.0
                    }
                },
                "effects": [{"kind": "CashTransfer", "from_account_id": brokerage,
                    "to_account_id": checking, "amount": {"kind": "Fixed", "value": 500.0}}]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, run) = app
        .post(&format!("{base}/runs"), json!({"iterations": 64}))
        .await;
    let run_id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");

    // The run measured a real envelope, which is the part that needs every
    // iteration to agree on its dates.
    let (status, results) = app.get(&format!("/api/runs/{run_id}/results")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !results["real_net_worth"].is_null(),
        "the envelope is what the shared date grid is for"
    );

    // Every analysis finishes too, on the same plan.
    let (_, params) = app.get(&format!("{base}/analysis/parameters")).await;
    let amount = params
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["kind"] == "amount")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    for body in [
        json!({"kind": "sensitivity", "iterations": 25}),
        json!({"kind": "sweep", "iterations": 25,
               "axes": [{"parameter_id": amount, "steps": 3}]}),
        json!({"kind": "solve", "iterations": 25, "objective": "max-parameter",
               "min_value": 0.5, "vary": [{"parameter_id": amount}]}),
    ] {
        let kind = body["kind"].as_str().unwrap().to_string();
        let (status, job) = app.post(&format!("{base}/analyses"), body).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{kind}");
        let id = job["id"].as_i64().unwrap();
        assert_eq!(app.await_analysis(id).await, "succeeded", "{kind}");
        let (status, results) = app.get(&format!("/api/analyses/{id}/results")).await;
        assert_eq!(status, StatusCode::OK, "{kind}");
        assert_eq!(results["kind"], json!(kind));
    }
}

#[tokio::test]
async fn shared_return_edit_invalidates_dependents_only_and_failed_save_does_not() {
    let mut app = TestApp::new().await;
    app.login_as("freshness@example.com").await;
    let (sid, _, _) = app.seed_scenario().await;
    let (_, other) = app
        .post(
            "/api/scenarios",
            json!({"name":"Untouched", "start_date":"2026-01-01", "duration_years":10}),
        )
        .await;
    let other_id = other["id"].as_i64().unwrap();
    let (_, profiles) = app.get("/api/return-profiles").await;
    let profile = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "US Total Market")
        .unwrap();
    let pid = profile["id"].as_i64().unwrap();
    let db = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        app._dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE scenarios SET updated_at = '2000-01-01 00:00:00'")
        .execute(&db)
        .await
        .unwrap();

    let (status, _) = app
        .patch(
            &format!("/api/return-profiles/{pid}"),
            json!({"distribution":{"kind":"Fixed","rate":0.04}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, changed) = app.get(&format!("/api/scenarios/{sid}")).await;
    let (_, unchanged) = app.get(&format!("/api/scenarios/{other_id}")).await;
    assert_ne!(changed["updated_at"], "2000-01-01 00:00:00");
    assert_eq!(unchanged["updated_at"], "2000-01-01 00:00:00");

    sqlx::query("UPDATE scenarios SET updated_at = '2000-01-01 00:00:00'")
        .execute(&db)
        .await
        .unwrap();
    let (status, _) = app
        .patch(
            &format!("/api/return-profiles/{pid}"),
            json!({"name":"Savings Account"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, unchanged) = app.get(&format!("/api/scenarios/{sid}")).await;
    assert_eq!(unchanged["updated_at"], "2000-01-01 00:00:00");
}

#[tokio::test]
async fn shared_tax_edits_and_inflation_deletion_invalidate_dependents() {
    let mut app = TestApp::new().await;
    app.login_as("assumption-freshness@example.com").await;
    let (_, taxes) = app.get("/api/tax-configs").await;
    let tax = taxes[0]["id"].as_i64().unwrap();
    let (_, inflation) = app.get("/api/inflation-profiles").await;
    let inflation_id = inflation[0]["id"].as_i64().unwrap();
    let (_, scenario) = app.post("/api/scenarios", json!({"name":"Assumptions", "start_date":"2026-01-01", "duration_years":10, "tax_config_id":tax, "inflation_profile_id":inflation_id})).await;
    let sid = scenario["id"].as_i64().unwrap();
    let db = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        app._dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    for (method, path, body) in [
        (
            "PATCH",
            format!("/api/tax-configs/{tax}"),
            Some(json!({"state_rate":0.05})),
        ),
        (
            "DELETE",
            format!("/api/inflation-profiles/{inflation_id}"),
            None,
        ),
        ("DELETE", format!("/api/tax-configs/{tax}"), None),
    ] {
        sqlx::query("UPDATE scenarios SET updated_at = '2000-01-01 00:00:00'")
            .execute(&db)
            .await
            .unwrap();
        let (status, _) = app.send(method, &path, body).await;
        assert!(status.is_success(), "{method} {path}: {status}");
        let (_, changed) = app.get(&format!("/api/scenarios/{sid}")).await;
        assert_ne!(changed["updated_at"], "2000-01-01 00:00:00");
    }
}

#[path = "cases/history.rs"]
mod history_cases;

#[path = "cases/onboarding.rs"]
mod onboarding_cases;

#[path = "cases/runs.rs"]
mod runs_cases;

#[path = "cases/draft_preview.rs"]
mod draft_preview_cases;

#[path = "cases/documents.rs"]
mod documents_cases;

#[path = "cases/draft_agent.rs"]
mod draft_agent_cases;

#[path = "cases/archives.rs"]
mod archives_cases;

#[path = "cases/parameters.rs"]
mod parameter_cases;

#[path = "cases/real_estate.rs"]
mod real_estate_cases;

#[path = "cases/results_projection.rs"]
mod results_projection_cases;

#[path = "cases/plan_reads.rs"]
mod plan_reads_cases;

#[path = "cases/what_if.rs"]
mod what_if_cases;

mod named_analysis_cases {
    use super::*;
    include!("cases/named_analysis.rs");
}

#[tokio::test]
async fn scenario_slugs_are_unique_and_survive_renaming() {
    let mut app = TestApp::new().await;
    app.login_as("slugs@example.com").await;
    let (status, created) = app
        .post(
            "/api/scenarios",
            json!({
                "name": "Slug plan", "start_date": "2026-01-01"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let slug = created["slug"].as_str().unwrap();
    assert_eq!(slug.len(), 13);
    assert!(slug.starts_with('s'));
    assert!(slug.chars().all(|c| c.is_ascii_alphanumeric()));
    let base = format!("/api/scenarios/{}", created["id"]);
    let (status, renamed) = app
        .send("PATCH", &base, Some(json!({"name": "Renamed"})))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["slug"], slug);
    let (status, duplicate) = app
        .post(&format!("{base}/duplicate"), json!({"name": "Copy"}))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(duplicate["slug"], slug);
    assert_eq!(app.get(&base).await.1["slug"], slug);
    let (_, listed) = app.get("/api/scenarios").await;
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["slug"] == slug)
    );
}

#[tokio::test]
async fn baseline_assigns_unique_scenario_slugs() {
    let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    finplan_server::db::MIGRATOR.run(&db).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id, email, password_hash) VALUES ('owner', 'owner@example.com', 'unused');
         INSERT INTO scenarios(id, user_id, name, start_date)
         VALUES (1, 'owner', 'Existing', '2026-01-01'), (2, 'owner', 'Another', '2026-01-01');",
    )
    .execute(&db)
    .await
    .unwrap();
    let before: Vec<String> = sqlx::query_scalar("SELECT slug FROM scenarios ORDER BY id")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(before.len(), 2);
    assert_ne!(before[0], before[1]);
    assert!(
        before
            .iter()
            .all(|slug| slug.len() == 13 && slug.starts_with('s'))
    );
    sqlx::query("INSERT INTO scenarios(user_id, name, start_date) VALUES ('owner', 'New', '2026-01-01'), ('owner', 'Imported', '2026-01-01')")
        .execute(&db)
        .await
        .unwrap();
    let after: Vec<String> = sqlx::query_scalar("SELECT slug FROM scenarios ORDER BY id")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(&after[..2], &before);
    assert!(after.iter().all(|slug| slug.len() == 13));
    assert!(
        sqlx::query("INSERT INTO scenarios(user_id, name, start_date, slug) VALUES ('owner', 'Collision', '2026-01-01', ?)")
            .bind(&before[0])
            .execute(&db)
            .await
            .is_err()
    );
}

// ── preview ─────────────────────────────────────────────────────────────────

impl TestApp {
    /// A seeded plan whose checking account runs dry in every iteration, and a
    /// finished run of it. Returns (scenario, run, spending event, brokerage).
    async fn previewable_plan(&self) -> (i64, i64, i64, i64) {
        let (scenario_id, checking, brokerage) = self.seed_scenario().await;
        let (status, event) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/events"),
                json!({
                    "name": "Spending",
                    "trigger": {"kind": "Repeating", "interval": "Monthly"},
                    "effects": [{
                        "kind": "Expense", "from_account_id": checking,
                        "amount": {"kind": "Fixed", "value": 1000.0}
                    }]
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{event}");
        let (status, run) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/runs"),
                json!({"iterations": 40, "seed": 7}),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{run}");
        let run_id = run["id"].as_i64().unwrap();
        assert_eq!(self.await_run(run_id).await, "succeeded");
        (
            scenario_id,
            run_id,
            event["id"].as_i64().unwrap(),
            brokerage,
        )
    }

    async fn preview(&self, scenario_id: i64, body: Value) -> (StatusCode, Value) {
        self.post(&format!("/api/scenarios/{scenario_id}/preview"), body)
            .await
    }
}

#[tokio::test]
async fn a_preview_simulates_the_edit_against_its_run() {
    let mut app = TestApp::new().await;
    app.login_as("preview@example.com").await;
    let (scenario_id, run_id, spending, _) = app.previewable_plan().await;

    let (status, preview) = app
        .preview(
            scenario_id,
            json!({"changes": [
                {"op": "remove", "target": {"event": spending}, "path": "/effects/0"}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["base_run_id"], run_id);
    assert_eq!(preview["iterations"], 40);
    assert_eq!(preview["paired"], true);
    assert_eq!(preview["problems"], json!([]));
    let labels: Vec<&str> = preview["diff"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["label"].as_str().unwrap())
        .collect();
    assert!(
        labels.iter().any(|l| l.contains("Spending")),
        "diff: {labels:?}"
    );

    // Checking empties within a year with spending on; without it, never.
    let base = &preview["base"];
    let edited = &preview["edited"];
    assert!(
        base["funding_success_rate"].as_f64().unwrap() < 0.5,
        "{base}"
    );
    assert_eq!(edited["funding_success_rate"], 1.0, "{edited}");
    assert!(
        base["funding"]["cash_shortfall"].as_i64().unwrap() > 0,
        "{base}"
    );
    assert_eq!(edited["funding"]["cash_shortfall"], 0, "{edited}");
    assert!(
        edited["real_final"]["p50"].as_f64().unwrap() > base["real_final"]["p50"].as_f64().unwrap()
    );

    // Nothing was written: the plan still spends, and no run was added.
    let (_, event) = app
        .get(&format!("/api/scenarios/{scenario_id}/events/{spending}"))
        .await;
    assert_eq!(event["effects"].as_array().unwrap().len(), 1);
    let (_, runs) = app.get(&format!("/api/scenarios/{scenario_id}/runs")).await;
    assert_eq!(runs.as_array().unwrap().len(), 1, "{runs}");
}

#[tokio::test]
async fn a_preview_can_create_an_asset_an_account_holding_it_and_an_event_funding_it() {
    let mut app = TestApp::new().await;
    app.login_as("create-preview@example.com").await;
    let (scenario_id, _, spending, _) = app.previewable_plan().await;
    let (_, event) = app
        .get(&format!("/api/scenarios/{scenario_id}/events/{spending}"))
        .await;
    let checking = event["effects"][0]["from_account_id"].as_i64().unwrap();
    let (_, assets) = app
        .get(&format!("/api/scenarios/{scenario_id}/assets"))
        .await;
    let equity = assets[0]["return_profile_id"].as_i64().unwrap();
    let (_, accounts) = app
        .get(&format!("/api/scenarios/{scenario_id}/accounts"))
        .await;
    let cash = accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["flavor"] == "Bank")
        .unwrap()["return_profile_id"]
        .as_i64()
        .unwrap();

    let (status, preview) = app
        .preview(
            scenario_id,
            json!({"changes": [
                {"op": "add", "target": {"new_asset": "etf"}, "path": "", "value": {
                    "name": "Total World ETF", "initial_price": 120.0,
                    "return_profile_id": equity}},
                {"op": "add", "target": {"new_account": "ira"}, "path": "", "value": {
                    "name": "New IRA", "flavor": "Investment", "tax_status": "TaxFree",
                    "cash_value": 500.0, "cash_return_profile_id": cash,
                    "positions": [{"asset_id": {"$new": "etf"}, "units": 50.0,
                                   "cost_basis": 6000.0}]}},
                {"op": "add", "target": {"new_event": "fund"}, "path": "", "value": {
                    "name": "Fund the IRA", "fires_once": true,
                    "trigger": {"kind": "Date", "on_date": "2027-01-01"},
                    "effects": [{"kind": "CashTransfer", "from_account_id": checking,
                                 "to_account_id": {"$new": "ira"},
                                 "amount": {"kind": "Fixed", "value": 1000.0}}]}}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["problems"], json!([]), "{preview}");
    // New rows on profiles the run already sampled draw the same markets.
    assert_eq!(preview["paired"], true);
    let lines: Vec<(String, String)> = preview["diff"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["label"].as_str().unwrap().to_string(),
                l["to"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let to = |label: &str| {
        lines
            .iter()
            .find(|(l, _)| l == label)
            .unwrap_or_else(|| panic!("no {label} in {lines:?}"))
            .1
            .clone()
    };
    assert!(to("Portfolio › + Total World ETF").starts_with("$120.00 · "));
    assert_eq!(
        to("Portfolio › + New IRA"),
        "Tax-free investment · $500 cash · 1 lot"
    );
    assert!(to("Plan › + Fund the IRA").contains("New IRA"), "{lines:?}");
    assert!(preview["edited"]["success_rate"].is_number(), "{preview}");

    // Nothing was written.
    let (_, after) = app
        .get(&format!("/api/scenarios/{scenario_id}/assets"))
        .await;
    assert_eq!(
        after.as_array().unwrap().len(),
        assets.as_array().unwrap().len()
    );

    // A reference to nothing is a problem with the batch, not a failure.
    let (status, preview) = app
        .preview(
            scenario_id,
            json!({"changes": [
                {"op": "replace", "target": {"event": spending},
                 "path": "/effects/0/from_account_id", "value": {"$new": "nowhere"}}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(
        preview["problems"][0]["kind"], "unknown_reference",
        "{preview}"
    );
    assert!(preview["edited"].is_null());
}

#[tokio::test]
async fn an_unchanged_preview_reproduces_its_run() {
    let mut app = TestApp::new().await;
    app.login_as("noop@example.com").await;
    let (scenario_id, _, spending, _) = app.previewable_plan().await;

    // Read back from the stored run on one side, simulated on the other: the
    // same markets from the same seed give the same answer, to the bit.
    // A change that rewrites a value to itself resolves to no write at all.
    for changes in [
        json!([]),
        json!([{"op": "replace", "target": {"event": spending},
                "path": "/effects/0/amount/value", "expect": 1000.0, "value": 1000.0}]),
    ] {
        let (status, preview) = app.preview(scenario_id, json!({"changes": changes})).await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        assert_eq!(preview["paired"], true);
        assert_eq!(preview["diff"], json!([]));
        assert_eq!(preview["base"], preview["edited"], "{preview}");
    }

    // A different sample size simulates the base too, and still agrees.
    let (status, preview) = app
        .preview(scenario_id, json!({"changes": [], "iterations": 25}))
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["iterations"], 25);
    assert_eq!(preview["base"], preview["edited"], "{preview}");
    assert_ne!(preview["base"]["funding"]["iterations"], 40);
}

#[tokio::test]
async fn a_stale_or_broken_change_is_reported_without_simulating() {
    let mut app = TestApp::new().await;
    app.login_as("stale@example.com").await;
    let (scenario_id, _, spending, brokerage) = app.previewable_plan().await;

    let (status, preview) = app
        .preview(
            scenario_id,
            json!({"changes": [
                {"op": "replace", "target": {"event": spending},
                 "path": "/effects/0/amount/value", "expect": 999.0, "value": 500.0},
                {"op": "replace", "target": {"account": brokerage},
                 "path": "/no_such_field", "value": 1}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let kinds: Vec<&str> = preview["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"stale"), "{preview}");
    assert_eq!(preview["problems"][0]["actual"], 1000.0, "{preview}");
    assert_eq!(kinds.len(), 2, "{preview}");
    assert_eq!(preview["base"], Value::Null);
    assert_eq!(preview["edited"], Value::Null);

    // An edit the plan refuses — a lot with a negative basis — is a problem
    // with the batch, not a server error.
    let (status, preview) = app
        .preview(
            scenario_id,
            json!({"changes": [
                {"op": "replace", "target": {"account": brokerage},
                 "path": "/positions/0/cost_basis", "value": -1.0}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["problems"][0]["kind"], "invalid_body", "{preview}");
    assert_eq!(preview["edited"], Value::Null);
}

#[tokio::test]
async fn a_preview_can_edit_a_lot_or_map_an_asset_onto_a_profile_the_run_never_used() {
    let mut app = TestApp::new().await;
    app.login_as("remap@example.com").await;
    let (scenario_id, _, _, brokerage) = app.previewable_plan().await;

    // A cost-basis edit leaves the markets alone.
    let (status, preview) = app
        .preview(
            scenario_id,
            json!({"changes": [
                {"op": "replace", "target": {"account": brokerage},
                 "path": "/positions/0/cost_basis", "expect": 40000.0, "value": 50000.0}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["problems"], json!([]), "{preview}");
    assert_eq!(preview["paired"], true);
    assert_eq!(preview["diff"].as_array().unwrap().len(), 1, "{preview}");

    // The run's snapshot only holds the profiles it used; bonds were not one.
    let (_, profiles) = app.get("/api/return-profiles").await;
    let bonds = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "US Aggregate Bonds")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let (_, assets) = app
        .get(&format!("/api/scenarios/{scenario_id}/assets"))
        .await;
    let vti = assets[0]["id"].as_i64().unwrap();
    let (status, preview) = app
        .preview(
            scenario_id,
            json!({"changes": [
                {"op": "replace", "target": {"asset": vti}, "path": "/return_profile_id",
                 "value": bonds}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["problems"], json!([]), "{preview}");
    assert_eq!(preview["paired"], false, "a new profile draws new markets");
    assert_eq!(preview["diff"][0]["to"], "US Aggregate Bonds", "{preview}");
    assert!(preview["edited"]["real_final"].is_object(), "{preview}");
}

#[tokio::test]
async fn a_preview_needs_a_finished_run_of_the_callers_own_plan() {
    let mut owner = TestApp::new().await;
    owner.login_as("preview-owner@example.com").await;
    let (unrun, _, _) = owner.seed_scenario().await;
    owner
        .patch(
            &format!("/api/scenarios/{unrun}"),
            json!({"name": "Never run"}),
        )
        .await;
    let (status, body) = owner.preview(unrun, json!({"changes": []})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (scenario_id, run_id, _, _) = owner.previewable_plan().await;
    // A run of another scenario is not this one's to preview against.
    let (status, body) = owner
        .preview(unrun, json!({"changes": [], "base_run_id": run_id}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let mut intruder = TestApp {
        router: owner.router.clone(),
        cookie: None,
        _dir: tempfile::tempdir().unwrap(),
    };
    intruder.login_as("preview-intruder@example.com").await;
    let (status, _) = intruder.preview(scenario_id, json!({"changes": []})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_preview_caps_its_sample() {
    let mut app = TestApp::new().await;
    app.login_as("cap@example.com").await;
    // No events: the cheapest plan there is, so the cap costs little to hit.
    let (scenario_id, _, _) = app.seed_scenario().await;
    let (_, run) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/runs"),
            json!({"iterations": 10, "seed": 3}),
        )
        .await;
    assert_eq!(
        app.await_run(run["id"].as_i64().unwrap()).await,
        "succeeded"
    );

    let (status, preview) = app
        .preview(scenario_id, json!({"changes": [], "iterations": 1_000_000}))
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["iterations"], 5000);
    assert_eq!(preview["edited"]["funding"]["iterations"], 5000);
    assert_eq!(preview["base"], preview["edited"]);

    let (status, _) = app
        .preview(scenario_id, json!({"changes": [], "iterations": 0}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ── suggestions ─────────────────────────────────────────────────────────────

impl TestApp {
    /// A seeded plan with a loan paid by an inflation-adjusted transfer (which
    /// the liability rule flags), and a finished run. Returns (scenario, run,
    /// payment event).
    async fn reviewable_plan(&self) -> (i64, i64, i64) {
        let (scenario_id, checking, _) = self.seed_scenario().await;
        let (status, loan) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/accounts"),
                json!({"name": "Car loan", "flavor": "Liability",
                       "principal": 5000.0, "interest_rate": 0.05}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{loan}");
        let (status, event) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/events"),
                json!({
                    "name": "Loan payment",
                    "trigger": {"kind": "Repeating", "interval": "Monthly"},
                    "effects": [{
                        "kind": "CashTransfer", "from_account_id": checking,
                        "to_account_id": loan["id"],
                        "amount": {"kind": "InflationAdjusted",
                                   "inner": {"kind": "Fixed", "value": 200.0}}
                    }]
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{event}");
        let (_, run) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/runs"),
                json!({"iterations": 40, "seed": 7}),
            )
            .await;
        let run_id = run["id"].as_i64().unwrap();
        assert_eq!(self.await_run(run_id).await, "succeeded");
        (scenario_id, run_id, event["id"].as_i64().unwrap())
    }

    async fn review(&self, scenario_id: i64) -> Value {
        let (status, review) = self
            .post(
                &format!("/api/scenarios/{scenario_id}/review"),
                json!({"run_id": null}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{review}");
        review
    }

    /// A model-written suggestion that drops the plan's spending.
    async fn suggest(&self, scenario_id: i64, body: Value) -> (StatusCode, Value) {
        self.post(&format!("/api/scenarios/{scenario_id}/suggestions"), body)
            .await
    }
}

fn by_rule<'a>(review: &'a Value, rule: &str) -> Vec<&'a Value> {
    review["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["rule"] == rule)
        .collect()
}

fn drop_spending(spending: i64) -> Value {
    json!({
        "run_id": null, "kind": "fix", "section": "plan",
        "title": "Spending empties checking within the year",
        "summary": "The one-line lead.", "reasoning": "Checking starts at $10,000 and pays $1,000 a month with nothing coming in.",
        "evidence": [{"ref": "ledger", "year": 2026, "event_id": spending, "account_id": null}],
        "paths": [{
            "key": "a", "label": "Stop the spending", "reasoning": null, "recommended": true,
            "estimate": {"success_rate": null, "funding_success_rate": 1.0},
            "steps": [{"key": "a", "title": "Remove its expense", "reasoning": null,
                       "changes": [{"op": "remove", "target": {"event": spending},
                                    "path": "/effects/0"}]}]
        }]
    })
}

/// Apply every remaining step of `path` to the plan.
fn apply_path(path: &str) -> Value {
    json!({"path": path, "through_step": null, "to": "plan", "name": null})
}

#[tokio::test]
async fn a_review_stores_checked_notes_and_remembers_dismissals() {
    let mut app = TestApp::new().await;
    app.login_as("review@example.com").await;
    let (scenario_id, run_id, payment) = app.reviewable_plan().await;

    let (status, none) = app
        .get(&format!("/api/scenarios/{scenario_id}/review"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(none, Value::Null);

    let review = app.review(scenario_id).await;
    assert_eq!(review["run_id"], run_id);
    let notes = by_rule(&review, "liability_payment_inflation_adjusted");
    assert_eq!(notes.len(), 1, "{review}");
    let note = notes[0];
    assert_eq!(note["source"], "rules");
    assert!(
        note["summary"].as_str().is_some_and(|s| !s.is_empty()),
        "{note}"
    );
    assert_eq!(note["kind"], "fix");
    assert_eq!(note["section"], "plan");
    assert_eq!(note["status"], "open");
    assert_eq!(note["applied_path"], Value::Null);
    // Keep today's figure (recommended, checked), or the loan's 30-year
    // amortizing payment.
    let keys: Vec<&str> = note["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["fixed", "amortizing"], "{note}");
    let fixed = &note["paths"][0];
    assert_eq!(fixed["recommended"], true);
    assert_eq!(fixed["steps"][0]["changes"][0]["target"]["event"], payment);
    assert_eq!(fixed["steps"][0]["applied"], false);
    assert!(
        fixed["steps"][0]["diff"][0]["label"]
            .as_str()
            .unwrap()
            .contains("Loan payment"),
        "{note}"
    );
    assert_eq!(note["paths"][1]["check"], Value::Null, "one check per note");
    let check = &fixed["check"];
    assert_eq!(check["iterations"], 40, "{note}");
    assert_eq!(check["paired"], true);
    assert!(check["base"]["success_rate"].is_number());
    assert!(check["edited"]["real_final"].is_object());

    let (_, latest) = app
        .get(&format!("/api/scenarios/{scenario_id}/review"))
        .await;
    assert_eq!(latest["suggestions"], review["suggestions"]);

    // Reviewing again refreshes the open notes rather than adding to them,
    // and a rule note raised again keeps its row.
    let again = app.review(scenario_id).await;
    assert_eq!(
        by_rule(&again, "liability_payment_inflation_adjusted")[0]["id"],
        note["id"]
    );
    assert_eq!(
        again["suggestions"].as_array().unwrap().len(),
        review["suggestions"].as_array().unwrap().len()
    );
    let (_, all) = app
        .get(&format!("/api/scenarios/{scenario_id}/suggestions"))
        .await;
    assert_eq!(
        all.as_array().unwrap().len(),
        review["suggestions"].as_array().unwrap().len()
    );

    // "It's correct" keeps the note out of later reviews.
    let id = by_rule(&again, "liability_payment_inflation_adjusted")[0]["id"]
        .as_i64()
        .unwrap();
    let (status, confirmed) = app
        .post(
            &format!("/api/suggestions/{id}/dismiss"),
            json!({"as": "confirmed"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    assert_eq!(confirmed["status"], "confirmed");
    assert!(confirmed["resolved_at"].is_string());
    let (status, _) = app
        .post(
            &format!("/api/suggestions/{id}/dismiss"),
            json!({"as": "dismissed"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Undo puts it back on the board, open; an open note has nothing to undo.
    let (status, reopened) = app
        .post(&format!("/api/suggestions/{id}/reopen"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened["status"], "open");
    assert_eq!(reopened["resolved_at"], Value::Null);
    let (status, _) = app
        .post(&format!("/api/suggestions/{id}/reopen"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = app
        .post(
            &format!("/api/suggestions/{id}/dismiss"),
            json!({"as": "confirmed"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Not raised again: the review lists only the confirmed note, for undo.
    let third = app.review(scenario_id).await;
    let standing = by_rule(&third, "liability_payment_inflation_adjusted");
    assert_eq!(standing.len(), 1, "{third}");
    assert_eq!(standing[0]["id"], id);
    assert_eq!(standing[0]["status"], "confirmed");
    let (_, confirmed) = app
        .get(&format!(
            "/api/scenarios/{scenario_id}/suggestions?status=confirmed"
        ))
        .await;
    assert_eq!(confirmed.as_array().unwrap().len(), 1);
    let (status, _) = app
        .get(&format!(
            "/api/scenarios/{scenario_id}/suggestions?status=maybe"
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn open_notes_carry_into_the_next_review_until_the_plan_moves_under_them() {
    let mut app = TestApp::new().await;
    app.login_as("carry@example.com").await;
    let (scenario_id, run_id, spending, _) = app.previewable_plan().await;
    let (status, stored) = app.suggest(scenario_id, drop_spending(spending)).await;
    assert_eq!(status, StatusCode::CREATED, "{stored}");
    let id = stored["id"].clone();

    // A new review keeps it, re-walked against the run's plan, once.
    let ids = |review: &Value| -> Vec<Value> {
        review["suggestions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["source"] == "ai")
            .map(|s| s["id"].clone())
            .collect()
    };
    let first = app.review(scenario_id).await;
    assert_eq!(ids(&first), std::slice::from_ref(&id), "{first}");
    let again = app.review(scenario_id).await;
    assert_eq!(ids(&again), std::slice::from_ref(&id), "{again}");
    let note = &again["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap()
        .clone();
    assert_eq!(note["status"], "open");
    assert_eq!(note["run_id"], run_id);
    assert!(
        !note["paths"][0]["steps"][0]["diff"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Rule notes raised again keep their rows (pinned by the liability rule
    // in a_review_stores_checked_notes_and_remembers_dismissals).
    let rule_ids = |review: &Value| -> Vec<Value> {
        review["suggestions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["source"] == "rules")
            .map(|s| s["id"].clone())
            .collect()
    };
    assert_eq!(rule_ids(&first), rule_ids(&again));

    // A note set aside stays listed, as handled, through a review of a
    // newer run.
    let (status, read) = app
        .suggest(
            scenario_id,
            json!({
                "run_id": null, "kind": "read", "section": "results",
                "title": "Checking runs dry in the first year",
                "summary": "The one-line lead.", "reasoning": "Nothing refills it.",
                "evidence": [], "paths": []
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{read}");
    let read_id = read["id"].as_i64().unwrap();
    let (status, _) = app
        .post(
            &format!("/api/suggestions/{read_id}/dismiss"),
            json!({"as": "dismissed"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Once a run no longer has the event it removes, the note goes.
    let (status, _) = app
        .delete(&format!("/api/scenarios/{scenario_id}/events/{spending}"))
        .await;
    assert!(status.is_success(), "{status}");
    let (status, run) = app
        .post(
            &format!("/api/scenarios/{scenario_id}/runs"),
            json!({"iterations": 40, "seed": 7}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    assert_eq!(
        app.await_run(run["id"].as_i64().unwrap()).await,
        "succeeded"
    );
    let after = app.review(scenario_id).await;
    assert_ne!(after["run_id"], run_id);
    let listed: Vec<(Value, Value)> = after["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["source"] == "ai")
        .map(|s| (s["id"].clone(), s["status"].clone()))
        .collect();
    assert_eq!(listed, [(json!(read_id), json!("dismissed"))], "{after}");
}

#[tokio::test]
async fn a_written_suggestion_is_checked_before_it_is_stored() {
    let mut app = TestApp::new().await;
    app.login_as("draft@example.com").await;
    let (scenario_id, run_id, spending, _) = app.previewable_plan().await;

    let (status, stored) = app.suggest(scenario_id, drop_spending(spending)).await;
    assert_eq!(status, StatusCode::CREATED, "{stored}");
    assert_eq!(stored["source"], "ai");
    assert_eq!(stored["rule"], Value::Null);
    assert_eq!(stored["summary"], "The one-line lead.");
    assert_eq!(stored["run_id"], run_id);
    assert_eq!(stored["status"], "open");
    assert_eq!(stored["paths"][0]["check"], Value::Null);
    assert_eq!(stored["paths"][0]["estimate"]["funding_success_rate"], 1.0);
    assert!(
        stored["paths"][0]["steps"][0]["diff"][0]["label"]
            .as_str()
            .unwrap()
            .contains("Spending"),
        "{stored}"
    );

    // The same proposal twice is one suggestion.
    let (status, _) = app.suggest(scenario_id, drop_spending(spending)).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Changes that do not fit the run's plan come back with their problems.
    let mut stale = drop_spending(spending);
    stale["title"] = json!("Halve the spending");
    stale["paths"][0]["steps"][0]["changes"] = json!([{"op": "replace", "target": {"event": spending},
        "path": "/effects/0/amount/value", "expect": 999.0, "value": 500.0}]);
    let (status, body) = app.suggest(scenario_id, stale).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["problems"][0]["kind"], "stale", "{body}");
    assert_eq!(body["by_step"][0]["path"], "a", "{body}");
    assert_eq!(body["by_step"][0]["step"], "a", "{body}");
    assert_eq!(body["by_step"][0]["problems"][0]["kind"], "stale", "{body}");
    assert_eq!(body["error"]["code"], "unprocessable");

    // Evidence must point at what the run has.
    for evidence in [
        json!({"ref": "ledger", "year": 1900, "event_id": null, "account_id": null}),
        json!({"ref": "account_series", "account_id": 999_999, "date": "2027-12-31", "value": 1.0}),
        json!({"ref": "diagnostic", "field": "no_such_field", "value": 1.0}),
    ] {
        let mut draft = drop_spending(spending);
        draft["title"] = json!("Other");
        draft["evidence"] = json!([evidence]);
        let (status, body) = app.suggest(scenario_id, draft).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    let mut odd = drop_spending(spending);
    odd["title"] = json!("Other");
    odd["paths"][0]["estimate"] = json!({"success_rate": 1.5, "funding_success_rate": null});
    let (status, _) = app.suggest(scenario_id, odd).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A preview simulates the stored changes and keeps the result.
    let id = stored["id"].as_i64().unwrap();
    let (status, _) = app
        .post(
            &format!("/api/suggestions/{id}/preview"),
            json!({"path": "nope"}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, preview) = app
        .post(
            &format!("/api/suggestions/{id}/preview"),
            json!({"path": "a", "through_step": null}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["edited"]["funding_success_rate"], 1.0, "{preview}");
    let (_, open) = app
        .get(&format!(
            "/api/scenarios/{scenario_id}/suggestions?status=open"
        ))
        .await;
    let check = &open[0]["paths"][0]["check"];
    assert_eq!(check["edited"]["funding_success_rate"], 1.0, "{open}");
    assert_eq!(check["paired"], true);
}

#[tokio::test]
async fn applying_a_suggestion_writes_the_plan_once_and_refuses_a_stale_one() {
    let mut app = TestApp::new().await;
    app.login_as("apply@example.com").await;
    let (scenario_id, _, spending, _) = app.previewable_plan().await;

    let halve = |title: &str, value: f64| {
        let mut draft = drop_spending(spending);
        draft["title"] = json!(title);
        draft["paths"][0]["steps"][0]["changes"] = json!([{"op": "replace", "target": {"event": spending},
            "path": "/effects/0/amount/value", "expect": 1000.0, "value": value}]);
        draft
    };
    let (status, first) = app.suggest(scenario_id, halve("Spend $500", 500.0)).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let first = first["id"].as_i64().unwrap();

    let (_, before) = app.get(&format!("/api/scenarios/{scenario_id}")).await;
    let (status, applied) = app
        .post(&format!("/api/suggestions/{first}/apply"), apply_path("a"))
        .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["scenario_id"], scenario_id);
    assert_eq!(applied["suggestion"]["status"], "applied");
    assert_eq!(applied["suggestion"]["applied_path"], "a");
    assert_eq!(
        applied["suggestion"]["paths"][0]["steps"][0]["applied"],
        true
    );
    assert!(applied["suggestion"]["resolved_at"].is_string());
    let (_, event) = app
        .get(&format!("/api/scenarios/{scenario_id}/events/{spending}"))
        .await;
    assert_eq!(event["effects"][0]["amount"]["value"], 500.0, "{event}");
    let (_, after) = app.get(&format!("/api/scenarios/{scenario_id}")).await;
    assert!(after["updated_at"].as_str() >= before["updated_at"].as_str());
    // Nothing is run on the user's behalf.
    let (_, runs) = app.get(&format!("/api/scenarios/{scenario_id}/runs")).await;
    assert_eq!(runs.as_array().unwrap().len(), 1);

    let (status, _) = app
        .post(&format!("/api/suggestions/{first}/apply"), apply_path("a"))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Written against the run (spending 1000), applied to a plan that now
    // spends 500: stale.
    let (status, second) = app.suggest(scenario_id, halve("Spend $750", 750.0)).await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    let second = second["id"].as_i64().unwrap();
    let (status, body) = app
        .post(&format!("/api/suggestions/{second}/apply"), apply_path("a"))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["problems"][0]["kind"], "stale", "{body}");
    assert_eq!(body["problems"][0]["actual"], 500.0, "{body}");
    let (_, event) = app
        .get(&format!("/api/scenarios/{scenario_id}/events/{spending}"))
        .await;
    assert_eq!(event["effects"][0]["amount"]["value"], 500.0);
    let (_, still) = app
        .get(&format!(
            "/api/scenarios/{scenario_id}/suggestions?status=open"
        ))
        .await;
    assert_eq!(still[0]["id"], second);
}

#[tokio::test]
async fn applying_to_a_copy_leaves_the_plan_alone() {
    let mut app = TestApp::new().await;
    app.login_as("copy@example.com").await;
    let (scenario_id, _, spending, _) = app.previewable_plan().await;
    let (_, stored) = app.suggest(scenario_id, drop_spending(spending)).await;
    let id = stored["id"].as_i64().unwrap();

    let (status, applied) = app
        .post(
            &format!("/api/suggestions/{id}/apply"),
            json!({"path": "a", "through_step": null, "to": "copy", "name": null}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    let copy = applied["scenario_id"].as_i64().unwrap();
    assert_ne!(copy, scenario_id);
    assert_eq!(applied["suggestion"]["status"], "applied");

    let (_, scenario) = app.get(&format!("/api/scenarios/{copy}")).await;
    assert_eq!(
        scenario["name"],
        "Test plan — Spending empties checking within the year"
    );
    let (_, events) = app.get(&format!("/api/scenarios/{copy}/events")).await;
    let copied = events
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Spending")
        .unwrap();
    assert_eq!(copied["effects"], json!([]), "{copied}");
    let (_, original) = app
        .get(&format!("/api/scenarios/{scenario_id}/events/{spending}"))
        .await;
    assert_eq!(original["effects"].as_array().unwrap().len(), 1);
}

/// The Checking account's id and its return profile, from the plan.
async fn checking_of(app: &TestApp, scenario_id: i64) -> (i64, i64) {
    let (_, accounts) = app
        .get(&format!("/api/scenarios/{scenario_id}/accounts"))
        .await;
    let checking = accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "Checking")
        .unwrap()
        .clone();
    (
        checking["id"].as_i64().unwrap(),
        checking["return_profile_id"].as_i64().unwrap(),
    )
}

/// A two-path fix: open a buffer account and fund it (two steps, the second
/// naming what the first creates), or stop the spending (one step).
fn buffer_or_stop(spending: i64, checking: i64, cash_profile: i64) -> Value {
    json!({
        "run_id": null, "kind": "fix", "section": "plan",
        "title": "Checking pays $1,000 a month with nothing coming in",
        "reasoning": "Either hold a buffer or stop the spending.",
        "evidence": [],
        "paths": [
            {
                "key": "buffer", "label": "Open a savings buffer and fund it",
                "reasoning": null, "recommended": true, "estimate": null,
                "steps": [
                    {"key": "open", "title": "Open a savings account", "reasoning": null,
                     "changes": [{"op": "add", "target": {"new_account": "buffer"}, "path": "",
                                  "value": {"name": "Buffer", "flavor": "Bank",
                                            "cash_value": 0.0,
                                            "return_profile_id": cash_profile}}]},
                    {"key": "fund", "title": "Move $500 a month into it", "reasoning": null,
                     "changes": [{"op": "add", "target": {"new_event": "fund"}, "path": "",
                                  "value": {"name": "Fund buffer",
                                            "trigger": {"kind": "Repeating", "interval": "Monthly"},
                                            "effects": [{"kind": "CashTransfer",
                                                         "from_account_id": checking,
                                                         "to_account_id": {"$new": "buffer"},
                                                         "amount": {"kind": "Fixed", "value": 500.0}}]}}]}
                ]
            },
            {
                "key": "stop", "label": "Stop the spending", "reasoning": null,
                "recommended": false, "estimate": null,
                "steps": [{"key": "a", "title": "Remove its expense", "reasoning": null,
                           "changes": [{"op": "remove", "target": {"event": spending},
                                        "path": "/effects/0"}]}]
            }
        ]
    })
}

#[tokio::test]
async fn a_path_is_followed_step_by_step_and_later_steps_use_what_earlier_ones_created() {
    let mut app = TestApp::new().await;
    app.login_as("paths@example.com").await;
    let (scenario_id, _, spending, _) = app.previewable_plan().await;
    let (checking, cash_profile) = checking_of(&app, scenario_id).await;

    let (status, stored) = app
        .suggest(
            scenario_id,
            buffer_or_stop(spending, checking, cash_profile),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{stored}");
    let id = stored["id"].as_i64().unwrap();
    let buffer = &stored["paths"][0];
    assert_eq!(buffer["key"], "buffer");
    assert!(
        buffer["steps"][0]["diff"][0]["label"]
            .as_str()
            .unwrap()
            .contains("Buffer"),
        "{stored}"
    );
    assert!(
        buffer["steps"][1]["diff"][0]["label"]
            .as_str()
            .unwrap()
            .contains("Fund buffer"),
        "{stored}"
    );
    assert_eq!(stored["paths"][1]["key"], "stop");

    // Previewing part of a path shows it without keeping it as the check.
    let (status, partial) = app
        .post(
            &format!("/api/suggestions/{id}/preview"),
            json!({"path": "buffer", "through_step": "open"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{partial}");
    assert!(
        partial["problems"].as_array().unwrap().is_empty(),
        "{partial}"
    );
    let (status, whole) = app
        .post(
            &format!("/api/suggestions/{id}/preview"),
            json!({"path": "buffer", "through_step": null}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{whole}");
    assert!(whole["problems"].as_array().unwrap().is_empty(), "{whole}");
    let (_, listed) = app
        .get(&format!(
            "/api/scenarios/{scenario_id}/suggestions?status=open"
        ))
        .await;
    assert!(listed[0]["paths"][0]["check"].is_object(), "{listed}");
    assert_eq!(listed[0]["paths"][1]["check"], Value::Null);

    // Step one: the account exists; the path is started, the note still open.
    let (status, applied) = app
        .post(
            &format!("/api/suggestions/{id}/apply"),
            json!({"path": "buffer", "through_step": "open", "to": "plan", "name": null}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    let note = &applied["suggestion"];
    assert_eq!(note["status"], "open");
    assert_eq!(note["applied_path"], "buffer");
    assert_eq!(note["paths"][0]["steps"][0]["applied"], true);
    assert!(note["paths"][0]["steps"][0]["applied_at"].is_string());
    assert_eq!(note["paths"][0]["steps"][1]["applied"], false);
    assert_eq!(note["paths"][0]["steps"][1]["applied_at"], Value::Null);
    assert_eq!(note["resolved_at"], Value::Null);
    let buffer_id = app.account_named(scenario_id, "Buffer").await;
    let (_, events) = app
        .get(&format!("/api/scenarios/{scenario_id}/events"))
        .await;
    assert!(
        !events
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "Fund buffer")
    );

    // The other path is closed; a step already applied is not applied again.
    let (status, _) = app
        .post(&format!("/api/suggestions/{id}/apply"), apply_path("stop"))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = app
        .post(
            &format!("/api/suggestions/{id}/apply"),
            json!({"path": "buffer", "through_step": "open", "to": "plan", "name": null}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = app
        .post(
            &format!("/api/suggestions/{id}/apply"),
            json!({"path": "buffer", "through_step": null, "to": "copy", "name": null}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a started path goes to no copy"
    );

    // A new review keeps the part-applied note on the board.
    let review = app.review(scenario_id).await;
    assert!(
        review["suggestions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == id),
        "{review}"
    );

    // Step two, in a later request: its `$new` names the account step one
    // created, by the id it was written under.
    let (status, applied) = app
        .post(
            &format!("/api/suggestions/{id}/apply"),
            apply_path("buffer"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    let note = &applied["suggestion"];
    assert_eq!(note["status"], "applied");
    assert!(note["resolved_at"].is_string());
    assert_eq!(note["paths"][0]["steps"][1]["applied"], true);
    assert_eq!(
        note["paths"][0]["steps"][1]["applied_at"],
        note["resolved_at"]
    );
    let (_, events) = app
        .get(&format!("/api/scenarios/{scenario_id}/events"))
        .await;
    let fund = events
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Fund buffer")
        .expect("the funding event")
        .clone();
    assert_eq!(fund["effects"][0]["to_account_id"], buffer_id, "{fund}");
    assert_eq!(fund["effects"][0]["from_account_id"], checking);

    let (status, _) = app
        .post(
            &format!("/api/suggestions/{id}/apply"),
            apply_path("buffer"),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn paths_are_checked_for_shape_and_problems_name_their_path_and_step() {
    let mut app = TestApp::new().await;
    app.login_as("shapes@example.com").await;
    let (scenario_id, _, spending, _) = app.previewable_plan().await;
    let (checking, cash_profile) = checking_of(&app, scenario_id).await;
    let base = buffer_or_stop(spending, checking, cash_profile);
    let set_value = |from: f64, to: f64| {
        json!({"op": "replace", "target": {"event": spending},
               "path": "/effects/0/amount/value", "expect": from, "value": to})
    };

    let mut refused: Vec<(&str, Value)> = Vec::new();
    let mut many = base.clone();
    let path = many["paths"][1].clone();
    for key in ["c", "d", "e"] {
        let mut extra = path.clone();
        extra["key"] = json!(key);
        extra["steps"][0]["changes"] = json!([set_value(1000.0, 900.0)]);
        many["paths"].as_array_mut().unwrap().push(extra);
    }
    refused.push(("five paths", many));
    let mut dup = base.clone();
    dup["paths"][1]["key"] = json!("buffer");
    refused.push(("duplicate path keys", dup));
    let mut twins = base.clone();
    twins["paths"][1]["steps"] = twins["paths"][0]["steps"].clone();
    refused.push(("identical paths", twins));
    let mut two = base.clone();
    two["paths"][1]["recommended"] = json!(true);
    refused.push(("two recommended", two));
    let mut bad_key = base.clone();
    bad_key["paths"][0]["steps"][0]["key"] = json!("Open");
    refused.push(("bad step key", bad_key));
    let mut long = base.clone();
    long["paths"][1]["steps"] = json!(
        (0..5)
            .map(|i| json!({"key": format!("s{i}"), "title": "Step",
                        "changes": [set_value(1000.0, 900.0)]}))
            .collect::<Vec<_>>()
    );
    refused.push(("five steps", long));
    let mut read = base.clone();
    read["kind"] = json!("read");
    refused.push(("a read note with paths", read));
    let mut bare = base.clone();
    bare["paths"] = json!([]);
    refused.push(("a fix without paths", bare));
    for (why, draft) in refused {
        let (status, body) = app.suggest(scenario_id, draft).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{why}: {body}");
    }

    // Steps read the plan as the earlier ones leave it: the second step's
    // `expect` is the first one's value.
    let mut sequential = base.clone();
    sequential["paths"][1]["steps"] = json!([
        {"key": "cut", "title": "Cut to $800", "changes": [set_value(1000.0, 800.0)]},
        {"key": "more", "title": "Then to $600", "changes": [set_value(800.0, 600.0)]}
    ]);
    let (status, body) = app.suggest(scenario_id, sequential.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // A step that reads the plan wrong is named, with its path.
    let mut stale = base.clone();
    stale["title"] = json!("Another note");
    stale["paths"][1]["steps"] = json!([
        {"key": "cut", "title": "Cut to $800", "changes": [set_value(1000.0, 800.0)]},
        {"key": "more", "title": "Then to $600", "changes": [set_value(1000.0, 600.0)]}
    ]);
    let (status, body) = app.suggest(scenario_id, stale).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["by_step"][0]["path"], "stop", "{body}");
    assert_eq!(body["by_step"][0]["step"], "more", "{body}");
    assert_eq!(body["by_step"][0]["problems"][0]["kind"], "stale", "{body}");
    assert_eq!(body["problems"], body["by_step"][0]["problems"]);

    // The same courses of action in another order, keys and labels are one
    // suggestion.
    let mut shuffled = sequential;
    shuffled["title"] = json!("A differently worded title");
    let paths = shuffled["paths"].as_array_mut().unwrap();
    paths.reverse();
    paths[0]["key"] = json!("first");
    paths[0]["label"] = json!("Other words");
    let (status, body) = app.suggest(scenario_id, shuffled).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn suggestions_belong_to_the_plans_owner() {
    let mut owner = TestApp::new().await;
    owner.login_as("suggest-owner@example.com").await;
    let (scenario_id, _, spending, _) = owner.previewable_plan().await;
    let (_, stored) = owner.suggest(scenario_id, drop_spending(spending)).await;
    let id = stored["id"].as_i64().unwrap();

    let mut intruder = TestApp {
        router: owner.router.clone(),
        cookie: None,
        _dir: tempfile::tempdir().unwrap(),
    };
    intruder.login_as("suggest-intruder@example.com").await;
    let (status, _) = intruder
        .get(&format!("/api/scenarios/{scenario_id}/review"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = intruder
        .post(
            &format!("/api/scenarios/{scenario_id}/review"),
            json!({"run_id": null}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = intruder
        .get(&format!("/api/scenarios/{scenario_id}/suggestions"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = intruder.suggest(scenario_id, drop_spending(spending)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    for (path, body) in [
        ("preview", json!({"path": "a", "through_step": null})),
        ("apply", apply_path("a")),
        ("dismiss", json!({"as": "dismissed"})),
        ("reopen", json!({})),
    ] {
        let (status, _) = intruder
            .post(&format!("/api/suggestions/{id}/{path}"), body)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    let (_, mine) = owner
        .get(&format!(
            "/api/scenarios/{scenario_id}/suggestions?status=open"
        ))
        .await;
    assert_eq!(mine[0]["id"], id);
}

// ── model-written review notes ──────────────────────────────────────────────

mod review_model {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use finplan_server::suggest::ai::{
        AiClient, BoxFuture, DEFAULT_MODEL, Reply as ModelReply, Request, Settings, Transport,
        TransportError,
    };
    use serde_json::{Value, json};
    use tokio::sync::Semaphore;

    /// What the scripted API does with the next request.
    pub enum Reply {
        Message(Value),
        /// Waits for the test to open the gate, then answers.
        Gated(Value),
        Fail(TransportError),
        /// Never answers.
        Hang,
    }

    /// A Messages API that plays back a script, one reply per request.
    pub struct Script {
        replies: Mutex<VecDeque<Reply>>,
        gate: Semaphore,
        pub requests: Mutex<Vec<Value>>,
    }

    impl Script {
        pub fn new(replies: Vec<Reply>) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(replies.into()),
                gate: Semaphore::new(0),
                requests: Mutex::new(Vec::new()),
            })
        }

        /// Queue more replies behind any already scripted.
        pub fn push(&self, replies: Vec<Reply>) {
            self.replies.lock().unwrap().extend(replies);
        }

        pub fn open_gate(&self) {
            self.gate.add_permits(1);
        }

        pub fn client(self: &Arc<Self>) -> Option<Arc<AiClient>> {
            let settings = Settings {
                model: DEFAULT_MODEL.into(),
                max_turns: 6,
                max_suggestions: 3,
                max_previews: 3,
                max_tokens: 2_000,
                thinking: finplan_server::suggest::ai::ThinkingMode::On,
                effort: "high",
                max_retries: 0,
                retry_base: Duration::from_millis(1),
                retry_cap: Duration::from_millis(1),
                materiality: Default::default(),
            };
            Some(Arc::new(AiClient::new(settings, self.clone(), None)))
        }
    }

    /// Scripted replies are written as JSON and read as Messages replies;
    /// requests are recorded as the JSON that would go on the wire.
    impl Transport for Script {
        fn send<'a>(
            &'a self,
            request: &'a Request,
        ) -> BoxFuture<'a, Result<ModelReply, TransportError>> {
            Box::pin(async move {
                self.requests
                    .lock()
                    .unwrap()
                    .push(serde_json::to_value(request).unwrap());
                let next = self.replies.lock().unwrap().pop_front();
                let reply = match next {
                    Some(Reply::Message(reply)) => reply,
                    Some(Reply::Gated(reply)) => {
                        self.gate.acquire().await.unwrap().forget();
                        reply
                    }
                    Some(Reply::Fail(error)) => return Err(error),
                    Some(Reply::Hang) => std::future::pending().await,
                    None => return Err(TransportError::Network("script ran out".into())),
                };
                Ok(serde_json::from_value(reply).expect("scripted reply is a Messages reply"))
            })
        }
    }

    fn message(stop_reason: &str, content: Value) -> Value {
        json!({
            "id": "msg", "type": "message", "role": "assistant", "model": DEFAULT_MODEL,
            "stop_reason": stop_reason, "content": content,
            "usage": {"input_tokens": 100, "output_tokens": 20}
        })
    }

    pub fn call(id: &str, tool: &str, input: Value) -> Value {
        message(
            "tool_use",
            json!([{"type": "tool_use", "id": id, "name": tool, "input": input}]),
        )
    }

    pub fn end() -> Value {
        message(
            "end_turn",
            json!([{"type": "text", "text": "One note added."}]),
        )
    }

    /// The model's changes for its one note: more cash in checking.
    pub fn raise_checking(checking: i64) -> Value {
        json!([{
            "op": "replace", "target": {"account": checking}, "path": "/cash_value",
            "expect": 10_000.0, "value": 20_000.0
        }])
    }

    /// One full pass: preview the changes, submit the note, end.
    pub fn pass(checking: i64) -> Vec<Reply> {
        let changes = raise_checking(checking);
        vec![
            Reply::Message(call("t1", "preview_changes", json!({"changes": changes}))),
            Reply::Message(call(
                "t2",
                "submit_suggestion",
                json!({
                    "kind": "fix", "section": "portfolio", "motive": "realism",
                    "title": "Checking starts too thin for the loan payments",
                    "summary": "The one-line lead.", "reasoning": "Checking opens at $10,000 and the loan takes $200 a month.",
                    "evidence": [],
                    "paths": [{"key": "a", "label": "Start checking at $20,000",
                               "recommended": true,
                               "steps": [{"key": "a", "title": "Raise checking",
                                          "changes": changes}]}]
                }),
            )),
            Reply::Message(end()),
        ]
    }
}

use review_model::{Reply, Script};

impl TestApp {
    async fn account_named(&self, scenario_id: i64, name: &str) -> i64 {
        let (_, accounts) = self
            .get(&format!("/api/scenarios/{scenario_id}/accounts"))
            .await;
        accounts
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    }

    /// Poll the review until its model pass is no longer running.
    async fn await_review_model(&self, scenario_id: i64) -> Value {
        for _ in 0..200 {
            let (_, review) = self
                .get(&format!("/api/scenarios/{scenario_id}/review"))
                .await;
            if review["ai"]["status"] != "running" {
                return review;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("the review model never finished");
    }
}

fn model_notes(review: &Value) -> Vec<&Value> {
    review["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["source"] == "ai")
        .collect()
}

#[tokio::test]
async fn a_review_without_a_model_reports_no_model_pass() {
    let mut app = TestApp::new().await;
    app.login_as("no-model@example.com").await;
    let (scenario_id, _, _) = app.reviewable_plan().await;
    let review = app.review(scenario_id).await;
    assert_eq!(review["ai"], Value::Null, "{review}");
    assert!(!review["suggestions"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn the_model_adds_checked_notes_after_the_rule_notes() {
    // Two passes: the first writes a note the user then dismisses; the second
    // proposes it again and is kept quiet.
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("model-review@example.com").await;
    let (scenario_id, run_id, _) = app.reviewable_plan().await;
    let checking = app.account_named(scenario_id, "Checking").await;
    {
        let mut replies = review_model::pass(checking);
        // Hold the first request until the rule notes have been answered.
        let Reply::Message(first) = replies.remove(0) else {
            unreachable!()
        };
        replies.insert(0, Reply::Gated(first));
        replies.extend(review_model::pass(checking));
        script.push(replies);
    }

    let review = app.review(scenario_id).await;
    assert_eq!(review["ai"]["status"], "running", "{review}");
    assert!(review["ai"]["started_at"].is_string());
    assert!(model_notes(&review).is_empty());
    assert!(!by_rule(&review, "liability_payment_inflation_adjusted").is_empty());
    script.open_gate();

    let review = app.await_review_model(scenario_id).await;
    assert_eq!(review["ai"]["status"], "done", "{review}");
    assert_eq!(review["ai"]["stop"], "finished");
    assert_eq!(review["ai"]["error"], Value::Null);
    assert!(review["ai"]["finished_at"].is_string());
    let notes = model_notes(&review);
    assert_eq!(notes.len(), 1, "{review}");
    let note = notes[0];
    assert_eq!(note["rule"], Value::Null);
    assert_eq!(note["run_id"], run_id);
    assert_eq!(note["kind"], "fix");
    assert_eq!(note["status"], "open");
    assert!(
        note["paths"][0]["steps"][0]["diff"][0]["label"]
            .as_str()
            .unwrap()
            .contains("Checking"),
        "{note}"
    );
    // The model's own preview became the note's check.
    assert_eq!(note["paths"][0]["check"]["iterations"], 40, "{note}");
    assert_eq!(note["paths"][0]["check"]["paired"], true);
    assert!(note["paths"][0]["check"]["edited"]["success_rate"].is_number());
    // The rule notes are still there.
    assert!(!by_rule(&review, "liability_payment_inflation_adjusted").is_empty());

    // The model read the plan and the rule notes, never the API key.
    let requests = script.requests.lock().unwrap().clone();
    let first = requests[0].to_string();
    assert!(first.contains("Checking"));
    assert!(first.contains("preview_changes"));

    // Dismissed, the note stays gone when the next pass proposes it again.
    let id = note["id"].as_i64().unwrap();
    let (status, _) = app
        .post(
            &format!("/api/suggestions/{id}/dismiss"),
            json!({"as": "dismissed"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    app.review(scenario_id).await;
    let review = app.await_review_model(scenario_id).await;
    assert_eq!(review["ai"]["status"], "done", "{review}");
    // Listed once, as dismissed (for undo), not raised again as open.
    let notes = model_notes(&review);
    assert_eq!(notes.len(), 1, "{review}");
    assert_eq!(notes[0]["id"], id);
    assert_eq!(notes[0]["status"], "dismissed");
    // The second pass was told the user dismissed it.
    let title = note["title"].as_str().unwrap().to_owned();
    let told = script
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(Value::to_string)
        .any(|r| r.contains("<dismissed_notes>") && r.contains(&title));
    assert!(told, "no request listed the dismissed note");
    let (_, dismissed) = app
        .get(&format!(
            "/api/scenarios/{scenario_id}/suggestions?status=dismissed"
        ))
        .await;
    assert_eq!(dismissed.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_failed_model_pass_leaves_the_rule_notes() {
    let script = Script::new(vec![Reply::Fail(
        finplan_server::suggest::ai::TransportError::Status {
            status: 400,
            error_type: None,
            message: "bad request".into(),
        },
    )]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("model-fails@example.com").await;
    let (scenario_id, _, _) = app.reviewable_plan().await;

    app.review(scenario_id).await;
    let review = app.await_review_model(scenario_id).await;
    assert_eq!(review["ai"]["status"], "failed", "{review}");
    assert_eq!(
        review["ai"]["error"],
        "the review model could not be reached"
    );
    // The public message never carries the API's own text.
    assert!(!review["ai"].to_string().contains("bad request"));
    assert!(model_notes(&review).is_empty());
    assert!(!by_rule(&review, "liability_payment_inflation_adjusted").is_empty());
}

#[tokio::test]
async fn a_new_review_supersedes_a_running_model_pass() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("model-supersede@example.com").await;
    let (scenario_id, _, _) = app.reviewable_plan().await;
    let checking = app.account_named(scenario_id, "Checking").await;
    {
        // Pass one finishes; pass two hangs; pass three proposes pass one's
        // note again.
        let mut replies = review_model::pass(checking);
        replies.push(Reply::Hang);
        replies.extend(review_model::pass(checking));
        script.push(replies);
    }

    app.review(scenario_id).await;
    let first = app.await_review_model(scenario_id).await;
    let first_notes = model_notes(&first);
    assert_eq!(first_notes.len(), 1, "{first}");
    let first_id = first_notes[0]["id"].clone();

    let running = app.review(scenario_id).await;
    assert_eq!(running["ai"]["status"], "running");
    // Wait until pass two has sent its (hanging) request.
    for _ in 0..200 {
        if script.requests.lock().unwrap().len() >= 4 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(script.requests.lock().unwrap().len(), 4);

    app.review(scenario_id).await;
    let last = app.await_review_model(scenario_id).await;
    assert_eq!(last["ai"]["status"], "done", "{last}");
    let notes = model_notes(&last);
    assert_eq!(notes.len(), 1, "{last}");
    // Pass one's open note was carried over, and pass three's repeat of it
    // was not stored again.
    assert_eq!(notes[0]["id"], first_id);
    let (_, open) = app
        .get(&format!(
            "/api/scenarios/{scenario_id}/suggestions?status=open"
        ))
        .await;
    let open_model_notes = open
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["source"] == "ai")
        .count();
    assert_eq!(open_model_notes, 1);
}

#[tokio::test]
async fn a_restart_fails_a_model_pass_left_running() {
    let script = Script::new(vec![Reply::Hang]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("model-restart@example.com").await;
    let (scenario_id, _, _) = app.reviewable_plan().await;

    let review = app.review(scenario_id).await;
    assert_eq!(review["ai"]["status"], "running");

    app.restart().await;
    let (_, review) = app
        .get(&format!("/api/scenarios/{scenario_id}/review"))
        .await;
    assert_eq!(review["ai"]["status"], "failed", "{review}");
    assert_eq!(review["ai"]["error"], "interrupted by a server restart");
    assert!(!review["suggestions"].as_array().unwrap().is_empty());
}

// ── "Chat about this" ───────────────────────────────────────────────────────

/// The model ending its turn with `text`.
fn chat_answer(text: &str) -> Value {
    json!({
        "id": "msg", "type": "message", "role": "assistant",
        "model": finplan_server::suggest::ai::DEFAULT_MODEL,
        "stop_reason": "end_turn", "content": [{"type": "text", "text": text}],
        "usage": {"input_tokens": 100, "output_tokens": 20}
    })
}

/// A chat turn that previews a change, submits it as a note, then answers.
fn chat_turn_adding_a_note(checking: i64) -> Vec<Reply> {
    let mut replies = review_model::pass(checking);
    replies.pop();
    replies.push(Reply::Message(chat_answer(
        "## Why\nChecking runs short in the first year. I added a suggestion to start it at $20,000.",
    )));
    replies
}

impl TestApp {
    /// A reviewed plan whose model pass added nothing: its scenario, one of
    /// its rule notes, and its checking account.
    async fn chat_ready(&mut self, email: &str, script: &Script) -> (i64, i64, i64) {
        self.login_as(email).await;
        let (scenario_id, _, _) = self.reviewable_plan().await;
        let checking = self.account_named(scenario_id, "Checking").await;
        script.push(vec![Reply::Message(chat_answer("Nothing to add."))]);
        self.review(scenario_id).await;
        let review = self.await_review_model(scenario_id).await;
        assert_eq!(review["ai"]["status"], "done", "{review}");
        let note = by_rule(&review, "liability_payment_inflation_adjusted")[0]["id"]
            .as_i64()
            .unwrap();
        (scenario_id, note, checking)
    }

    async fn chat(&self, note: i64, message: &str) -> (StatusCode, Value) {
        self.post(
            &format!("/api/suggestions/{note}/chat"),
            json!({"message": message}),
        )
        .await
    }

    /// Poll a thread until its turn is no longer running.
    async fn await_chat(&self, note: i64) -> Value {
        for _ in 0..200 {
            let (status, thread) = self.get(&format!("/api/suggestions/{note}/chat")).await;
            assert_eq!(status, StatusCode::OK, "{thread}");
            if thread["status"] != "running" {
                return thread;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("the chat turn never finished");
    }
}

fn roles(thread: &Value) -> Vec<&str> {
    thread["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["role"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn a_chat_turn_answers_and_adds_a_linked_suggestion() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    let (scenario_id, note, checking) = app.chat_ready("chat@example.com", &script).await;

    let (status, thread) = app.get(&format!("/api/suggestions/{note}/chat")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(thread["status"], "idle");
    assert_eq!(thread["messages"], json!([]));

    let mut replies = chat_turn_adding_a_note(checking);
    let Reply::Message(first) = replies.remove(0) else {
        unreachable!()
    };
    replies.insert(0, Reply::Gated(first));
    script.push(replies);
    let (status, thread) = app.chat(note, "  Why is this a problem?  ").await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    assert_eq!(thread["suggestion_id"], note);
    assert_eq!(thread["status"], "running");
    assert_eq!(roles(&thread), ["user"]);
    assert_eq!(thread["messages"][0]["text"], "Why is this a problem?");
    script.open_gate();

    let thread = app.await_chat(note).await;
    assert_eq!(thread["status"], "idle", "{thread}");
    assert_eq!(thread["error"], Value::Null);
    assert_eq!(roles(&thread), ["user", "assistant"]);
    let answer = &thread["messages"][1];
    // Plain text: the heading marker is gone.
    assert_eq!(
        answer["text"],
        "Why\nChecking runs short in the first year. I added a suggestion to start it at $20,000."
    );
    let added = answer["suggestion_ids"].as_array().unwrap();
    assert_eq!(added.len(), 1, "{thread}");
    let child = added[0].as_i64().unwrap();

    // The note it added is in the review, linked to the one discussed.
    let (_, review) = app
        .get(&format!("/api/scenarios/{scenario_id}/review"))
        .await;
    let linked = review["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == child)
        .unwrap_or_else(|| panic!("the chat's note is not in the review: {review}"));
    assert_eq!(linked["parent_id"], note);
    assert_eq!(linked["source"], "ai");
    assert_eq!(linked["status"], "open");
    assert_eq!(linked["paths"][0]["check"]["paired"], true, "{linked}");
    // Every other note has no parent.
    let (_, rule_note) = app
        .get(&format!(
            "/api/scenarios/{scenario_id}/suggestions?status=open"
        ))
        .await;
    assert!(
        rule_note
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["id"] != child)
            .all(|s| s["parent_id"].is_null())
    );

    // The model read the note and the question.
    let asked = script.requests.lock().unwrap().clone();
    let first_chat_request = asked
        .iter()
        .find(|r| r.to_string().contains("The note being discussed"))
        .expect("a chat request")
        .to_string();
    assert!(first_chat_request.contains("Why is this a problem?"));

    // A follow-up carries the thread so far.
    script.push(vec![Reply::Message(chat_answer(
        "Because the loan starts at once.",
    ))]);
    let (status, _) = app.chat(note, "And after that?").await;
    assert_eq!(status, StatusCode::OK);
    let thread = app.await_chat(note).await;
    assert_eq!(roles(&thread), ["user", "assistant", "user", "assistant"]);
    assert_eq!(thread["messages"][3]["suggestion_ids"], json!([]));
    let last = script.requests.lock().unwrap().last().unwrap().to_string();
    assert!(last.contains("Checking runs short in the first year"));
    assert!(last.contains("And after that?"));
}

#[tokio::test]
async fn chat_needs_a_review_model() {
    let mut app = TestApp::new().await;
    app.login_as("chat-no-model@example.com").await;
    let (scenario_id, _, _) = app.reviewable_plan().await;
    let review = app.review(scenario_id).await;
    let note = review["suggestions"][0]["id"].as_i64().unwrap();

    let (status, body) = app.chat(note, "Why?").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.to_string().contains("not enabled"));
    // Reading a thread still works; there is none.
    let (status, thread) = app.get(&format!("/api/suggestions/{note}/chat")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(thread["status"], "idle");
    assert_eq!(thread["messages"], json!([]));
}

#[tokio::test]
async fn chat_turns_are_checked_one_at_a_time_and_owner_scoped() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    let (_, note, _) = app.chat_ready("chat-rules@example.com", &script).await;

    for message in ["", "   ", &"x".repeat(2_001)] {
        let (status, body) = app.chat(note, message).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    let (status, thread) = app.get(&format!("/api/suggestions/{note}/chat")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        thread["messages"],
        json!([]),
        "a refused message is not kept"
    );

    script.push(vec![Reply::Hang]);
    let (status, thread) = app.chat(note, "First question").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(thread["status"], "running");
    let (status, body) = app.chat(note, "Second question").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (_, thread) = app.get(&format!("/api/suggestions/{note}/chat")).await;
    assert_eq!(roles(&thread), ["user"]);

    // Someone else's note reads as not found.
    app.login_as("chat-intruder@example.com").await;
    let (status, _) = app.get(&format!("/api/suggestions/{note}/chat")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.chat(note, "Let me in").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.get("/api/suggestions/999999/chat").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_thread_takes_a_bounded_number_of_questions() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    let (_, note, _) = app.chat_ready("chat-cap@example.com", &script).await;
    for i in 0..20 {
        script.push(vec![Reply::Message(chat_answer("An answer."))]);
        let (status, body) = app.chat(note, &format!("Question {i}")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let thread = app.await_chat(note).await;
        assert_eq!(thread["status"], "idle", "{thread}");
    }
    let (status, body) = app.chat(note, "One more").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (_, thread) = app.get(&format!("/api/suggestions/{note}/chat")).await;
    assert_eq!(thread["messages"].as_array().unwrap().len(), 40);
}

#[tokio::test]
async fn a_failed_chat_turn_says_so_and_can_be_retried() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    let (_, note, _) = app.chat_ready("chat-fails@example.com", &script).await;

    script.push(vec![Reply::Fail(
        finplan_server::suggest::ai::TransportError::Status {
            status: 400,
            error_type: None,
            message: "bad request".into(),
        },
    )]);
    let (status, _) = app.chat(note, "Why?").await;
    assert_eq!(status, StatusCode::OK);
    let thread = app.await_chat(note).await;
    assert_eq!(thread["status"], "failed", "{thread}");
    assert_eq!(thread["error"], "the model could not be reached");
    assert!(!thread.to_string().contains("bad request"));
    assert_eq!(roles(&thread), ["user"]);

    // Asking again reads both questions and answers.
    script.push(vec![Reply::Message(chat_answer("Here is why."))]);
    let (status, _) = app.chat(note, "Why, again?").await;
    assert_eq!(status, StatusCode::OK);
    let thread = app.await_chat(note).await;
    assert_eq!(thread["status"], "idle", "{thread}");
    assert_eq!(thread["error"], Value::Null);
    assert_eq!(roles(&thread), ["user", "user", "assistant"]);
    let last = script.requests.lock().unwrap().last().unwrap().to_string();
    assert!(last.contains("The user writes:\\nWhy?") && last.contains("Why, again?"));
}

#[tokio::test]
async fn a_restart_fails_a_chat_turn_left_running() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    let (_, note, _) = app.chat_ready("chat-restart@example.com", &script).await;
    script.push(vec![Reply::Hang]);
    let (_, thread) = app.chat(note, "Why?").await;
    assert_eq!(thread["status"], "running");

    app.restart().await;
    let (status, thread) = app.get(&format!("/api/suggestions/{note}/chat")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(thread["status"], "failed", "{thread}");
    assert_eq!(thread["error"], "interrupted by a server restart");
    assert_eq!(roles(&thread), ["user"]);
}

// ── Plan chat ───────────────────────────────────────────────────────────────

impl TestApp {
    /// A reviewed plan whose model pass added nothing: its scenario and its
    /// checking account.
    async fn plan_chat_ready(&mut self, email: &str, script: &Script) -> (i64, i64) {
        self.login_as(email).await;
        let (scenario_id, _, _) = self.reviewable_plan().await;
        let checking = self.account_named(scenario_id, "Checking").await;
        script.push(vec![Reply::Message(chat_answer("Nothing to add."))]);
        self.review(scenario_id).await;
        let review = self.await_review_model(scenario_id).await;
        assert_eq!(review["ai"]["status"], "done", "{review}");
        (scenario_id, checking)
    }

    async fn plan_chat(&self, scenario_id: i64, message: &str) -> (StatusCode, Value) {
        self.post(
            &format!("/api/scenarios/{scenario_id}/chat"),
            json!({"message": message}),
        )
        .await
    }

    async fn await_plan_chat(&self, scenario_id: i64) -> Value {
        for _ in 0..200 {
            let (status, thread) = self
                .get(&format!("/api/scenarios/{scenario_id}/chat"))
                .await;
            assert_eq!(status, StatusCode::OK, "{thread}");
            if thread["status"] != "running" {
                return thread;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("the plan chat turn never finished");
    }
}

#[tokio::test]
async fn plan_chat_proposes_changes_as_notes_and_leaves_the_plan_alone() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    let (scenario_id, checking) = app.plan_chat_ready("plan-chat@example.com", &script).await;
    let (_, before) = app
        .get(&format!("/api/scenarios/{scenario_id}/accounts"))
        .await;

    let (status, thread) = app.get(&format!("/api/scenarios/{scenario_id}/chat")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(thread["status"], "idle");
    assert_eq!(thread["messages"], json!([]));

    script.push(chat_turn_adding_a_note(checking));
    let (status, thread) = app
        .plan_chat(scenario_id, "  Put $20,000 in checking to start.  ")
        .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    assert_eq!(thread["scenario_id"], scenario_id);
    assert_eq!(
        thread["messages"][0]["text"],
        "Put $20,000 in checking to start."
    );

    let thread = app.await_plan_chat(scenario_id).await;
    assert_eq!(thread["status"], "idle", "{thread}");
    assert_eq!(roles(&thread), ["user", "assistant"]);
    let added = thread["messages"][1]["suggestion_ids"].as_array().unwrap();
    assert_eq!(added.len(), 1, "{thread}");
    let note_id = added[0].as_i64().unwrap();

    // The change is an open note on the review, with no parent note...
    let (_, review) = app
        .get(&format!("/api/scenarios/{scenario_id}/review"))
        .await;
    let note = review["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == note_id)
        .unwrap_or_else(|| panic!("the chat's note is not on the review: {review}"));
    assert_eq!(note["status"], "open");
    assert_eq!(note["source"], "ai");
    assert!(note["parent_id"].is_null());
    // ...and the plan itself is untouched until the user applies it.
    let (_, after) = app
        .get(&format!("/api/scenarios/{scenario_id}/accounts"))
        .await;
    assert_eq!(before, after);

    // The model read the whole plan's framing and the message.
    let asked = script.requests.lock().unwrap().clone();
    let request = asked
        .iter()
        .map(|r| r.to_string())
        .find(|r| r.contains("about the whole plan"))
        .expect("a plan chat request");
    assert!(request.contains("Put $20,000 in checking to start."));
    assert!(request.contains("cannot change the plan yourself"));

    // Starting over forgets the thread; the note stays on the board.
    let (status, _) = app
        .delete(&format!("/api/scenarios/{scenario_id}/chat"))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, thread) = app.get(&format!("/api/scenarios/{scenario_id}/chat")).await;
    assert_eq!(thread["messages"], json!([]));
    let (_, review) = app
        .get(&format!("/api/scenarios/{scenario_id}/review"))
        .await;
    assert!(
        review["suggestions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == note_id)
    );
}

/// A chat turn that rewrites note `id` in place: checking to start at `value`.
fn chat_turn_rewriting(checking: i64, id: i64, value: f64) -> Vec<Reply> {
    use review_model::call;
    let changes = json!([{
        "op": "replace", "target": {"account": checking}, "path": "/cash_value",
        "expect": 10_000.0, "value": value
    }]);
    vec![
        Reply::Message(call("t1", "preview_changes", json!({"changes": changes}))),
        Reply::Message(call(
            "t2",
            "submit_suggestion",
            json!({
                "kind": "fix", "section": "portfolio", "motive": "realism",
                "title": format!("Checking should start at ${value}"),
                "summary": "The one-line lead.", "reasoning": "A larger cushion for the loan payments.",
                "evidence": [],
                "paths": [{"key": "a", "label": format!("Start checking at ${value}"),
                           "recommended": true,
                           "steps": [{"key": "a", "title": "Raise checking", "changes": changes}]}],
                "replaces": id,
            }),
        )),
        Reply::Message(chat_answer("I changed the note.")),
    ]
}

#[tokio::test]
async fn plan_chat_rewrites_an_open_note_in_place_and_leaves_an_applied_one() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    let (scenario_id, checking) = app.plan_chat_ready("rewrite@example.com", &script).await;

    script.push(chat_turn_adding_a_note(checking));
    app.plan_chat(scenario_id, "Put $20,000 in checking.").await;
    let thread = app.await_plan_chat(scenario_id).await;
    let note_id = thread["messages"][1]["suggestion_ids"][0].as_i64().unwrap();
    let ai_notes = |review: &Value| model_notes(review).len();
    let (_, before) = app
        .get(&format!("/api/scenarios/{scenario_id}/review"))
        .await;

    // Asked for a different figure, the model rewrites the note it wrote.
    script.push(chat_turn_rewriting(checking, note_id, 25_000.0));
    app.plan_chat(scenario_id, "Make it $25,000 instead.").await;
    let thread = app.await_plan_chat(scenario_id).await;
    assert_eq!(thread["status"], "idle", "{thread}");
    assert_eq!(thread["messages"][3]["suggestion_ids"], json!([note_id]));
    let (_, review) = app
        .get(&format!("/api/scenarios/{scenario_id}/review"))
        .await;
    assert_eq!(ai_notes(&review), ai_notes(&before), "rewritten, not added");
    let note = review["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == note_id)
        .unwrap();
    assert_eq!(note["status"], "open");
    assert_eq!(note["title"], "Checking should start at $25000");
    assert_eq!(note["paths"][0]["label"], "Start checking at $25000");
    assert_eq!(
        note["paths"][0]["steps"][0]["changes"][0]["value"], 25_000.0,
        "{note}"
    );

    // Once applied, it is no longer the model's to rewrite.
    let (status, _) = app
        .post(
            &format!("/api/suggestions/{note_id}/apply"),
            apply_path("a"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    script.push(chat_turn_rewriting(checking, note_id, 30_000.0));
    app.plan_chat(scenario_id, "Now $30,000.").await;
    app.await_plan_chat(scenario_id).await;
    let told = script.requests.lock().unwrap().last().unwrap().to_string();
    assert!(told.contains("cannot be rewritten"), "{told}");
    let (_, review) = app
        .get(&format!("/api/scenarios/{scenario_id}/review"))
        .await;
    let note = review["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == note_id)
        .unwrap();
    assert_eq!(note["status"], "applied");
    assert_eq!(note["title"], "Checking should start at $25000");
}

#[tokio::test]
async fn plan_chat_needs_a_review_a_model_and_the_owner() {
    let mut app = TestApp::new().await;
    app.login_as("plan-chat-no-model@example.com").await;
    let (scenario_id, _, _) = app.reviewable_plan().await;
    let (status, body) = app.plan_chat(scenario_id, "Retire at 60").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.to_string().contains("not enabled"));
    let (_, entitlements) = app.get("/api/billing/entitlements").await;
    assert!(entitlements["ai_plan_chat"].is_null(), "{entitlements}");

    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("plan-chat-unreviewed@example.com").await;
    let (scenario_id, _, _) = app.reviewable_plan().await;
    let (status, body) = app.plan_chat(scenario_id, "Retire at 60").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.to_string().contains("review this plan first"));
    // Nothing was spent on a refused message.
    let (_, entitlements) = app.get("/api/billing/entitlements").await;
    assert_eq!(
        entitlements["ai_plan_chat"]["remaining"],
        entitlements["ai_plan_chat"]["per_month"]
    );
    for message in ["", "   ", &"x".repeat(2_001)] {
        let (status, _) = app.plan_chat(scenario_id, message).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    app.login_as("plan-chat-intruder@example.com").await;
    for (status, _) in [
        app.get(&format!("/api/scenarios/{scenario_id}/chat")).await,
        app.plan_chat(scenario_id, "Let me in").await,
        app.delete(&format!("/api/scenarios/{scenario_id}/chat"))
            .await,
    ] {
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn plan_chat_spends_a_monthly_allowance_one_turn_at_a_time() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_plan_chat(
        script.client(),
        finplan_server::suggest::ai::PlanChatConfig {
            free_per_month: 2,
            pro_per_month: 2,
        },
    )
    .await;
    let (scenario_id, _) = app
        .plan_chat_ready("plan-chat-quota@example.com", &script)
        .await;
    let (_, entitlements) = app.get("/api/billing/entitlements").await;
    assert_eq!(
        entitlements["ai_plan_chat"],
        json!({"enabled": true, "remaining": 2, "per_month": 2})
    );

    // One turn at a time: a second message while the first is out is refused
    // and costs nothing; so is clearing the thread.
    script.push(vec![Reply::Hang]);
    let (status, _) = app.plan_chat(scenario_id, "First").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = app.plan_chat(scenario_id, "Second").await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = app
        .delete(&format!("/api/scenarios/{scenario_id}/chat"))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, entitlements) = app.get("/api/billing/entitlements").await;
    assert_eq!(entitlements["ai_plan_chat"]["remaining"], 1);

    // A restart fails the hung turn.
    app.restart().await;
    let (_, thread) = app.get(&format!("/api/scenarios/{scenario_id}/chat")).await;
    assert_eq!(thread["status"], "failed", "{thread}");
    assert_eq!(thread["error"], "interrupted by a server restart");
}

#[tokio::test]
async fn plan_chat_refuses_once_the_month_is_spent() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_plan_chat(
        script.client(),
        finplan_server::suggest::ai::PlanChatConfig {
            free_per_month: 1,
            pro_per_month: 1,
        },
    )
    .await;
    let (scenario_id, _) = app
        .plan_chat_ready("plan-chat-spent@example.com", &script)
        .await;
    script.push(vec![Reply::Message(chat_answer("An answer."))]);
    let (status, _) = app.plan_chat(scenario_id, "How am I doing?").await;
    assert_eq!(status, StatusCode::OK);
    app.await_plan_chat(scenario_id).await;

    let (status, body) = app.plan_chat(scenario_id, "And now?").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.to_string().contains("plan chat messages"));
    let (_, thread) = app.get(&format!("/api/scenarios/{scenario_id}/chat")).await;
    assert_eq!(
        roles(&thread),
        ["user", "assistant"],
        "a refused message is not kept"
    );
    let (_, entitlements) = app.get("/api/billing/entitlements").await;
    assert_eq!(
        entitlements["ai_plan_chat"],
        json!({"enabled": false, "remaining": 0, "per_month": 1})
    );
}

// ── AI-guided drafts ────────────────────────────────────────────────────────

impl TestApp {
    async fn start_draft(&self) -> Value {
        let (status, draft) = self.post("/api/drafts", json!({})).await;
        assert_eq!(status, StatusCode::CREATED, "{draft}");
        draft
    }

    async fn ai_drafts(&self) -> Value {
        let (status, entitlements) = self.get("/api/billing/entitlements").await;
        assert_eq!(status, StatusCode::OK);
        entitlements["ai_drafts"].clone()
    }
}

fn today_utc() -> String {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .to_string()
}

#[tokio::test]
async fn drafts_are_offered_only_when_the_server_has_a_review_model() {
    let mut app = TestApp::new().await;
    app.login_as("no-drafts@example.com").await;
    assert_eq!(app.ai_drafts().await, Value::Null);
    let (status, body) = app.post("/api/drafts", json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn a_draft_is_hidden_replaced_and_deleted_and_spends_a_draft_once() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("drafts@example.com").await;
    // A self-hosted server is Pro: the Pro limits, all of them reported.
    assert_eq!(
        app.ai_drafts().await,
        json!({"enabled": true, "remaining": 20, "per_month": 20, "max_files": 25,
               "max_bytes": 104_857_600u64, "max_pages": 200})
    );

    let first = app.start_draft().await;
    let first_id = first["id"].as_i64().unwrap();
    assert_eq!(first["state"], "ready");
    assert_eq!(first["scenario"]["status"], "draft");
    assert_eq!(first["scenario"]["id"], first["id"]);
    // The start date is the server's: today, not anything the caller said.
    assert_eq!(first["scenario"]["start_date"], today_utc());
    assert_eq!(
        first["counts"],
        json!({"accounts": 0, "assets": 0, "events": 0, "parameters": 0, "open_suggestions": 0,
               "notes": 0, "notes_added": 0, "notes_to_confirm": 0})
    );
    assert!(first["expires_at"].as_str().is_some());
    assert_eq!(app.ai_drafts().await["remaining"], 19);

    // Not a plan yet: out of the list, though it reads like any scenario.
    let (_, listed) = app.get("/api/scenarios").await;
    assert_eq!(listed, json!([]));
    let (status, scenario) = app.get(&format!("/api/scenarios/{first_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(scenario["status"], "draft");
    let (status, _) = app.get(&format!("/api/drafts/{first_id}")).await;
    assert_eq!(status, StatusCode::OK);

    // A second draft replaces the first and spends another.
    let second = app.start_draft().await;
    let second_id = second["id"].as_i64().unwrap();
    assert_ne!(first_id, second_id);
    let (status, _) = app.get(&format!("/api/drafts/{first_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.get(&format!("/api/scenarios/{first_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(app.ai_drafts().await["remaining"], 18);

    // Cancelling deletes it at once, and does not give the draft back.
    let (status, _) = app.delete(&format!("/api/drafts/{second_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app.get(&format!("/api/drafts/{second_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.delete(&format!("/api/drafts/{second_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(app.ai_drafts().await["remaining"], 18);

    // A plan is not a draft: the draft routes leave it alone.
    let (plan_id, _, _) = app.seed_scenario().await;
    for (method, path) in [
        ("GET", format!("/api/drafts/{plan_id}")),
        ("DELETE", format!("/api/drafts/{plan_id}")),
        ("POST", format!("/api/drafts/{plan_id}/create")),
    ] {
        let (status, _) = app.send(method, &path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }
    let (status, _) = app.get(&format!("/api/scenarios/{plan_id}")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_draft_belongs_to_the_user_who_started_it() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("owner-of-draft@example.com").await;
    let id = app.start_draft().await["id"].as_i64().unwrap();
    app.login_as("someone-else@example.com").await;
    for (method, path) in [
        ("GET", format!("/api/drafts/{id}")),
        ("DELETE", format!("/api/drafts/{id}")),
        ("POST", format!("/api/drafts/{id}/create")),
    ] {
        let (status, _) = app.send(method, &path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }
}

/// One step that describes a whole plan: settings, a tax config and a return
/// profile that do not exist yet, a parameter, an account, an asset and an
/// event that use them.
fn draft_plan_changes(cash_profile: i64, inflation: i64) -> Value {
    json!([
        {"op": "replace", "target": "scenario", "path": "/inflation_profile_id",
         "value": inflation},
        {"op": "replace", "target": "scenario", "path": "/birth_date", "value": "1988-04-01"},
        {"op": "replace", "target": "scenario", "path": "/duration_years", "value": 40},
        {"op": "replace", "target": "scenario", "path": "/tax_config_id",
         "value": {"$new": "tax"}},
        {"op": "add", "target": {"new_tax_config": "tax"}, "path": "", "value": {
            "name": "Single, Colorado", "state_rate": 0.044,
            "federal_brackets": [{"threshold": 0.0, "rate": 0.10},
                                 {"threshold": 11000.0, "rate": 0.12}]}},
        {"op": "add", "target": {"new_return_profile": "broad"}, "path": "", "value": {
            "name": "US broad market", "asset_class": "UsEquity",
            "distribution": {"kind": "Bootstrap", "preset": "sp500"}}},
        {"op": "add", "target": {"new_asset": "vti"}, "path": "", "value": {
            "name": "VTI", "initial_price": 250.0, "return_profile_id": {"$new": "broad"}}},
        {"op": "add", "target": {"new_account": "checking"}, "path": "", "value": {
            "name": "Checking", "flavor": "Bank", "cash_value": 12000.0,
            "return_profile_id": cash_profile}},
        {"op": "add", "target": {"new_account": "brokerage"}, "path": "", "value": {
            "name": "Brokerage", "flavor": "Investment", "tax_status": "Taxable",
            "cash_value": 0.0, "cash_return_profile_id": cash_profile,
            "positions": [{"asset_id": {"$new": "vti"}, "units": 100.0, "cost_basis": 20000.0}]}},
        {"op": "add", "target": {"new_parameter": "age"}, "path": "", "value": {
            "name": "retirement_age", "value": {"kind": "Age", "years": 55, "months": 0}}},
        {"op": "add", "target": {"new_event": "retire"}, "path": "", "value": {
            "name": "Retirement spending", "fires_once": true,
            "trigger": {"kind": "AgeParameter", "parameter_id": {"$new": "age"}},
            "effects": [{"kind": "Expense", "from_account_id": {"$new": "checking"},
                         "amount": {"kind": "Fixed", "value": 3000.0}}]}},
    ])
}

#[tokio::test]
async fn a_draft_takes_notes_without_a_run_and_becomes_a_plan_when_created() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("draft-flow@example.com").await;
    let (_, profiles) = app.get("/api/return-profiles").await;
    let cash = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Savings Account")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let (_, inflation) = app.get("/api/inflation-profiles").await;
    let inflation = inflation[0]["id"].as_i64().unwrap();
    let id = app.start_draft().await["id"].as_i64().unwrap();

    // A note with no run to be written against.
    let (status, note) = app
        .suggest(
            id,
            json!({
                "run_id": null, "kind": "fix", "section": "plan",
                "title": "Describe the whole plan",
                "reasoning": "From the description: age, savings, retirement at 55.",
                "evidence": [],
                "paths": [{"key": "a", "label": "Add it", "recommended": true,
                           "steps": [{"key": "a", "title": "Describe the plan",
                                      "changes": draft_plan_changes(cash, inflation)}]}]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{note}");
    assert_eq!(note["run_id"], Value::Null);
    let labels: Vec<String> = note["paths"][0]["steps"][0]["diff"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["label"].as_str().unwrap().to_string())
        .collect();
    for expected in [
        "Scenario › birth date",
        "Scenario › inflation profile",
        "Scenario › tax config",
        "Assumptions › + Single, Colorado",
        "Assumptions › + US broad market",
        "Portfolio › + VTI",
        "Portfolio › + Checking",
        "Parameters › + $retirement_age",
        "Plan › + Retirement spending",
    ] {
        assert!(
            labels.iter().any(|l| l == expected),
            "{expected} in {labels:?}"
        );
    }
    let note_id = note["id"].as_i64().unwrap();
    let (_, status_now) = app.get(&format!("/api/drafts/{id}")).await;
    assert_eq!(status_now["counts"]["open_suggestions"], 1);

    // Previewing checks the batch and its diff; with no run there is nothing
    // to pair against, so the edited draft is simulated whole, unpaired.
    let (status, preview) = app
        .post(
            &format!("/api/suggestions/{note_id}/preview"),
            json!({"path": "a", "through_step": null}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["base_run_id"], Value::Null);
    assert_eq!(preview["problems"], json!([]));
    assert_eq!(preview["base"], Value::Null);
    assert_eq!(preview["paired"], false);
    assert!(
        preview["edited"]["success_rate"].as_f64().is_some(),
        "{preview}"
    );
    assert!(!preview["diff"].as_array().unwrap().is_empty());

    // A broken batch is reported the same way.
    let (_, broken) = app
        .post(
            &format!("/api/scenarios/{id}/preview"),
            json!({"changes": [{"op": "replace", "target": "scenario", "path": "/start_date",
                                "value": "2030-01-01"}]}),
        )
        .await;
    assert_eq!(broken["problems"][0]["kind"], "bad_path");

    // Chat needs a run's results, so it says so rather than failing.
    let (status, _) = app
        .post(
            &format!("/api/suggestions/{note_id}/chat"),
            json!({"message": "why?"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Applying writes the whole batch through the routes' SQL halves.
    let (status, applied) = app
        .post(
            &format!("/api/suggestions/{note_id}/apply"),
            json!({"path": "a", "through_step": null, "to": "plan"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    let (_, drafted) = app.get(&format!("/api/drafts/{id}")).await;
    assert_eq!(
        drafted["counts"],
        json!({"accounts": 2, "assets": 1, "events": 1, "parameters": 1, "open_suggestions": 0,
               "notes": 1, "notes_added": 1, "notes_to_confirm": 0})
    );
    let scenario = &drafted["scenario"];
    assert_eq!(scenario["birth_date"], "1988-04-01");
    assert_eq!(scenario["duration_years"], 40);
    assert_eq!(scenario["start_date"], today_utc());
    let (_, taxes) = app.get("/api/tax-configs").await;
    let tax = taxes
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "Single, Colorado")
        .expect("the tax config was created");
    assert_eq!(scenario["tax_config_id"], tax["id"]);
    assert_eq!(scenario["inflation_profile_id"], inflation);
    let (_, profiles) = app.get("/api/return-profiles").await;
    let broad = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "US broad market")
        .expect("the return profile was created");
    let (_, assets) = app.get(&format!("/api/scenarios/{id}/assets")).await;
    assert_eq!(assets[0]["return_profile_id"], broad["id"]);
    let (_, parameters) = app.get(&format!("/api/scenarios/{id}/parameters")).await;
    assert_eq!(parameters[0]["name"], "retirement_age");
    assert_eq!(
        parameters[0]["uses"][0]["event_name"],
        "Retirement spending"
    );

    // Create & run: the draft becomes a plan and its run is queued.
    let (status, created) = app
        .post(&format!("/api/drafts/{id}/create"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["scenario"]["status"], "active");
    assert_eq!(created["scenario"]["id"], id);
    let run_id = created["run"]["id"].as_i64().unwrap();
    let finished = app.await_run(run_id).await;
    let (_, run) = app.get(&format!("/api/runs/{run_id}")).await;
    assert_eq!(finished, "succeeded", "{run}");
    let (status, _) = app.get(&format!("/api/drafts/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, listed) = app.get("/api/scenarios").await;
    assert_eq!(names(&listed).len(), 1);

    // The rule review works as for any plan, and its notes carry the run; the
    // draft's own note keeps the null it was written with.
    let review = app.review(id).await;
    assert_eq!(review["run_id"], run_id);
    let (_, all) = app.get(&format!("/api/scenarios/{id}/suggestions")).await;
    let mine = all
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == note_id)
        .unwrap();
    assert_eq!(mine["run_id"], Value::Null);
    assert_eq!(mine["status"], "applied");
}

#[tokio::test]
async fn a_draft_that_cannot_run_stays_a_draft() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("draft-broken@example.com").await;
    let id = app.start_draft().await["id"].as_i64().unwrap();
    // An age trigger needs a birth date the draft does not have yet: the
    // event is stored, but the plan cannot be compiled to run.
    let (status, event) = app
        .post(
            &format!("/api/scenarios/{id}/events"),
            json!({"name": "Retire", "trigger": {"kind": "Age", "years": 60}}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{event}");
    let (status, created) = app
        .post(&format!("/api/drafts/{id}/create"), json!({}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{created}");
    let (status, draft) = app.get(&format!("/api/drafts/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    assert_eq!(draft["scenario"]["status"], "draft");
    let (_, listed) = app.get("/api/scenarios").await;
    assert_eq!(listed, json!([]));

    // Once it can run, the same draft is created.
    let (status, _) = app
        .patch(
            &format!("/api/scenarios/{id}"),
            json!({"birth_date": "1990-01-01"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, created) = app
        .post(&format!("/api/drafts/{id}/create"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
}
