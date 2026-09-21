//! Verify the request, mutation, and worker instrumentation together.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use finplan_server::config::{LogFormat, ServerConfig};
use finplan_server::state::AppState;
use serde_json::{Value, json};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

struct Fixture {
    router: Router,
    state: AppState,
    cookie: String,
    user_id: String,
}

impl Fixture {
    async fn new() -> Self {
        let config = ServerConfig {
            hosted: false,
            local_mail_sink: None,
            bind: "127.0.0.1:0".into(),
            database_url: "sqlite::memory:".into(),
            db_pool_size: 1,
            sim_workers: 1,
            max_iterations: 50_000,
            secure_cookies: false,
            cors_origins: vec!["http://localhost:3000".into()],
            log_format: LogFormat::Json,
            metrics_bind: None,
        };
        let (router, state) = finplan_server::build(config).await.unwrap();
        let user_id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES (?,'private-observability-email@example.com','private-password-hash')")
            .bind(&user_id).execute(&state.db).await.unwrap();
        finplan_server::seed::seed_user_library(&state.db, &user_id)
            .await
            .unwrap();
        let token = finplan_server::auth::session::issue(&state.db, &user_id, None)
            .await
            .unwrap();
        Self {
            router,
            state,
            cookie: format!("finplan_session={token}"),
            user_id,
        }
    }

    async fn send(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value, String) {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("cookie", &self.cookie)
                    .header("authorization", "Bearer private-authorization-value")
                    .header("x-request-id", "untrusted-client-request-id")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let request_id = response.headers()["x-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(uuid::Uuid::parse_str(&request_id).is_ok());
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value, request_id)
    }

    async fn scenario(&self) -> i64 {
        let (status, scenario, _) = self
            .send(
                "POST",
                "/api/scenarios",
                json!({"name":"private-plan-name", "start_date":"2026-01-01", "duration_years":1}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{scenario}");
        scenario["id"].as_i64().unwrap()
    }
}

fn sample(exposition: &str, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
    exposition.lines().find_map(|line| {
        if line.split(['{', ' ']).next() != Some(name)
            || !labels
                .iter()
                .all(|(key, value)| line.contains(&format!("{key}=\"{value}\"")))
        {
            return None;
        }
        line.split_whitespace().last()?.parse().ok()
    })
}

fn contains_string(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(value) => value == needle,
        Value::Array(values) => values.iter().any(|value| contains_string(value, needle)),
        Value::Object(values) => values.values().any(|value| contains_string(value, needle)),
        _ => false,
    }
}

#[tokio::test]
async fn activity_and_worker_logs_share_request_context_without_financial_payloads() {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_env_filter("finplan_server=trace,warn")
        .with_writer(move || writer.clone())
        .finish();

    async {
        let app = Fixture::new().await;
        let scenario_id = app.scenario().await;
        let profile_id: i64 = sqlx::query_scalar(
            "SELECT id FROM return_profiles WHERE user_id=? AND name='Savings Account'",
        )
        .bind(&app.user_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        let collection = format!("/api/scenarios/{scenario_id}/accounts");
        let (status, account, account_request_id) = app
            .send(
                "POST",
                &collection,
                json!({
                    "name":"private-account-name", "flavor":"Bank", "cash_value":8412345.67,
                    "return_profile_id":profile_id,
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{account}");
        let account_path = format!("{collection}/{}", account["id"]);
        assert_eq!(
            app.send(
                "PATCH",
                &account_path,
                json!({"name":"private-renamed-account"})
            )
            .await
            .0,
            StatusCode::OK
        );

        let (status, run, run_request_id) = app
            .send(
                "POST",
                &format!("/api/scenarios/{scenario_id}/runs"),
                json!({"iterations":12,"seed":31}),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{run}");
        assert_ne!(account_request_id, run_request_id);
        let run_path = format!("/api/runs/{}", run["id"]);
        let mut finished = false;
        for _ in 0..200 {
            let (_, current, _) = app.send("GET", &run_path, Value::Null).await;
            assert_ne!(current["status"], "failed", "{current}");
            let metrics = app.state.telemetry.encode().unwrap();
            if current["status"] == "succeeded"
                && sample(
                    &metrics,
                    "finplan_job_attempts_total",
                    &[("kind", "run"), ("outcome", "succeeded")],
                ) == Some(1.0)
            {
                finished = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(finished, "successful run telemetry never settled");
        assert_eq!(
            app.send("DELETE", &account_path, Value::Null).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            app.send("GET", "/api/health?token=private-query-token", Value::Null)
                .await
                .0,
            StatusCode::OK
        );

        let metrics = app.state.telemetry.encode().unwrap();
        assert_eq!(
            sample(&metrics, "finplan_run_iterations_completed_total", &[]),
            Some(12.0)
        );
        assert_eq!(
            sample(&metrics, "finplan_jobs_running", &[("kind", "run")]),
            Some(0.0)
        );
        assert_eq!(
            sample(
                &metrics,
                "finplan_job_processing_duration_seconds_count",
                &[("kind", "run"), ("outcome", "succeeded")]
            ),
            Some(1.0)
        );
        assert_eq!(
            sample(&metrics, "finplan_run_iterations_per_second_count", &[]),
            Some(1.0)
        );
        assert!(metrics.contains("route=\"/api/runs/{id}\""));
        assert!(!metrics.contains(&format!("route=\"{run_path}\"")));
        assert!(!metrics.contains("request_id="));
        assert!(!metrics.contains("user_id="));

        let logs = capture.text();
        let entries: Vec<Value> = logs
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for (event, request_id) in [
            ("account.created", &account_request_id),
            ("run.submitted", &run_request_id),
            ("run.started", &run_request_id),
            ("run.succeeded", &run_request_id),
        ] {
            assert!(
                entries.iter().any(
                    |entry| contains_string(entry, event) && contains_string(entry, request_id)
                ),
                "missing correlated {event}: {logs}"
            );
        }
        for private in [
            "private-observability-email",
            "private-password-hash",
            "private-plan-name",
            "private-account-name",
            "private-renamed-account",
            "8412345.67",
            "private-query-token",
            "private-authorization-value",
            "untrusted-client-request-id",
            &app.cookie,
        ] {
            assert!(
                !logs.contains(private),
                "private data leaked into logs: {private}"
            );
            assert!(
                !metrics.contains(private),
                "private data leaked into metrics: {private}"
            );
        }
    }
    .with_subscriber(subscriber)
    .await;
}

#[tokio::test]
async fn authentication_database_failure_is_counted_once_and_keeps_request_correlation() {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_env_filter("finplan_server=trace,warn")
        .with_writer(move || writer.clone())
        .finish();

    async {
        let app = Fixture::new().await;
        for path in ["/metrics", "/api/metrics"] {
            assert_eq!(
                app.send("GET", path, Value::Null).await.0,
                StatusCode::NOT_FOUND
            );
        }
        app.state.db.close().await;
        let (status, body, request_id) = app
            .send(
                "GET",
                "/api/scenarios?token=private-error-query-token",
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["message"], "internal server error");
        let metrics = app.state.telemetry.encode().unwrap();
        assert_eq!(
            sample(
                &metrics,
                "finplan_server_errors_total",
                &[("component", "http"), ("class", "database")]
            ),
            Some(1.0)
        );
        assert_eq!(
            sample(&metrics, "finplan_http_requests_in_flight", &[]),
            Some(0.0)
        );
        let logs = capture.text();
        let failures: Vec<Value> = logs
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|entry| contains_string(entry, "request.failed"))
            .collect();
        assert_eq!(failures.len(), 1);
        assert!(contains_string(&failures[0], &request_id));
        assert!(contains_string(&failures[0], "database"));
        assert!(!logs.contains("private-error-query-token"));
        assert!(!logs.contains(&app.cookie));
    }
    .with_subscriber(subscriber)
    .await;
}

#[tokio::test]
async fn failed_run_commit_releases_admission_and_never_counts_as_accepted() {
    let app = Fixture::new().await;
    let scenario_id = app.scenario().await;
    sqlx::query("CREATE TRIGGER reject_test_run BEFORE INSERT ON runs BEGIN SELECT RAISE(ABORT, 'injected run insert failure'); END")
        .execute(&app.state.db).await.unwrap();
    let (status, _, _) = app
        .send(
            "POST",
            &format!("/api/scenarios/{scenario_id}/runs"),
            json!({"iterations":12, "seed":31}),
        )
        .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let retained: i64 = sqlx::query_scalar("SELECT count(*) FROM runs")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(retained, 0);
    let metrics = app.state.telemetry.encode().unwrap();
    assert_eq!(
        sample(
            &metrics,
            "finplan_job_submissions_total",
            &[("kind", "run"), ("result", "internal_error")]
        ),
        Some(1.0)
    );
    assert_eq!(
        sample(
            &metrics,
            "finplan_job_submissions_total",
            &[("kind", "run"), ("result", "accepted")]
        )
        .unwrap_or(0.0),
        0.0
    );
    // Both per-user permits remain available after the transaction failed.
    let first = finplan_server::billing::admit_compute(&app.user_id).unwrap();
    let second = finplan_server::billing::admit_compute(&app.user_id).unwrap();
    drop((first, second));
    sqlx::query("DROP TRIGGER reject_test_run")
        .execute(&app.state.db)
        .await
        .unwrap();
    let (status, _, _) = app
        .send(
            "POST",
            &format!("/api/scenarios/{scenario_id}/runs"),
            json!({"iterations":12, "seed":31}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
}
