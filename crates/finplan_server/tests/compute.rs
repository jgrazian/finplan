//! Server offload (spec 19): `/compute/runs`, its budget, `/health`'s local
//! mode and the `default_plan_home` preference.
//!
//! Each test builds its own server over a temporary database. Admission counts
//! are process-wide, per user, so tests do not interfere through them.

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use finplan_server::config::ServerConfig;
use finplan_server::state::AppState;
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "https://finplan.example";
const PASSWORD: &str = "correct horse battery";

fn config(hosted: bool) -> ServerConfig {
    ServerConfig {
        mail: Default::default(),
        review_ai: Default::default(),
        draft: Default::default(),
        plan_chat: Default::default(),
        log_format: Default::default(),
        metrics_bind: None,
        hosted,
        access_mode: Default::default(),
        registration_open: true,
        guest_access: true,
        guest_max_iterations: 100,
        guest_retention_days: 30,
        local_mode: true,
        offload: Default::default(),
        local_mail_sink: None,
        bind: "127.0.0.1:0".into(),
        database_url: "sqlite::memory:".into(),
        db_pool_size: 4,
        sim_workers: 1,
        max_iterations: 50_000,
        secure_cookies: hosted,
        cors_origins: vec![ORIGIN.into()],
    }
}

struct App {
    router: Router,
    state: AppState,
    peer: SocketAddr,
    _dir: tempfile::TempDir,
}

impl App {
    async fn new(mut config: ServerConfig, last_octet: u8) -> Self {
        let dir = tempfile::tempdir().unwrap();
        config.database_url = format!("sqlite://{}", dir.path().join("test.db").display());
        let (router, state) = finplan_server::build(config).await.unwrap();
        Self {
            router,
            state,
            peer: format!("198.51.100.{last_octet}:4000").parse().unwrap(),
            _dir: dir,
        }
    }

    async fn plain(last_octet: u8) -> Self {
        Self::new(config(false), last_octet).await
    }

    async fn raw(
        &self,
        method: &str,
        path: &str,
        cookie: Option<&str>,
        body: Vec<u8>,
    ) -> (StatusCode, HeaderMap, Value) {
        let response = loop {
            let mut req = Request::builder()
                .method(method)
                .uri(path)
                .header("origin", ORIGIN)
                .header("content-type", "application/json");
            if let Some(cookie) = cookie {
                req = req.header("cookie", cookie);
            }
            let mut req = req.body(Body::from(body.clone())).unwrap();
            req.extensions_mut()
                .insert(axum::extract::ConnectInfo(self.peer));
            let response = self.router.clone().oneshot(req).await.unwrap();
            // Hashing slots are process-wide; wait out "busy".
            if response.status() == StatusCode::TOO_MANY_REQUESTS
                && response
                    .headers()
                    .get("retry-after")
                    .is_some_and(|v| v == "2")
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
            break response;
        };
        let (status, headers) = (response.status(), response.headers().clone());
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        cookie: Option<&str>,
        body: Value,
    ) -> (StatusCode, Value) {
        let (status, _, value) = self
            .raw(method, path, cookie, serde_json::to_vec(&body).unwrap())
            .await;
        (status, value)
    }

    async fn register(&self, email: &str) -> String {
        let (status, headers, body) = self
            .raw(
                "POST",
                "/api/auth/register",
                None,
                serde_json::to_vec(&json!({
                    "email": email, "password": PASSWORD, "password_confirmation": PASSWORD
                }))
                .unwrap(),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        cookie_of(&headers)
    }

    async fn guest(&self) -> String {
        let (status, headers, body) = self
            .raw("POST", "/api/auth/guest", None, b"{}".to_vec())
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        cookie_of(&headers)
    }

    /// A small complete plan; returns its scenario id.
    async fn seed_scenario(&self, cookie: &str) -> i64 {
        let (_, profiles) = self
            .call("GET", "/api/return-profiles", Some(cookie), Value::Null)
            .await;
        let profile = |name: &str| {
            profiles
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["name"] == name)
                .unwrap()["id"]
                .as_i64()
                .unwrap()
        };
        let (equity, cash) = (profile("US Total Market"), profile("Savings Account"));
        let (status, scenario) = self
            .call(
                "POST",
                "/api/scenarios",
                Some(cookie),
                json!({"name": "Offload plan", "start_date": "2026-01-01",
                       "birth_date": "1985-01-01", "duration_years": 10}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{scenario}");
        let id = scenario["id"].as_i64().unwrap();
        let (_, asset) = self
            .call(
                "POST",
                &format!("/api/scenarios/{id}/assets"),
                Some(cookie),
                json!({"name": "VTI", "initial_price": 100.0, "return_profile_id": equity}),
            )
            .await;
        let (_, checking) = self
            .call(
                "POST",
                &format!("/api/scenarios/{id}/accounts"),
                Some(cookie),
                json!({"name": "Checking", "flavor": "Bank",
                       "cash_value": 10_000.0, "return_profile_id": cash}),
            )
            .await;
        let _ = checking;
        let (_, brokerage) = self
            .call(
                "POST",
                &format!("/api/scenarios/{id}/accounts"),
                Some(cookie),
                json!({"name": "Brokerage", "flavor": "Investment", "tax_status": "Taxable",
                       "cash_value": 0.0, "cash_return_profile_id": cash}),
            )
            .await;
        let (status, _) = self
            .call(
                "POST",
                &format!(
                    "/api/scenarios/{id}/accounts/{}/positions",
                    brokerage["id"].as_i64().unwrap()
                ),
                Some(cookie),
                json!({"asset_id": asset["id"].as_i64().unwrap(),
                       "units": 500.0, "cost_basis": 40_000.0}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        id
    }

    /// The plan's snapshot and model version, as a local plan would hold them,
    /// taken from a stored run's inputs.
    async fn snapshot_of(&self, cookie: &str, scenario: i64) -> (Value, String) {
        let (status, run) = self
            .call(
                "POST",
                &format!("/api/scenarios/{scenario}/runs"),
                Some(cookie),
                json!({"iterations": 1, "seed": 1}),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{run}");
        let run_id = run["id"].as_i64().unwrap();
        self.await_run(cookie, run_id).await;
        let (_, inputs) = self
            .call(
                "GET",
                &format!("/api/runs/{run_id}/inputs"),
                Some(cookie),
                Value::Null,
            )
            .await;
        (
            inputs["snapshot"].clone(),
            inputs["model_version"].as_str().unwrap().to_owned(),
        )
    }

    async fn await_run(&self, cookie: &str, id: i64) -> String {
        for _ in 0..300 {
            let (_, run) = self
                .call("GET", &format!("/api/runs/{id}"), Some(cookie), Value::Null)
                .await;
            let status = run["status"].as_str().unwrap_or_default().to_owned();
            if matches!(status.as_str(), "succeeded" | "failed" | "canceled") {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("run {id} never finished");
    }

    async fn offload(
        &self,
        cookie: &str,
        snapshot: &Value,
        version: &str,
        settings: Value,
    ) -> (StatusCode, Value) {
        self.call(
            "POST",
            "/api/compute/runs",
            Some(cookie),
            json!({"snapshot": snapshot, "model_version": version, "settings": settings}),
        )
        .await
    }

    /// Poll an offloaded job until it ends, and return its final body.
    async fn await_job(&self, cookie: &str, id: i64) -> Value {
        for _ in 0..600 {
            let (status, job) = self
                .call(
                    "GET",
                    &format!("/api/compute/runs/{id}"),
                    Some(cookie),
                    Value::Null,
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{job}");
            if matches!(
                job["status"].as_str().unwrap(),
                "succeeded" | "failed" | "canceled"
            ) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("job {id} never finished");
    }

    async fn budget(&self, cookie: &str) -> Value {
        let (status, body) = self
            .call("GET", "/api/compute/budget", Some(cookie), Value::Null)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
}

fn cookie_of(headers: &HeaderMap) -> String {
    let value = headers.get("set-cookie").unwrap().to_str().unwrap();
    value.split(';').next().unwrap().to_owned()
}

fn message(body: &Value) -> &str {
    body["error"]["message"].as_str().unwrap_or_default()
}

// ── health and preferences ──────────────────────────────────────────────────

#[tokio::test]
async fn health_says_whether_local_mode_is_on() {
    let on = App::plain(1).await;
    let (status, health) = on.call("GET", "/api/health", None, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["status"], "ok");
    assert_eq!(health["local_mode"], true);
    assert_eq!(
        health["model_version"],
        finplan_plan::snapshot::MODEL_VERSION
    );

    let mut off = config(false);
    off.local_mode = false;
    let off = App::new(off, 2).await;
    let (_, health) = off.call("GET", "/api/health", None, Value::Null).await;
    assert_eq!(health["local_mode"], false);
}

#[tokio::test]
async fn default_plan_home_starts_local_and_round_trips() {
    let app = App::plain(3).await;
    let cookie = app.register("home@example.com").await;
    let (_, me) = app
        .call("GET", "/api/auth/me", Some(&cookie), Value::Null)
        .await;
    assert_eq!(me["default_plan_home"], "local");

    let prefs = |home: Value| {
        let mut body = json!({
            "default_iterations": 500, "default_duration_years": 40, "auto_run": true
        });
        if !home.is_null() {
            body["default_plan_home"] = home;
        }
        body
    };
    let (status, user) = app
        .call(
            "PUT",
            "/api/auth/preferences",
            Some(&cookie),
            prefs(json!("cloud")),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{user}");
    assert_eq!(user["default_plan_home"], "cloud");
    let (_, me) = app
        .call("GET", "/api/auth/me", Some(&cookie), Value::Null)
        .await;
    assert_eq!(me["default_plan_home"], "cloud");

    // A client that does not send it leaves it alone.
    let (status, user) = app
        .call(
            "PUT",
            "/api/auth/preferences",
            Some(&cookie),
            prefs(Value::Null),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{user}");
    assert_eq!(user["default_plan_home"], "cloud");
    assert_eq!(user["default_iterations"], 500);

    let (status, error) = app
        .call(
            "PUT",
            "/api/auth/preferences",
            Some(&cookie),
            prefs(json!("mars")),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    let (_, user) = app
        .call(
            "PUT",
            "/api/auth/preferences",
            Some(&cookie),
            prefs(json!("local")),
        )
        .await;
    assert_eq!(user["default_plan_home"], "local");
}

// ── offload ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn an_offloaded_run_matches_a_stored_run_of_the_same_graph_and_seed() {
    let app = App::plain(4).await;
    let cookie = app.register("offload@example.com").await;
    let scenario = app.seed_scenario(&cookie).await;
    let (snapshot, version) = app.snapshot_of(&cookie, scenario).await;

    let (status, stored) = app
        .call(
            "POST",
            &format!("/api/scenarios/{scenario}/runs"),
            Some(&cookie),
            json!({"iterations": 200, "seed": 42}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{stored}");
    let stored_id = stored["id"].as_i64().unwrap();
    assert_eq!(app.await_run(&cookie, stored_id).await, "succeeded");
    let (_, stored_results) = app
        .call(
            "GET",
            &format!("/api/runs/{stored_id}/results"),
            Some(&cookie),
            Value::Null,
        )
        .await;

    let scenarios_before = count(&app, "scenarios").await;
    let runs_before = count(&app, "runs").await;

    let (status, created) = app
        .offload(
            &cookie,
            &snapshot,
            &version,
            json!({"iterations": 200, "seed": 42}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let id = created["id"].as_i64().unwrap();

    let job = app.await_job(&cookie, id).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert_eq!(job["iterations"], 200);
    assert_eq!(job["completed_iterations"], 200);
    assert_eq!(job["seed"], 42);
    assert!(job["error"].is_null());
    let offloaded = &job["results"];

    // The same shaping path, the same engine settings: the figures agree
    // exactly, not just statistically.
    assert_eq!(offloaded["stats"], stored_results["stats"]);
    assert_eq!(offloaded["stats"]["num_iterations"], 200);
    assert_eq!(
        offloaded["real_net_worth"].is_null(),
        stored_results["real_net_worth"].is_null()
    );
    let bands = stored_results["bands"].as_array().unwrap();
    let paths = offloaded["paths"].as_array().unwrap();
    assert_eq!(bands.len(), paths.len());
    for path in paths {
        let band = bands
            .iter()
            .find(|band| band["percentile"] == path["percentile"])
            .expect("a stored band for every offloaded path");
        let points: Vec<f64> = path["net_worth"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["net_worth"].as_f64().unwrap())
            .collect();
        let expected: Vec<f64> = band["net_worth"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(points, expected);
    }

    // No scenario and no run row was written, and the plan was not kept.
    assert_eq!(count(&app, "scenarios").await, scenarios_before);
    assert_eq!(count(&app, "runs").await, runs_before);
    let stored_plan: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM compute_jobs WHERE result_json LIKE '%Offload plan%'",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(stored_plan, 0, "results do not carry the plan's name");

    // The charge: 200 iterations x 10 years x (2 accounts + 1 asset).
    let budget = app.budget(&cookie).await;
    assert_eq!(budget["used"], 6000);
    assert_eq!(
        budget["remaining"],
        budget["monthly"].as_i64().unwrap() - 6000
    );

    // Results expire an hour after completion.
    let (expires_in_minutes,): (f64,) = sqlx::query_as(
        "SELECT (julianday(expires_at) - julianday('now')) * 1440 FROM compute_jobs WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert!(
        (55.0..=61.0).contains(&expires_in_minutes),
        "{expires_in_minutes}"
    );
}

#[tokio::test]
async fn a_snapshot_may_arrive_as_its_json_text_and_a_random_seed_is_reported() {
    let app = App::plain(5).await;
    let cookie = app.register("text@example.com").await;
    let scenario = app.seed_scenario(&cookie).await;
    let (snapshot, version) = app.snapshot_of(&cookie, scenario).await;

    let as_text = Value::String(snapshot.to_string());
    let (status, created) = app
        .offload(&cookie, &as_text, &version, json!({"iterations": 20}))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    let job = app
        .await_job(&cookie, created["id"].as_i64().unwrap())
        .await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert!(job["seed"].is_i64());
}

#[tokio::test]
async fn a_stale_model_version_is_refused_with_a_reload_message() {
    let app = App::plain(6).await;
    let cookie = app.register("stale@example.com").await;
    let (status, error) = app
        .offload(
            &cookie,
            &json!({"anything": true}),
            "finplan-0.0.1",
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(message(&error), "Reload FinPlan to update");
    assert_eq!(count(&app, "compute_jobs").await, 0);
}

#[tokio::test]
async fn an_oversized_body_is_refused_before_it_is_read() {
    let app = App::plain(7).await;
    let cookie = app.register("big@example.com").await;
    let padding = "x".repeat(2 * 1024 * 1024 + 1);
    let body = serde_json::to_vec(&json!({
        "snapshot": {"pad": padding},
        "model_version": finplan_plan::snapshot::MODEL_VERSION,
    }))
    .unwrap();
    let (status, _, error) = app
        .raw("POST", "/api/compute/runs", Some(&cookie), body)
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{error}");
    assert_eq!(error["error"]["code"], "payload_too_large");
}

#[tokio::test]
async fn bad_requests_are_refused_without_echoing_the_plan() {
    let app = App::plain(8).await;
    let cookie = app.register("bad@example.com").await;
    let scenario = app.seed_scenario(&cookie).await;
    let (snapshot, version) = app.snapshot_of(&cookie, scenario).await;

    let (status, error) = app
        .offload(&cookie, &snapshot, &version, json!({"iterations": 0}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let (status, error) = app
        .offload(&cookie, &snapshot, &version, json!({"percentiles": []}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let (status, error) = app
        .offload(&cookie, &snapshot, &version, json!({"iterations": 50_001}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    let (status, error) = app
        .offload(
            &cookie,
            &json!({"scenario": "Secret plan name"}),
            &version,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert!(!error.to_string().contains("Secret plan name"));

    let (status, _, error) = app
        .raw(
            "POST",
            "/api/compute/runs",
            Some(&cookie),
            b"not json".to_vec(),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    // Nothing was charged for any of it.
    assert_eq!(app.budget(&cookie).await["used"], 0);
}

#[tokio::test]
async fn offload_needs_a_signed_in_account_not_a_guest() {
    let app = App::plain(9).await;
    let body = json!({"snapshot": {}, "model_version": finplan_plan::snapshot::MODEL_VERSION});

    let (status, _) = app
        .call("POST", "/api/compute/runs", None, body.clone())
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = app
        .call("GET", "/api/compute/budget", None, Value::Null)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let guest = app.guest().await;
    let (status, error) = app
        .call("POST", "/api/compute/runs", Some(&guest), body)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    assert!(message(&error).contains("account"), "{error}");

    let budget = app.budget(&guest).await;
    assert_eq!(budget["available"], false);
    assert_eq!(budget["unavailable_reason"], "guest");
    assert_eq!(budget["monthly"], 0);
}

#[tokio::test]
async fn a_spent_budget_refuses_with_its_reset_date() {
    let mut small = config(false);
    small.offload.budget_pro = 10_000;
    let app = App::new(small, 10).await;
    let cookie = app.register("budget@example.com").await;
    let scenario = app.seed_scenario(&cookie).await;
    let (snapshot, version) = app.snapshot_of(&cookie, scenario).await;

    let budget = app.budget(&cookie).await;
    assert_eq!(budget["monthly"], 10_000);
    assert_eq!(budget["remaining"], 10_000);
    assert_eq!(budget["available"], true);
    let resets_at = budget["resets_at"].as_str().unwrap();
    assert!(resets_at.ends_with("-01T00:00:00Z"), "{resets_at}");

    // 200 x 10 x 3 = 6,000 units: one fits, the second does not.
    let (status, first) = app
        .offload(&cookie, &snapshot, &version, json!({"iterations": 200}))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let first_id = first["id"].as_i64().unwrap();
    let (status, error) = app
        .offload(&cookie, &snapshot, &version, json!({"iterations": 200}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    assert_eq!(error["error"]["code"], "offload_budget_spent");
    let reset_day = &resets_at[..10];
    assert!(message(&error).contains(reset_day), "{error}");

    let budget = app.budget(&cookie).await;
    assert_eq!(budget["used"], 6000);
    assert_eq!(budget["remaining"], 4000);

    // A run bigger than the whole month's budget says so, not "spent".
    let (status, error) = app
        .offload(&cookie, &snapshot, &version, json!({"iterations": 1000}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    assert_eq!(error["error"]["code"], "offload_run_exceeds_budget");

    // Deleting a job that ran does not give its budget back.
    app.await_job(&cookie, first_id).await;
    let (status, _) = app
        .call(
            "DELETE",
            &format!("/api/compute/runs/{first_id}"),
            Some(&cookie),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.budget(&cookie).await["used"], 6000);
}

#[tokio::test]
async fn a_free_account_has_the_small_budget_and_a_pro_account_the_large_one() {
    let app = App::new(config(true), 11).await;
    let (_, headers, _) = app
        .raw(
            "POST",
            "/api/auth/register",
            None,
            serde_json::to_vec(&json!({
                "email": "free@example.com", "password": PASSWORD,
                "password_confirmation": PASSWORD
            }))
            .unwrap(),
        )
        .await;
    let cookie = cookie_of(&headers);
    let budget = app.budget(&cookie).await;
    assert_eq!(budget["monthly"], 20_000_000);
    assert_eq!(budget["available"], true);

    let mut beta = config(true);
    beta.access_mode = finplan_server::config::HostedAccessMode::Beta;
    let app = App::new(beta, 12).await;
    let (_, headers, _) = app
        .raw(
            "POST",
            "/api/auth/register",
            None,
            serde_json::to_vec(&json!({
                "email": "pro@example.com", "password": PASSWORD,
                "password_confirmation": PASSWORD
            }))
            .unwrap(),
        )
        .await;
    let cookie = cookie_of(&headers);
    assert_eq!(app.budget(&cookie).await["monthly"], 1_000_000_000_i64);
}

#[tokio::test]
async fn jobs_belong_to_the_user_who_started_them() {
    let app = App::plain(13).await;
    let owner = app.register("owner@example.com").await;
    let other = app.register("other@example.com").await;
    let scenario = app.seed_scenario(&owner).await;
    let (snapshot, version) = app.snapshot_of(&owner, scenario).await;

    let (_, created) = app
        .offload(&owner, &snapshot, &version, json!({"iterations": 20}))
        .await;
    let id = created["id"].as_i64().unwrap();
    app.await_job(&owner, id).await;

    let path = format!("/api/compute/runs/{id}");
    let (status, _) = app.call("GET", &path, Some(&other), Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.call("DELETE", &path, Some(&other), Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Still there for the owner.
    let (status, _) = app.call("GET", &path, Some(&owner), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = app
        .call("GET", "/api/compute/runs/999999", Some(&owner), Value::Null)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn canceling_stops_a_running_job_and_refunds_one_that_never_started() {
    // One worker slot: the first job runs, the second waits behind it.
    let app = App::plain(14).await;
    let cookie = app.register("cancel@example.com").await;
    let scenario = app.seed_scenario(&cookie).await;
    let (snapshot, version) = app.snapshot_of(&cookie, scenario).await;

    let big = json!({"iterations": 50_000, "seed": 1});
    let (status, first) = app.offload(&cookie, &snapshot, &version, big.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let first_id = first["id"].as_i64().unwrap();
    // Wait until it is actually running, so the second is queued behind it.
    for _ in 0..200 {
        let (_, job) = app
            .call(
                "GET",
                &format!("/api/compute/runs/{first_id}"),
                Some(&cookie),
                Value::Null,
            )
            .await;
        if job["status"] == "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let (status, second) = app.offload(&cookie, &snapshot, &version, big).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    let second_id = second["id"].as_i64().unwrap();
    let (_, job) = app
        .call(
            "GET",
            &format!("/api/compute/runs/{second_id}"),
            Some(&cookie),
            Value::Null,
        )
        .await;
    assert_eq!(job["status"], "queued");
    let big_cost = 50_000 * 10 * 3;
    assert_eq!(app.budget(&cookie).await["used"], 2 * big_cost);

    // Canceling the queued one refunds it.
    let (status, _) = app
        .call(
            "DELETE",
            &format!("/api/compute/runs/{second_id}"),
            Some(&cookie),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.budget(&cookie).await["used"], big_cost);
    let (status, _) = app
        .call(
            "GET",
            &format!("/api/compute/runs/{second_id}"),
            Some(&cookie),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Canceling the running one stops it and frees the worker; a small job
    // then runs to completion.
    let (status, _) = app
        .call(
            "DELETE",
            &format!("/api/compute/runs/{first_id}"),
            Some(&cookie),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.budget(&cookie).await["used"],
        big_cost,
        "no refund once it ran"
    );
    // The canceled queued job's admission is released by its own task, and the
    // running one holds its until the engine notices (as a stored run does), so
    // capacity comes back shortly rather than instantly: retry while busy.
    let third = {
        let mut attempt = 0;
        loop {
            let (status, body) = app
                .offload(&cookie, &snapshot, &version, json!({"iterations": 20}))
                .await;
            if status == StatusCode::ACCEPTED {
                break body;
            }
            assert_eq!(status, StatusCode::CONFLICT, "{body}");
            assert!(message(&body).contains("capacity"), "{body}");
            attempt += 1;
            assert!(attempt < 200, "capacity never came back after cancel");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let job = app.await_job(&cookie, third["id"].as_i64().unwrap()).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert_eq!(count(&app, "compute_jobs").await, 1);
}

#[tokio::test]
async fn expired_results_are_gone_and_the_purge_deletes_them() {
    let app = App::plain(15).await;
    let cookie = app.register("expiry@example.com").await;
    let scenario = app.seed_scenario(&cookie).await;
    let (snapshot, version) = app.snapshot_of(&cookie, scenario).await;

    let (_, created) = app
        .offload(&cookie, &snapshot, &version, json!({"iterations": 20}))
        .await;
    let id = created["id"].as_i64().unwrap();
    app.await_job(&cookie, id).await;

    // Not yet expired: the purge leaves it.
    assert_eq!(
        finplan_server::offload::purge_expired(&app.state.db)
            .await
            .unwrap(),
        0
    );

    sqlx::query("UPDATE compute_jobs SET expires_at = datetime('now', '-1 minute')")
        .execute(&app.state.db)
        .await
        .unwrap();
    // Gone to the owner even before the maintenance loop gets to it...
    let (status, _) = app
        .call(
            "GET",
            &format!("/api/compute/runs/{id}"),
            Some(&cookie),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(count(&app, "compute_jobs").await, 1);
    // ...and then deleted.
    assert_eq!(
        finplan_server::offload::purge_expired(&app.state.db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(count(&app, "compute_jobs").await, 0);
    // The spend stays: purging results does not refund them.
    assert_eq!(app.budget(&cookie).await["used"], 20 * 10 * 3);
}

#[tokio::test]
async fn jobs_orphaned_by_a_restart_fail_and_are_refunded() {
    let app = App::plain(16).await;
    let cookie = app.register("orphan@example.com").await;
    let (_, me) = app
        .call("GET", "/api/auth/me", Some(&cookie), Value::Null)
        .await;
    let user = me["id"].as_str().unwrap();
    sqlx::query(
        "INSERT INTO monthly_offload_spend (user_id, month, used)
         VALUES (?1, strftime('%Y-%m', 'now'), 500)",
    )
    .bind(user)
    .execute(&app.state.db)
    .await
    .unwrap();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO compute_jobs (user_id, status, iterations, seed, cost, month)
         VALUES (?1, 'running', 10, 1, 500, strftime('%Y-%m', 'now')) RETURNING id",
    )
    .bind(user)
    .fetch_one(&app.state.db)
    .await
    .unwrap();

    assert_eq!(
        finplan_server::offload::fail_orphans(&app.state.db)
            .await
            .unwrap(),
        1
    );
    let (status, job) = app
        .call(
            "GET",
            &format!("/api/compute/runs/{id}"),
            Some(&cookie),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(job["status"], "failed");
    assert!(message(&json!({"error": {"message": job["error"]}})).contains("restarted"));
    assert!(job["results"].is_null());
    assert_eq!(app.budget(&cookie).await["used"], 0);
}

async fn count(app: &App, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(&app.state.db)
        .await
        .unwrap()
}
