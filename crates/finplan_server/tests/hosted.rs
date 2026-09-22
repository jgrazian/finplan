//! Hosted security boundaries exercise the real router without external delivery.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use finplan_server::config::ServerConfig;
use tower::ServiceExt;
fn config() -> ServerConfig {
    ServerConfig {
        mail: Default::default(),
        log_format: Default::default(),
        metrics_bind: None,
        hosted: true,
        access_mode: Default::default(),
        registration_open: true,
        local_mail_sink: None,
        bind: "127.0.0.1:0".into(),
        database_url: "sqlite::memory:".into(),
        db_pool_size: 1,
        sim_workers: 1,
        max_iterations: 50000,
        secure_cookies: true,
        cors_origins: vec!["https://finplan.example".into()],
    }
}
#[tokio::test]
async fn hosted_requires_explicit_trusted_mutation_origins() {
    let (router, _) = finplan_server::build(config()).await.unwrap();
    for (origin, expected) in [
        (None, StatusCode::FORBIDDEN),
        (Some("null"), StatusCode::FORBIDDEN),
        (Some("https://hostile.example"), StatusCode::FORBIDDEN),
        (Some("https://finplan.example"), StatusCode::NO_CONTENT),
    ] {
        let mut req = Request::builder().method("POST").uri("/api/auth/logout");
        if let Some(origin) = origin {
            req = req.header("origin", origin);
        }
        let response = router
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header("authorization", "Bearer test")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.oneshot(request).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
}
#[test]
fn hosted_rejects_insecure_or_ambiguous_configuration() {
    let mut c = config();
    assert!(c.validate().is_ok());
    c.secure_cookies = false;
    assert!(c.validate().is_err());
    c.secure_cookies = true;
    for origin in [
        "http://finplan.example",
        "https://*",
        "https://finplan.example/path",
        "https://finplan.example/",
    ] {
        c.cors_origins = vec![origin.into()];
        assert!(c.validate().is_err());
    }
    c = config();
    c.local_mail_sink = Some("/tmp/mail".into());
    assert!(c.validate().is_err());
}
#[tokio::test]
async fn hosted_auth_abuse_returns_retry_after() {
    let (router, _) = finplan_server::build(config()).await.unwrap();
    let peer = "192.0.2.123:8888".parse::<std::net::SocketAddr>().unwrap();
    let mut last = None;
    for _ in 0..31 {
        let mut req = Request::builder()
            .method("POST")
            .uri("/api/auth/logout")
            .header("origin", "https://finplan.example")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(peer));
        last = Some(router.clone().oneshot(req).await.unwrap());
    }
    let response = last.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()["retry-after"], "60");
}

async fn hosted_fixture() -> (axum::Router, finplan_server::state::AppState, String) {
    fixture_with_config(config()).await
}
async fn fixture_with_config(
    config: ServerConfig,
) -> (axum::Router, finplan_server::state::AppState, String) {
    let (router, state) = finplan_server::build(config).await.unwrap();
    sqlx::query("INSERT INTO users(id,email,password_hash) VALUES ('owner','owner@example.com','unused'),('other','other@example.com','unused')").execute(&state.db).await.unwrap();
    let token = finplan_server::auth::session::issue(&state.db, "owner", None)
        .await
        .unwrap();
    (router, state, format!("finplan_session={token}"))
}
async fn send(
    router: axum::Router,
    cookie: &str,
    method: &str,
    path: &str,
    body: serde_json::Value,
) -> StatusCode {
    router
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("origin", "https://finplan.example")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}
#[tokio::test]
async fn free_plan_creation_is_atomic_and_downgrade_preserves_read_export_delete() {
    let (router, state, cookie) = hosted_fixture().await;
    let one = serde_json::json!({"name":"One","start_date":"2026-01-01"});
    let two = serde_json::json!({"name":"Two","start_date":"2026-01-01"});
    let (a, b) = tokio::join!(
        send(router.clone(), &cookie, "POST", "/api/scenarios", one),
        send(router.clone(), &cookie, "POST", "/api/scenarios", two)
    );
    assert_eq!(
        usize::from(a == StatusCode::CREATED) + usize::from(b == StatusCode::CREATED),
        1
    );
    assert!(a == StatusCode::FORBIDDEN || b == StatusCode::FORBIDDEN);
    let first: i64 = sqlx::query_scalar("SELECT id FROM scenarios WHERE user_id='owner'")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "POST",
            &format!("/api/scenarios/{first}/duplicate"),
            serde_json::json!({"name":"Clone"})
        )
        .await,
        StatusCode::FORBIDDEN
    );
    // Simulate an existing second plan from before downgrade.
    let second:i64=sqlx::query_scalar("INSERT INTO scenarios(user_id,name,start_date) VALUES ('owner','Old Pro plan','2026-01-01') RETURNING id").fetch_one(&state.db).await.unwrap();
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "PATCH",
            &format!("/api/scenarios/{second}"),
            serde_json::json!({"name":"Changed"})
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "POST",
            &format!("/api/scenarios/{second}/assets"),
            serde_json::json!({"name":"Bypass"})
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "GET",
            &format!("/api/scenarios/{second}"),
            serde_json::json!(null)
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "POST",
            "/api/billing/editable-plan",
            serde_json::json!({"scenario_id":second})
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "PATCH",
            &format!("/api/scenarios/{second}"),
            serde_json::json!({"name":"Changed"})
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "POST",
            "/api/billing/editable-plan",
            serde_json::json!({"scenario_id":9999})
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            router,
            &cookie,
            "DELETE",
            &format!("/api/scenarios/{first}"),
            serde_json::json!(null)
        )
        .await,
        StatusCode::NO_CONTENT
    );
}
#[test]
fn compute_admission_bounds_and_releases_per_user_capacity() {
    let first = finplan_server::billing::admit_compute("quota-test-user").unwrap();
    let second = finplan_server::billing::admit_compute("quota-test-user").unwrap();
    assert!(finplan_server::billing::admit_compute("quota-test-user").is_err());
    drop(first);
    assert!(finplan_server::billing::admit_compute("quota-test-user").is_ok());
    drop(second);
}

#[tokio::test]
async fn direct_compute_requests_cannot_bypass_free_tier() {
    let (router, state, cookie) = hosted_fixture().await;
    let id:i64=sqlx::query_scalar("INSERT INTO scenarios(user_id,name,start_date) VALUES ('owner','Budget','2026-01-01') RETURNING id").fetch_one(&state.db).await.unwrap();
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "POST",
            &format!("/api/scenarios/{id}/runs"),
            serde_json::json!({"iterations":1001})
        )
        .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "POST",
            &format!("/api/scenarios/{id}/analyses"),
            serde_json::json!({"kind":"sweep","axes":[]})
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(send(router,&cookie,"POST",&format!("/api/scenarios/{id}/analyses"),serde_json::json!({"kind":"solve","vary":[],"objective":"max-parameter","min_value":0.9,"iterations":1001})).await,StatusCode::FORBIDDEN);
    let usage: i64 =
        sqlx::query_scalar("SELECT count(*) FROM monthly_goal_seeks WHERE user_id='owner'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(usage, 0);
}

fn beta_config() -> ServerConfig {
    let mut config = config();
    config.access_mode = finplan_server::config::HostedAccessMode::Beta;
    config.max_iterations = 100;
    config
}

#[test]
fn beta_does_not_relax_hosted_configuration() {
    let mut config = beta_config();
    assert!(config.validate().is_ok());
    config.secure_cookies = false;
    assert!(config.validate().is_err());
    config.secure_cookies = true;
    config.cors_origins = vec!["http://localhost:3000".into()];
    assert!(config.validate().is_err());
    config = beta_config();
    config.max_iterations = 0;
    assert!(config.validate().is_err());
}

#[tokio::test]
async fn beta_unlocks_planning_without_subscriptions_and_preserves_ownership() {
    let (router, state, cookie) = fixture_with_config(beta_config()).await;
    for name in ["First beta plan", "Second beta plan"] {
        assert_eq!(
            send(
                router.clone(),
                &cookie,
                "POST",
                "/api/scenarios",
                serde_json::json!({"name":name,"start_date":"2026-01-01"})
            )
            .await,
            StatusCode::CREATED
        );
    }
    let second: i64 = sqlx::query_scalar("SELECT MAX(id) FROM scenarios WHERE user_id='owner'")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "PATCH",
            &format!("/api/scenarios/{second}"),
            serde_json::json!({"name":"Edited beta plan"})
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "POST",
            &format!("/api/scenarios/{second}/duplicate"),
            serde_json::json!({"name":"Beta clone"})
        )
        .await,
        StatusCode::CREATED
    );
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "POST",
            &format!("/api/scenarios/{second}/assets"),
            serde_json::json!({"name":"Beta asset","initial_price":100})
        )
        .await,
        StatusCode::CREATED
    );
    let other: i64 = sqlx::query_scalar("INSERT INTO scenarios(user_id,name,start_date) VALUES ('other','Private plan','2026-01-01') RETURNING id")
        .fetch_one(&state.db).await.unwrap();
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "PATCH",
            &format!("/api/scenarios/{other}"),
            serde_json::json!({"name":"Forbidden edit"})
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            router.clone(),
            "",
            "GET",
            "/api/billing/entitlements",
            serde_json::Value::Null
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/billing/entitlements")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let access: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(access["access_mode"], "beta");
    assert_eq!(access["pro"], true);
    assert_eq!(access["hosted"], true);
    assert_eq!(access["max_iterations"], 100);
    assert!(access["saved_plan_limit"].is_null());
    assert!(access["goal_seeks_per_month"].is_null());
    for _ in 0..2 {
        finplan_server::billing::reserve_goal_seek(&state.db, "owner", &state.config)
            .await
            .unwrap();
    }
    finplan_server::billing::require_pro(&state.db, "owner", &state.config)
        .await
        .unwrap();
    let subscriptions: i64 = sqlx::query_scalar("SELECT count(*) FROM subscriptions")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(subscriptions, 0);
    let usage: i64 = sqlx::query_scalar("SELECT count(*) FROM monthly_goal_seeks")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(usage, 0);
    // Browser mutations still need a trusted origin even with full beta access.
    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/scenarios/{second}"))
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"No origin"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn beta_direct_requests_cannot_bypass_compute_limits() {
    let (router, state, cookie) = fixture_with_config(beta_config()).await;
    let id: i64 = sqlx::query_scalar("INSERT INTO scenarios(user_id,name,start_date) VALUES ('owner','Beta','2026-01-01') RETURNING id")
        .fetch_one(&state.db).await.unwrap();
    assert_eq!(
        send(
            router.clone(),
            &cookie,
            "POST",
            &format!("/api/scenarios/{id}/runs"),
            serde_json::json!({"iterations":101})
        )
        .await,
        StatusCode::BAD_REQUEST
    );
    for request in [
        serde_json::json!({"kind":"sweep","axes":[],"iterations":101}),
        serde_json::json!({"kind":"sensitivity","iterations":101}),
        serde_json::json!({"kind":"solve","vary":[],"objective":"max-parameter","min_value":0.9,"iterations":101}),
    ] {
        assert_eq!(
            send(
                router.clone(),
                &cookie,
                "POST",
                &format!("/api/scenarios/{id}/analyses"),
                request
            )
            .await,
            StatusCode::BAD_REQUEST
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM runs")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
