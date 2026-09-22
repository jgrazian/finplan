//! Activity telemetry is tested at real persistence boundaries, with synthetic private data.
use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use finplan_server::{auth::session, config::ServerConfig, state::AppState};
use serde_json::{Value, json};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Capture {
    fn dispatch(&self) -> tracing::Dispatch {
        let writer = self.clone();
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .json()
                .with_max_level(tracing::Level::DEBUG)
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(move || writer.clone())
                .finish(),
        )
    }
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
    fn events(&self, name: &str) -> Vec<Value> {
        self.text()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|event| event["fields"]["event"] == name)
            .collect()
    }
    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

struct App {
    router: Router,
    state: AppState,
    cookie: String,
    user_id: String,
    scenario_id: i64,
    logs: Capture,
    dispatch: tracing::Dispatch,
    _dir: tempfile::TempDir,
}
impl App {
    async fn new() -> Self {
        // Parsing the actual CLI defaults keeps the fixture independent of new
        // optional settings; all stateful/security settings are set explicitly.
        #[derive(clap::Parser)]
        struct Args {
            #[command(flatten)]
            config: ServerConfig,
        }
        let mut config = <Args as clap::Parser>::parse_from(["activity-test"]).config;
        let dir = tempfile::tempdir().unwrap();
        config.database_url = format!("sqlite://{}", dir.path().join("test.db").display());
        config.bind = "127.0.0.1:0".into();
        config.db_pool_size = 2;
        config.sim_workers = 1;
        config.hosted = false;
        config.secure_cookies = false;
        config.local_mail_sink = Some(dir.path().join("mail").display().to_string());
        config.cors_origins = vec!["http://localhost:3000".into()];
        let (router, state) = finplan_server::build(config).await.unwrap();
        let user_id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES (?,?,?)")
            .bind(&user_id)
            .bind("private-email-sentinel@example.com")
            .bind(finplan_server::auth::hash_password("private-password-sentinel").unwrap())
            .execute(&state.db)
            .await
            .unwrap();
        finplan_server::seed::seed_user_library(&state.db, &user_id)
            .await
            .unwrap();
        let token = session::issue(&state.db, &user_id, Some("private-agent-sentinel"))
            .await
            .unwrap();
        let scenario_id = sqlx::query_scalar("INSERT INTO scenarios(user_id,name,start_date,duration_years) VALUES (?,'private-scenario-sentinel','2026-01-01',30) RETURNING id")
            .bind(&user_id).fetch_one(&state.db).await.unwrap();
        let logs = Capture::default();
        let dispatch = logs.dispatch();
        Self {
            router,
            state,
            cookie: format!("{}={token}", session::COOKIE_NAME),
            user_id,
            scenario_id,
            logs,
            dispatch,
            _dir: dir,
        }
    }
    async fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        authenticated: bool,
    ) -> (StatusCode, Value, String) {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header("x-request-id", "private-untrusted-request-id");
        if authenticated {
            req = req.header(header::COOKIE, &self.cookie);
        }
        let req = match body {
            Some(body) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let response = self
            .router
            .clone()
            .oneshot(req)
            .with_subscriber(self.dispatch.clone())
            .await
            .unwrap();
        let status = response.status();
        let request_id = response
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            request_id,
        )
    }
    fn metric(&self, prefix: &str) -> f64 {
        self.state
            .telemetry
            .encode()
            .unwrap()
            .lines()
            .find(|line| line.starts_with(prefix))
            .map(|line| line.split_whitespace().last().unwrap().parse().unwrap())
            .unwrap_or(0.)
    }
    fn private_data_absent(&self) {
        let text = self.logs.text();
        for secret in [
            "private-email-sentinel",
            "private-password-sentinel",
            "private-account-sentinel",
            "private-scenario-sentinel",
            "private-description-sentinel",
            "private-agent-sentinel",
            "private-query-sentinel",
            "private-error-sentinel",
            "private-untrusted-request-id",
            "private-unknown-key-sentinel",
            "987654321.75",
            &self.cookie,
        ] {
            assert!(!text.contains(secret), "private data leaked: {secret}");
        }
    }
}

#[tokio::test]
async fn committed_account_events_survive_response_failure_and_exclude_rollbacks() {
    let app = App::new().await;
    let path = format!("/api/scenarios/{}/accounts", app.scenario_id);
    let body = json!({"name":"private-account-sentinel", "description":"private-description-sentinel", "flavor":"Liability", "principal":987654321.75,"interest_rate":0.04});
    let (status, account, request_id) = app
        .send(
            "POST",
            &format!("{path}?source=private-query-sentinel"),
            Some(body.clone()),
            true,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{account}");
    let id = account["id"].as_i64().unwrap();
    let created = app.logs.events("account.created");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["fields"]["resource_id"], id);
    assert_eq!(created[0]["fields"]["user_id"], app.user_id);
    assert!(created[0].to_string().contains(&request_id));
    assert_ne!(request_id, "private-untrusted-request-id");
    assert_eq!(app.send("PATCH", &format!("{path}/{id}"), Some(json!({"description":null,"name":"private-account-sentinel-updated","private-unknown-key-sentinel":"secret"})), true).await.0, StatusCode::OK);
    let updated = app.logs.events("account.updated");
    assert_eq!(updated.len(), 1);
    assert_eq!(
        updated[0]["fields"]["submitted_fields"],
        r#"["description", "name"]"#
    );
    assert_eq!(
        app.send("DELETE", &format!("{path}/{id}"), None, true)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.send("DELETE", &format!("{path}/{id}"), None, true)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(app.logs.events("account.deleted").len(), 1);
    assert_eq!(
        app.send("POST", &path, Some(body.clone()), false).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(app.send("POST", &path, Some(json!({"name":"private-account-sentinel-rollback","flavor":"Bank","return_profile_id":-1})), true).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(app.logs.events("account.created").len(), 1);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE scenario_id=?")
        .bind(app.scenario_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(rows, 0, "detail FK failure must roll back account INSERT");
    sqlx::query("CREATE TRIGGER fail_activity_touch BEFORE UPDATE ON scenarios BEGIN SELECT RAISE(FAIL, 'private-error-sentinel'); END")
        .execute(&app.state.db).await.unwrap();
    assert_eq!(
        app.send("POST", &path, Some(body), true).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        app.logs.events("account.created").len(),
        2,
        "the account committed before scenario touch failed"
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE scenario_id=?")
        .bind(app.scenario_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(rows, 1);
    assert_eq!(
        app.metric("finplan_mutations_total{resource=\"account\",operation=\"created\"}"),
        2.
    );
    app.private_data_absent();
}

#[tokio::test]
async fn composite_events_replays_and_unchanged_reorders_are_not_new_mutations() {
    let app = App::new().await;
    let profile: i64 = sqlx::query_scalar("SELECT id FROM return_profiles WHERE user_id=? LIMIT 1")
        .bind(&app.user_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let setup = json!({"request_id":"private-setup-key-sentinel","name":"private-account-sentinel","start_date":"2026-01-01","birth_date":"1981-01-01","duration_years":50,"retirement_age":65,"cash":987654321.75,"retirement_401k":0,"investments":0,"stock_percent":60,"cash_profile_id":profile,"stock_profile_id":profile,"bond_profile_id":profile,"investment_tax_status":"Taxable","annual_income":0,"retirement_401k_contribution_percent":0,"annual_spending":0,"retirement_spending":0,"inflation_profile_id":null,"tax_config_id":null,"fund_from_investments":false,"assumptions_confirmed":true});
    let (status, created, _) = app
        .send("POST", "/api/scenarios/setup", Some(setup.clone()), true)
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(
        app.send("POST", "/api/scenarios/setup", Some(setup), true)
            .await
            .1,
        created
    );
    let events = app.logs.events("onboarding.completed");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["fields"]["replay"], false);
    assert_eq!(events[1]["fields"]["replay"], true);
    assert!(
        app.logs.events("account.created").is_empty(),
        "one parent event covers the composite mutation"
    );
    assert_eq!(
        app.metric("finplan_mutations_total{resource=\"onboarding\",operation=\"completed\"}"),
        1.
    );
    let sid = created["scenario_id"].as_i64().unwrap();
    let (_, archive, _) = app
        .send("GET", &format!("/api/scenarios/{sid}/archive"), None, true)
        .await;
    let import =
        json!({"archive":archive,"name_prefix":"Copy ","request_id":"private-import-key-sentinel"});
    let (status, imported, _) = app
        .send("POST", "/api/archives/import", Some(import.clone()), true)
        .await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    assert_eq!(
        app.send("POST", "/api/archives/import", Some(import), true)
            .await
            .1,
        imported
    );
    assert_eq!(
        app.metric("finplan_mutations_total{resource=\"archive\",operation=\"imported\"}"),
        1.
    );
    assert_eq!(
        app.metric("finplan_mutations_total{resource=\"archive\",operation=\"exported\"}"),
        0.
    );
    assert_eq!(
        app.logs.events("archive.imported")[1]["fields"]["replay"],
        true
    );
    assert_eq!(
        app.send(
            "POST",
            &format!("/api/scenarios/{sid}/accounts/reorder"),
            Some(json!({"ids":[]})),
            true
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert!(app.logs.events("account.reordered").is_empty());
    app.private_data_absent();
    assert!(!app.logs.text().contains("private-setup-key-sentinel"));
    assert!(!app.logs.text().contains("private-import-key-sentinel"));
}

#[tokio::test]
async fn auth_failures_are_generic_and_session_maintenance_failures_are_aggregated() {
    let app = App::new().await;
    for email in [
        "private-email-sentinel@example.com",
        "missing-private-email@example.com",
    ] {
        assert_eq!(
            app.send(
                "POST",
                "/api/auth/login",
                Some(json!({"email":email,"password":"private-password-wrong"})),
                false
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    let events = app.logs.events("auth.login");
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0]["fields"], events[1]["fields"],
        "a failed login never reports account existence or an actor"
    );
    app.logs.clear();
    sqlx::query("CREATE TRIGGER fail_session_touch BEFORE UPDATE ON sessions BEGIN SELECT RAISE(FAIL,'private-error-sentinel'); END")
        .execute(&app.state.db).await.unwrap();
    for _ in 0..3 {
        assert_eq!(
            app.send("GET", "/api/auth/me", None, true).await.0,
            StatusCode::OK
        );
    }
    assert_eq!(app.logs.events("maintenance.failed").len(), 1);
    assert_eq!(
        app.metric("finplan_server_errors_total{component=\"session\",class=\"database\"}"),
        3.
    );
    sqlx::query("DROP TRIGGER fail_session_touch")
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        app.send("GET", "/api/auth/me", None, true).await.0,
        StatusCode::OK
    );
    assert_eq!(app.logs.events("maintenance.recovered").len(), 1);
    assert_eq!(
        app.send("POST", "/api/auth/logout", None, true).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.send("POST", "/api/auth/logout", None, true).await.0,
        StatusCode::NO_CONTENT
    );
    let logout = app.logs.events("auth.logout");
    assert_eq!(logout[0]["fields"]["outcome"], "succeeded");
    assert_eq!(logout[1]["fields"]["outcome"], "replay");
    app.private_data_absent();
}

#[tokio::test]
async fn password_and_recovery_events_follow_atomic_commits_without_credentials() {
    let app = App::new().await;
    let other_token = session::issue(&app.state.db, &app.user_id, None)
        .await
        .unwrap();
    let change = json!({"current_password":"private-password-sentinel", "new_password":"private-new-password-sentinel", "new_password_confirmation":"private-new-password-sentinel"});
    sqlx::query("CREATE TRIGGER fail_revoke BEFORE DELETE ON sessions BEGIN SELECT RAISE(FAIL,'private-error-sentinel'); END")
        .execute(&app.state.db).await.unwrap();
    assert_eq!(
        app.send("POST", "/api/auth/password", Some(change.clone()), true)
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(app.logs.events("auth.password_changed").is_empty());
    let stored: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=?")
        .bind(&app.user_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(finplan_server::auth::verify_password(
        "private-password-sentinel",
        &stored
    ));
    sqlx::query("DROP TRIGGER fail_revoke")
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        app.send("POST", "/api/auth/password", Some(change), true)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let changed = app.logs.events("auth.password_changed");
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["fields"]["affected_count"], 1);
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE token_hash=?")
        .bind(session::hash_token(&other_token))
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    for email in [
        "private-email-sentinel@example.com",
        "missing-private-email@example.com",
    ] {
        assert_eq!(
            app.send(
                "POST",
                "/api/auth/forgot-password",
                Some(json!({"email":email})),
                false
            )
            .await
            .0,
            StatusCode::ACCEPTED
        );
    }
    let requested = app.logs.events("auth.reset_requested");
    assert_eq!(requested.len(), 2);
    assert_eq!(requested[0]["fields"], requested[1]["fields"]);
    assert_eq!(
        app.send(
            "POST",
            "/api/auth/request-verification",
            Some(json!({"email":"private-email-sentinel@example.com"})),
            false
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    let mut reset_token = None;
    let mut verification_token = None;
    for path in std::fs::read_dir(app.state.config.local_mail_sink.as_ref().unwrap()).unwrap() {
        let message: Value =
            serde_json::from_slice(&std::fs::read(path.unwrap().path()).unwrap()).unwrap();
        match message["purpose"].as_str().unwrap() {
            "reset" => reset_token = Some(message["token"].as_str().unwrap().to_owned()),
            "verify" => verification_token = Some(message["token"].as_str().unwrap().to_owned()),
            _ => unreachable!(),
        }
    }
    let reset_token = reset_token.unwrap();
    let verification_token = verification_token.unwrap();
    assert_eq!(
        app.send(
            "POST",
            "/api/auth/verify-email",
            Some(json!({"token":verification_token})),
            false
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let reset = json!({"token":reset_token,"new_password":"private-reset-password-sentinel"});
    assert_eq!(
        app.send(
            "POST",
            "/api/auth/reset-password",
            Some(reset.clone()),
            false
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.send("POST", "/api/auth/reset-password", Some(reset), false)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(app.logs.events("auth.email_verified").len(), 1);
    let completed = app.logs.events("auth.reset_completed");
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0]["fields"]["user_id"], app.user_id);
    assert_eq!(completed[0]["fields"]["affected_count"], 1);
    app.private_data_absent();
    for secret in [
        "private-new-password-sentinel",
        "private-reset-password-sentinel",
        &reset_token,
        &verification_token,
        &stored,
        &other_token,
    ] {
        assert!(!app.logs.text().contains(secret));
    }
}
