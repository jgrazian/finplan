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

use finplan_server::suggest::ai::{
    AiClient, BoxFuture, DEFAULT_MODEL, Reply, Request as ModelRequest, Settings, ThinkingMode,
    Transport, TransportError,
};

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
        Self::with_review_ai(None).await
    }

    /// With `review_ai` (a scripted model client) as the review model.
    async fn with_review_ai(review_ai: Option<Arc<AiClient>>) -> Self {
        let config = ServerConfig {
            mail: Default::default(),
            review_ai: Default::default(),
            draft: Default::default(),
            plan_chat: Default::default(),
            hosted: false,
            access_mode: Default::default(),
            registration_open: true,
            guest_access: true,
            guest_max_iterations: 100,
            guest_retention_days: 30,
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
        let (router, state) = finplan_server::build_with(config, review_ai).await.unwrap();
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

/// A review model that plays back scripted Messages replies.
struct ScriptedModel(Mutex<std::collections::VecDeque<Value>>);

impl Transport for ScriptedModel {
    fn send<'a>(
        &'a self,
        _request: &'a ModelRequest,
    ) -> BoxFuture<'a, Result<Reply, TransportError>> {
        Box::pin(async move {
            let next = self.0.lock().unwrap().pop_front();
            let reply = next.ok_or_else(|| TransportError::Network("script ran out".into()))?;
            Ok(serde_json::from_value(reply).unwrap())
        })
    }
}

fn model_reply(stop_reason: &str, content: Value, cost: f64) -> Value {
    json!({
        "id": "msg", "type": "message", "role": "assistant", "model": DEFAULT_MODEL,
        "stop_reason": stop_reason, "content": content,
        "usage": {"input_tokens": 1000, "output_tokens": 200,
                  "cache_read_input_tokens": 300, "cache_creation_input_tokens": 40,
                  "cost": cost}
    })
}

fn tool_call(id: &str, tool: &str, input: Value) -> Value {
    json!([{"type": "tool_use", "id": id, "name": tool, "input": input}])
}

#[tokio::test]
async fn a_review_ai_pass_is_traced_to_its_request_and_metered() {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_env_filter("finplan_server=trace,warn")
        .with_writer(move || writer.clone())
        .finish();

    async {
        let script = Arc::new(ScriptedModel(Mutex::default()));
        let settings = Settings {
            model: DEFAULT_MODEL.into(),
            max_turns: 6,
            max_suggestions: 3,
            max_previews: 3,
            max_tokens: 2_000,
            thinking: ThinkingMode::On,
            effort: "high",
            max_retries: 0,
            retry_base: Duration::from_millis(1),
            retry_cap: Duration::from_millis(1),
            materiality: Default::default(),
        };
        let client = Arc::new(AiClient::new(settings, script.clone(), None));
        let app = Fixture::with_review_ai(Some(client)).await;
        let scenario_id = app.scenario().await;
        let profile_id: i64 = sqlx::query_scalar(
            "SELECT id FROM return_profiles WHERE user_id=? AND name='Savings Account'",
        )
        .bind(&app.user_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        let (status, account, _) = app
            .send(
                "POST",
                &format!("/api/scenarios/{scenario_id}/accounts"),
                json!({"name":"private-account-name", "flavor":"Bank", "cash_value":8412345.67,
                       "return_profile_id":profile_id}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{account}");
        let (status, run, _) = app
            .send(
                "POST",
                &format!("/api/scenarios/{scenario_id}/runs"),
                json!({"iterations":12,"seed":31}),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{run}");
        let run_path = format!("/api/runs/{}", run["id"]);
        for _ in 0..200 {
            let (_, current, _) = app.send("GET", &run_path, Value::Null).await;
            if current["status"] == "succeeded" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // Preview a change, submit it as a note, end: three turns, each
        // with OpenRouter's reported cost.
        let changes = json!([{
            "op": "replace", "target": {"account": account["id"]}, "path": "/cash_value",
            "expect": 8412345.67, "value": 9000000.0
        }]);
        script.0.lock().unwrap().extend([
            model_reply(
                "tool_use",
                tool_call("t1", "preview_changes", json!({"changes": changes})),
                0.25,
            ),
            model_reply(
                "tool_use",
                tool_call(
                    "t2",
                    "submit_suggestion",
                    json!({"kind": "fix", "section": "portfolio", "motive": "realism",
                           "title": "private-model-title", "summary": "The one-line lead.", "reasoning": "private-model-reasoning",
                           "evidence": [],
                           "paths": [{"key": "a", "label": "private-model-label",
                                      "recommended": true,
                                      "steps": [{"key": "a", "title": "private-model-step",
                                                 "changes": changes}]}]}),
                ),
                0.5,
            ),
            model_reply(
                "end_turn",
                json!([{"type": "text", "text": "private-model-text"}]),
                0.125,
            ),
        ]);

        let (status, review, review_request_id) = app
            .send(
                "POST",
                &format!("/api/scenarios/{scenario_id}/review"),
                json!({}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{review}");
        assert_eq!(review["ai"]["status"], "running", "{review}");

        let mut review = Value::Null;
        for _ in 0..300 {
            let (_, current, _) = app
                .send(
                    "GET",
                    &format!("/api/scenarios/{scenario_id}/review"),
                    Value::Null,
                )
                .await;
            if current["ai"]["status"] != "running" {
                review = current;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(review["ai"]["status"], "done", "{review}");
        assert_eq!(review["ai"]["stop"], "finished");

        // The review row names the request that started the pass, and what
        // the pass spent.
        let (request_id, turns, input, output, cost): (String, i64, i64, i64, f64) =
            sqlx::query_as(
                "SELECT ai_request_id, ai_turns, ai_input_tokens, ai_output_tokens, ai_cost_usd
                   FROM suggestion_reviews WHERE scenario_id = ?",
            )
            .bind(scenario_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        assert_eq!(request_id, review_request_id);
        assert_eq!((turns, input, output), (3, 3000, 600));
        assert!((cost - 0.875).abs() < 1e-9, "{cost}");

        // The job telemetry has settled once the attempt is recorded.
        let mut metrics = String::new();
        for _ in 0..200 {
            metrics = app.state.telemetry.encode().unwrap();
            if sample(
                &metrics,
                "finplan_job_attempts_total",
                &[("kind", "review_ai"), ("outcome", "succeeded")],
            ) == Some(1.0)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let model = ("model", DEFAULT_MODEL);
        for (name, labels, value) in [
            (
                "finplan_job_attempts_total",
                vec![("kind", "review_ai"), ("outcome", "succeeded")],
                1.0,
            ),
            (
                "finplan_job_submissions_total",
                vec![("kind", "review_ai"), ("result", "accepted")],
                1.0,
            ),
            ("finplan_jobs_running", vec![("kind", "review_ai")], 0.0),
            (
                "finplan_review_ai_passes_total",
                vec![model, ("outcome", "finished")],
                1.0,
            ),
            (
                "finplan_review_ai_pass_duration_seconds_count",
                vec![model, ("outcome", "finished")],
                1.0,
            ),
            (
                "finplan_review_ai_turn_duration_seconds_count",
                vec![model],
                3.0,
            ),
            (
                "finplan_review_ai_tokens_total",
                vec![model, ("type", "input")],
                3000.0,
            ),
            (
                "finplan_review_ai_tokens_total",
                vec![model, ("type", "output")],
                600.0,
            ),
            (
                "finplan_review_ai_tokens_total",
                vec![model, ("type", "cache_read")],
                900.0,
            ),
            (
                "finplan_review_ai_tokens_total",
                vec![model, ("type", "cache_write")],
                120.0,
            ),
            (
                "finplan_review_ai_cost_usd_total",
                vec![model, ("source", "reported")],
                0.875,
            ),
            (
                "finplan_review_ai_tool_calls_total",
                vec![("tool", "preview_changes"), ("outcome", "ok")],
                1.0,
            ),
            (
                "finplan_review_ai_tool_calls_total",
                vec![("tool", "submit_suggestion"), ("outcome", "accepted")],
                1.0,
            ),
            (
                "finplan_review_ai_tool_duration_seconds_count",
                vec![("tool", "preview_changes")],
                1.0,
            ),
            (
                "finplan_review_ai_suggestions_total",
                vec![("outcome", "accepted")],
                1.0,
            ),
            (
                "finplan_review_ai_suggestions_total",
                vec![("outcome", "stored")],
                1.0,
            ),
            (
                "finplan_review_ai_suggestions_total",
                vec![("outcome", "discarded")],
                0.0,
            ),
        ] {
            assert_eq!(
                sample(&metrics, name, &labels),
                Some(value),
                "{name} {labels:?}\n{metrics}"
            );
        }

        let logs = capture.text();
        let entries: Vec<Value> = logs
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for event in [
            "review_ai.started",
            "review_ai.turn",
            "review_ai.tool",
            "review_ai.finished",
            "review_ai.stored",
            "review_ai.succeeded",
        ] {
            assert!(
                entries.iter().any(|entry| contains_string(entry, event)
                    && contains_string(entry, &review_request_id)),
                "missing {event} correlated to the review request: {logs}"
            );
        }
        for private in [
            "private-model-title",
            "private-model-reasoning",
            "private-model-label",
            "private-model-step",
            "private-model-text",
            "private-plan-name",
            "private-account-name",
            "8412345.67",
            &app.cookie,
        ] {
            assert!(!logs.contains(private), "{private} leaked into logs");
            assert!(!metrics.contains(private), "{private} leaked into metrics");
        }
        assert!(!metrics.contains("request_id="));
        assert!(!metrics.contains("scenario_id="));
    }
    .with_subscriber(subscriber)
    .await;
}

#[tokio::test]
async fn a_chat_turn_is_traced_to_its_request_and_metered() {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_env_filter("finplan_server=trace,warn")
        .with_writer(move || writer.clone())
        .finish();

    async {
        let script = Arc::new(ScriptedModel(Mutex::default()));
        let settings = Settings {
            model: DEFAULT_MODEL.into(),
            max_turns: 6,
            max_suggestions: 3,
            max_previews: 3,
            max_tokens: 2_000,
            thinking: ThinkingMode::On,
            effort: "high",
            max_retries: 0,
            retry_base: Duration::from_millis(1),
            retry_cap: Duration::from_millis(1),
            materiality: Default::default(),
        };
        let client = Arc::new(AiClient::new(settings, script.clone(), None));
        let app = Fixture::with_review_ai(Some(client)).await;
        let scenario_id = app.scenario().await;
        let profile_id: i64 = sqlx::query_scalar(
            "SELECT id FROM return_profiles WHERE user_id=? AND name='Savings Account'",
        )
        .bind(&app.user_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        let (_, account, _) = app
            .send(
                "POST",
                &format!("/api/scenarios/{scenario_id}/accounts"),
                json!({"name":"private-account-name", "flavor":"Bank", "cash_value":8412345.67,
                       "return_profile_id":profile_id}),
            )
            .await;
        let (_, run, _) = app
            .send(
                "POST",
                &format!("/api/scenarios/{scenario_id}/runs"),
                json!({"iterations":12,"seed":31}),
            )
            .await;
        let run_path = format!("/api/runs/{}", run["id"]);
        for _ in 0..200 {
            let (_, current, _) = app.send("GET", &run_path, Value::Null).await;
            if current["status"] == "succeeded" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // The review pass writes one note to chat about.
        let changes = json!([{
            "op": "replace", "target": {"account": account["id"]}, "path": "/cash_value",
            "expect": 8412345.67, "value": 9000000.0
        }]);
        script.0.lock().unwrap().extend([
            model_reply(
                "tool_use",
                tool_call("t1", "preview_changes", json!({"changes": changes})),
                0.0,
            ),
            model_reply(
                "tool_use",
                tool_call(
                    "t2",
                    "submit_suggestion",
                    json!({"kind": "fix", "section": "portfolio", "motive": "realism",
                           "title": "private-model-title", "summary": "The one-line lead.", "reasoning": "private-model-reasoning",
                           "evidence": [],
                           "paths": [{"key": "a", "label": "private-model-label",
                                      "recommended": true,
                                      "steps": [{"key": "a", "title": "private-model-step",
                                                 "changes": changes}]}]}),
                ),
                0.0,
            ),
            model_reply(
                "end_turn",
                json!([{"type": "text", "text": "private-model-text"}]),
                0.0,
            ),
        ]);
        let (status, _, _) = app
            .send(
                "POST",
                &format!("/api/scenarios/{scenario_id}/review"),
                json!({}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let mut review = Value::Null;
        for _ in 0..300 {
            let (_, current, _) = app
                .send(
                    "GET",
                    &format!("/api/scenarios/{scenario_id}/review"),
                    Value::Null,
                )
                .await;
            if current["ai"]["status"] != "running" {
                review = current;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let note = review["suggestions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["source"] == "ai")
            .unwrap_or_else(|| panic!("no model note: {review}"))["id"]
            .as_i64()
            .unwrap();

        // The chat turn: a preview, then an answer.
        let other = json!([{
            "op": "replace", "target": {"account": account["id"]}, "path": "/cash_value",
            "expect": 8412345.67, "value": 9500000.0
        }]);
        script.0.lock().unwrap().extend([
            model_reply(
                "tool_use",
                tool_call("c1", "preview_changes", json!({"changes": other})),
                0.25,
            ),
            model_reply(
                "end_turn",
                json!([{"type": "text", "text": "private-chat-answer"}]),
                0.125,
            ),
        ]);
        let (status, thread, chat_request_id) = app
            .send(
                "POST",
                &format!("/api/suggestions/{note}/chat"),
                json!({"message": "private-chat-question"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{thread}");
        let mut thread = Value::Null;
        for _ in 0..300 {
            let (_, current, _) = app
                .send("GET", &format!("/api/suggestions/{note}/chat"), Value::Null)
                .await;
            if current["status"] != "running" {
                thread = current;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(thread["status"], "idle", "{thread}");
        assert_eq!(thread["messages"][1]["text"], "private-chat-answer");

        // The thread names the request that started the turn, and its spend.
        let (request_id, turns, input, cost): (String, i64, i64, f64) = sqlx::query_as(
            "SELECT request_id, turns, input_tokens, cost_usd FROM suggestion_threads
              WHERE suggestion_id = ?",
        )
        .bind(note)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        assert_eq!(request_id, chat_request_id);
        assert_eq!((turns, input), (2, 2000));
        assert!((cost - 0.375).abs() < 1e-9, "{cost}");

        let mut metrics = String::new();
        for _ in 0..200 {
            metrics = app.state.telemetry.encode().unwrap();
            if sample(
                &metrics,
                "finplan_job_attempts_total",
                &[("kind", "review_chat"), ("outcome", "succeeded")],
            ) == Some(1.0)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let model = ("model", DEFAULT_MODEL);
        for (name, labels, value) in [
            (
                "finplan_job_attempts_total",
                vec![("kind", "review_chat"), ("outcome", "succeeded")],
                1.0,
            ),
            (
                "finplan_job_submissions_total",
                vec![("kind", "review_chat"), ("result", "accepted")],
                1.0,
            ),
            ("finplan_jobs_running", vec![("kind", "review_chat")], 0.0),
            (
                "finplan_review_chat_turns_total",
                vec![model, ("outcome", "finished")],
                1.0,
            ),
            (
                "finplan_review_chat_turn_duration_seconds_count",
                vec![model, ("outcome", "finished")],
                1.0,
            ),
            (
                "finplan_review_chat_tokens_total",
                vec![model, ("type", "input")],
                2000.0,
            ),
            (
                "finplan_review_chat_tokens_total",
                vec![model, ("type", "cache_read")],
                600.0,
            ),
            (
                "finplan_review_chat_cost_usd_total",
                vec![model, ("source", "reported")],
                0.375,
            ),
            // The review_ai families total both the review pass and the chat.
            (
                "finplan_review_ai_tokens_total",
                vec![model, ("type", "input")],
                5000.0,
            ),
            (
                "finplan_review_ai_cost_usd_total",
                vec![model, ("source", "reported")],
                0.375,
            ),
            (
                "finplan_review_ai_passes_total",
                vec![model, ("outcome", "finished")],
                1.0,
            ),
        ] {
            assert_eq!(
                sample(&metrics, name, &labels),
                Some(value),
                "{name} {labels:?}\n{metrics}"
            );
        }

        let logs = capture.text();
        let entries: Vec<Value> = logs
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for event in [
            "review_chat.accepted",
            "review_chat.model_started",
            "review_ai.turn",
            "review_ai.tool",
            "review_ai.finished",
            "review_chat.stored",
            "review_chat.succeeded",
        ] {
            assert!(
                entries.iter().any(|entry| contains_string(entry, event)
                    && contains_string(entry, &chat_request_id)),
                "missing {event} correlated to the chat request: {logs}"
            );
        }
        for private in [
            "private-chat-question",
            "private-chat-answer",
            "private-model-title",
            "private-account-name",
            "8412345.67",
        ] {
            assert!(!logs.contains(private), "{private} leaked into logs");
            assert!(!metrics.contains(private), "{private} leaked into metrics");
        }
    }
    .with_subscriber(subscriber)
    .await;
}
