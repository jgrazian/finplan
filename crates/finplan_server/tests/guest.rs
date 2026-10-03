//! Guest access (spec 17): creation, limits, claiming, replacement on sign-in,
//! adoption through import, and retention.
//!
//! Rate-limit state is process-global, so every test sends from its own fake
//! peer address (the last octet of `203.0.113.x`).
use std::net::SocketAddr;

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use finplan_server::auth::session;
use finplan_server::config::{HostedAccessMode, ServerConfig};
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
        db_pool_size: 1,
        sim_workers: 1,
        max_iterations: 50000,
        secure_cookies: hosted,
        cors_origins: vec![ORIGIN.into()],
    }
}

struct App {
    router: Router,
    state: AppState,
    peer: SocketAddr,
}

impl App {
    async fn new(config: ServerConfig, last_octet: u8) -> Self {
        let (router, state) = finplan_server::build(config).await.unwrap();
        Self {
            router,
            state,
            peer: format!("203.0.113.{last_octet}:4000").parse().unwrap(),
        }
    }

    async fn hosted(last_octet: u8) -> Self {
        Self::new(config(true), last_octet).await
    }

    /// The same server, seen from another address.
    fn from(&self, last_octet: u8) -> Self {
        Self {
            router: self.router.clone(),
            state: self.state.clone(),
            peer: format!("203.0.113.{last_octet}:4000").parse().unwrap(),
        }
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        cookie: Option<&str>,
        body: Value,
    ) -> (StatusCode, HeaderMap, Value) {
        // Hashing slots are process-wide (`auth_throttle` answers "busy" with a
        // 2 second retry-after), so tests running side by side can be turned
        // away. Wait that out; the limits under test use other retry-afters.
        let response = loop {
            let mut req = Request::builder()
                .method(method)
                .uri(path)
                .header("origin", ORIGIN)
                .header("content-type", "application/json");
            if let Some(cookie) = cookie {
                req = req.header("cookie", cookie);
            }
            let mut req = req.body(Body::from(body.to_string())).unwrap();
            req.extensions_mut()
                .insert(axum::extract::ConnectInfo(self.peer));
            let response = self.router.clone().oneshot(req).await.unwrap();
            if response.status() == StatusCode::TOO_MANY_REQUESTS
                && response
                    .headers()
                    .get("retry-after")
                    .is_some_and(|v| v == "2")
            {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
            break response;
        };
        let (status, headers) = (response.status(), response.headers().clone());
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, json)
    }

    async fn guest(&self) -> (String, Value) {
        let (status, headers, user) = self.call("POST", "/api/auth/guest", None, json!({})).await;
        assert_eq!(status, StatusCode::CREATED, "{user}");
        (cookie_of(&headers).expect("a session cookie"), user)
    }

    async fn register(&self, email: &str) -> String {
        let (status, headers, body) = self
            .call(
                "POST",
                "/api/auth/register",
                None,
                json!({"email":email,"password":PASSWORD,"password_confirmation":PASSWORD}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        cookie_of(&headers).unwrap()
    }

    async fn plan(&self, cookie: &str, name: &str) -> StatusCode {
        self.call(
            "POST",
            "/api/scenarios",
            Some(cookie),
            json!({"name":name,"start_date":"2026-01-01"}),
        )
        .await
        .0
    }
}

/// `finplan_session=<token>` from a `Set-Cookie` header, ready to send back.
fn cookie_of(headers: &HeaderMap) -> Option<String> {
    let value = headers.get("set-cookie")?.to_str().ok()?;
    let pair = value.split(';').next()?.to_owned();
    (!pair.ends_with('=')).then_some(pair)
}

fn message(body: &Value) -> &str {
    body["error"]["message"].as_str().unwrap_or_default()
}

async fn user_exists(app: &App, id: &str) -> bool {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?)")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_guest_gets_a_session_a_seeded_library_and_the_guest_effort() {
    let app = App::hosted(1).await;
    let (cookie, user) = app.guest().await;

    assert_eq!(user["guest"], true);
    assert!(user["email"].as_str().unwrap().ends_with("@guest.invalid"));
    assert_eq!(user["default_iterations"], 100);

    let id = user["id"].as_str().unwrap();
    let profiles: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM return_profiles WHERE user_id = ?")
            .bind(id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(profiles > 0, "the shared library is seeded");

    let (status, headers, me) = app
        .call("GET", "/api/auth/me", Some(&cookie), json!(null))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["guest"], true);
    assert_eq!(me["id"], user["id"]);
    assert_eq!(
        cookie_of(&headers).as_deref(),
        Some(cookie.as_str()),
        "a guest's cookie is re-sent so its Max-Age slides"
    );
}

#[tokio::test]
async fn a_guests_session_slides_and_an_accounts_does_not() {
    let app = App::hosted(2).await;
    let (cookie, user) = app.guest().await;
    let account = app.register("slider@example.com").await;
    let near = "datetime('now', '+1 day')";
    sqlx::query(&format!("UPDATE sessions SET expires_at = {near}"))
        .execute(&app.state.db)
        .await
        .unwrap();

    for (cookie, is_guest) in [(&cookie, true), (&account, false)] {
        let (status, headers, _) = app
            .call("GET", "/api/auth/me", Some(cookie), json!(null))
            .await;
        assert_eq!(status, StatusCode::OK);
        // Only a guest's `me` hands the cookie back.
        assert_eq!(cookie_of(&headers).is_some(), is_guest);
    }
    let remaining = |id: String| {
        let db = app.state.db.clone();
        async move {
            sqlx::query_scalar::<_, bool>(
                "SELECT MIN(expires_at) > datetime('now', '+20 days') FROM sessions WHERE user_id = ?",
            )
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap()
        }
    };
    assert!(remaining(user["id"].as_str().unwrap().into()).await);
    let account_id: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind("slider@example.com")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(
        !remaining(account_id).await,
        "account expiry is fixed at issue"
    );
}

#[tokio::test]
async fn hosted_guest_access_can_be_turned_off_but_self_hosted_ignores_the_flag() {
    let mut off = config(true);
    off.guest_access = false;
    let app = App::new(off, 3).await;
    let (status, _, body) = app.call("POST", "/api/auth/guest", None, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let mut off = config(false);
    off.guest_access = false;
    let app = App::new(off, 4).await;
    let (status, _, body) = app.call("POST", "/api/auth/guest", None, json!({})).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["guest"], true);
    assert_eq!(
        body["default_iterations"], 2000,
        "self-hosted guests are not capped"
    );
}

#[tokio::test]
async fn one_address_gets_five_guests_an_hour() {
    let app = App::hosted(5).await;
    for _ in 0..5 {
        app.guest().await;
    }
    let (status, headers, _) = app.call("POST", "/api/auth/guest", None, json!({})).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.contains_key("retry-after"));

    let elsewhere = app.from(6);
    elsewhere.guest().await;
    let throttled = app.state.telemetry.encode().unwrap();
    assert!(throttled.contains("action=\"throttled\""));
}

#[tokio::test]
async fn claiming_keeps_the_plan_and_rotates_the_session() {
    let app = App::hosted(7).await;
    let (cookie, user) = app.guest().await;
    assert_eq!(app.plan(&cookie, "Mine").await, StatusCode::CREATED);

    let claim = json!({
        "email":"Claimer@Example.com","password":PASSWORD,
        "password_confirmation":PASSWORD,"display_name":"Casey"
    });
    let (status, headers, claimed) = app
        .call("POST", "/api/auth/guest/claim", Some(&cookie), claim)
        .await;
    assert_eq!(status, StatusCode::OK, "{claimed}");
    assert_eq!(claimed["guest"], false);
    assert_eq!(claimed["id"], user["id"], "same user, so the plan stays");
    assert_eq!(claimed["email"], "claimer@example.com");
    assert_eq!(claimed["display_name"], "Casey");
    assert_eq!(
        claimed["default_iterations"], 2000,
        "the guest cap was never the user's choice"
    );
    let fresh = cookie_of(&headers).unwrap();
    assert_ne!(fresh, cookie);

    let (status, ..) = app
        .call("GET", "/api/auth/me", Some(&cookie), json!(null))
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the guest session is revoked"
    );
    let (status, _, me) = app
        .call("GET", "/api/auth/me", Some(&fresh), json!(null))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["guest"], false);

    let (_, _, plans) = app
        .call("GET", "/api/scenarios", Some(&fresh), json!(null))
        .await;
    assert_eq!(plans.as_array().unwrap().len(), 1);
    assert_eq!(plans[0]["name"], "Mine");

    // The new password works for a plain sign-in.
    let (status, ..) = app
        .call(
            "POST",
            "/api/auth/login",
            None,
            json!({"email":"claimer@example.com","password":PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // An account cannot claim.
    let (status, _, body) = app
        .call(
            "POST",
            "/api/auth/guest/claim",
            Some(&fresh),
            json!({"email":"again@example.com","password":PASSWORD,"password_confirmation":PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let adopted = app.state.telemetry.encode().unwrap();
    assert!(adopted.contains("action=\"guest_created\""));
    assert!(adopted.contains("action=\"guest_claimed\""));
}

#[tokio::test]
async fn claiming_refuses_taken_and_placeholder_emails() {
    let app = App::hosted(8).await;
    app.register("taken@example.com").await;
    let (cookie, _) = app.guest().await;

    let (status, _, body) = app
        .call(
            "POST",
            "/api/auth/guest/claim",
            Some(&cookie),
            json!({"email":"taken@example.com","password":PASSWORD,"password_confirmation":PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        message(&body),
        "An account with that email exists. Sign in to bring this plan with you."
    );
    let (status, ..) = app
        .call("GET", "/api/auth/me", Some(&cookie), json!(null))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a refused claim leaves the guest signed in"
    );

    let placeholder = json!({
        "email":"guest-abc@guest.invalid","password":PASSWORD,"password_confirmation":PASSWORD
    });
    let (status, ..) = app
        .call(
            "POST",
            "/api/auth/guest/claim",
            Some(&cookie),
            placeholder.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, ..) = app
        .call("POST", "/api/auth/register", None, placeholder)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, ..) = app
        .call(
            "POST",
            "/api/auth/guest/claim",
            Some(&cookie),
            json!({"email":"new@example.com","password":PASSWORD,"password_confirmation":"other password"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, ..) = app
        .call(
            "POST",
            "/api/auth/guest/claim",
            None,
            json!({"email":"new@example.com","password":PASSWORD,"password_confirmation":PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_guest_placeholder_can_neither_sign_in_nor_recover() {
    let dir = tempfile::tempdir().unwrap();
    let mut local = config(false);
    local.secure_cookies = false;
    local.local_mail_sink = Some(dir.path().join("mail").display().to_string());
    let app = App::new(local, 9).await;
    let (_, guest) = app.guest().await;
    let placeholder = guest["email"].as_str().unwrap();

    // Even the right "password" for the placeholder hash is refused as a missing user.
    for password in ["!", PASSWORD, "correct horse battery staple"] {
        let (status, _, body) = app
            .call(
                "POST",
                "/api/auth/login",
                None,
                json!({"email":placeholder,"password":password}),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(message(&body), "invalid email or password");
    }

    let tokens = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM auth_action_tokens")
            .fetch_one(&app.state.db)
            .await
            .unwrap()
    };
    for path in [
        "/api/auth/forgot-password",
        "/api/auth/request-verification",
    ] {
        let (status, ..) = app
            .call("POST", path, None, json!({"email":placeholder}))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }
    assert_eq!(tokens().await, 0);

    // The same request for an account does issue a token, so the zero above means something.
    app.register("real@example.com").await;
    let (status, ..) = app
        .call(
            "POST",
            "/api/auth/forgot-password",
            None,
            json!({"email":"real@example.com"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(tokens().await, 1);
}

#[tokio::test]
async fn signing_in_from_a_guest_cookie_deletes_that_guest() {
    let app = App::hosted(10).await;
    app.register("owner@example.com").await;
    let (cookie, guest) = app.guest().await;
    assert_eq!(app.plan(&cookie, "Guest plan").await, StatusCode::CREATED);
    let guest_id = guest["id"].as_str().unwrap();

    let (status, headers, user) = app
        .call(
            "POST",
            "/api/auth/login",
            Some(&cookie),
            json!({"email":"owner@example.com","password":PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{user}");
    assert_eq!(user["guest"], false);
    assert!(cookie_of(&headers).is_some());
    assert!(!user_exists(&app, guest_id).await);
    let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scenarios WHERE user_id = ?")
        .bind(guest_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(plans, 0, "the guest's data goes with it");
    let (status, ..) = app
        .call("GET", "/api/auth/me", Some(&cookie), json!(null))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A failed sign-in leaves a guest alone.
    let (other, other_guest) = app.guest().await;
    let (status, ..) = app
        .call(
            "POST",
            "/api/auth/login",
            Some(&other),
            json!({"email":"owner@example.com","password":"wrong password"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(user_exists(&app, other_guest["id"].as_str().unwrap()).await);
}

#[tokio::test]
async fn importing_a_guest_plan_respects_plan_slots_and_counts_adoption() {
    let app = App::hosted(11).await;
    let (guest, _) = app.guest().await;
    assert_eq!(app.plan(&guest, "Guest plan").await, StatusCode::CREATED);
    let (status, _, archive) = app
        .call("GET", "/api/archives", Some(&guest), json!(null))
        .await;
    assert_eq!(status, StatusCode::OK);

    // A free account that already has its one plan cannot take another.
    let full = app.register("full@example.com").await;
    assert_eq!(app.plan(&full, "Existing").await, StatusCode::CREATED);
    let body = |request_id: &str| {
        json!({
            "archive": archive, "name_prefix": "Guest - ",
            "request_id": request_id, "from_guest": true
        })
    };
    let (status, ..) = app
        .call("POST", "/api/archives/import", Some(&full), body("a"))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        !app.state
            .telemetry
            .encode()
            .unwrap()
            .contains("action=\"guest_adopted\""),
        "a refused import is not an adoption"
    );

    // An empty account takes it, once; a replay is not counted again.
    let empty = app.register("empty@example.com").await;
    for _ in 0..2 {
        let (status, _, imported) = app
            .call("POST", "/api/archives/import", Some(&empty), body("b"))
            .await;
        assert_eq!(status, StatusCode::OK, "{imported}");
        assert_eq!(imported["scenario_ids"].as_array().unwrap().len(), 1);
    }
    let metrics = app.state.telemetry.encode().unwrap();
    let adopted: Vec<_> = metrics
        .lines()
        .filter(|l| l.contains("action=\"guest_adopted\""))
        .collect();
    assert_eq!(adopted.len(), 1, "{adopted:?}");
    assert!(adopted[0].ends_with(" 1"), "{adopted:?}");

    // `from_guest` is optional.
    let plain = app.register("plain@example.com").await;
    let (status, ..) = app
        .call(
            "POST",
            "/api/archives/import",
            Some(&plain),
            json!({"archive": archive, "name_prefix": "", "request_id": "c"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn in_beta_mode_a_hosted_guest_is_not_pro() {
    let mut beta = config(true);
    beta.access_mode = HostedAccessMode::Beta;
    let app = App::new(beta, 12).await;
    let (guest, _) = app.guest().await;
    let account = app.register("beta@example.com").await;

    let (status, _, e) = app
        .call(
            "GET",
            "/api/billing/entitlements",
            Some(&guest),
            json!(null),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(e["pro"], false);
    assert_eq!(e["guest"], true);
    assert_eq!(e["max_iterations"], 100);
    assert_eq!(e["saved_plan_limit"], 1);
    assert_eq!(e["guest_retention_days"], 30);

    let (_, _, e) = app
        .call(
            "GET",
            "/api/billing/entitlements",
            Some(&account),
            json!(null),
        )
        .await;
    assert_eq!(e["pro"], true, "beta still opens everything to accounts");
    assert_eq!(e["guest"], false);
}

async fn insert_user(app: &App, id: &str, kind: &str, age: &str) {
    sqlx::query(
        "INSERT INTO users(id,email,password_hash,kind,created_at)
         VALUES (?1, ?1 || '@example.com', '!', ?2, datetime('now', ?3))",
    )
    .bind(id)
    .bind(kind)
    .bind(age)
    .execute(&app.state.db)
    .await
    .unwrap();
}

async fn add_plan(app: &App, id: &str) {
    sqlx::query("INSERT INTO scenarios(user_id,name,start_date) VALUES (?,'Plan','2026-01-01')")
        .bind(id)
        .execute(&app.state.db)
        .await
        .unwrap();
}

async fn add_session(app: &App, id: &str, last_seen: &str) {
    session::issue(&app.state.db, id, None).await.unwrap();
    sqlx::query("UPDATE sessions SET last_seen = datetime('now', ?) WHERE user_id = ?")
        .bind(last_seen)
        .bind(id)
        .execute(&app.state.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn purge_takes_abandoned_guests_and_never_accounts() {
    let app = App::hosted(13).await;

    // Gone: no plan, over a day old (a drive-by).
    insert_user(&app, "empty-old", "guest", "-2 days").await;
    add_session(&app, "empty-old", "-1 minutes").await;
    // Gone: a plan, but nobody has been back inside the retention window.
    insert_user(&app, "idle-plan", "guest", "-40 days").await;
    add_plan(&app, "idle-plan").await;
    add_session(&app, "idle-plan", "-31 days").await;
    // Gone: a plan and no session left at all.
    insert_user(&app, "orphan-plan", "guest", "-2 hours").await;
    add_plan(&app, "orphan-plan").await;
    // Kept: a plan and a recent visit, however old the guest is.
    insert_user(&app, "live-plan", "guest", "-40 days").await;
    add_plan(&app, "live-plan").await;
    add_session(&app, "live-plan", "-2 days").await;
    // Kept: no plan yet, but under a day old.
    insert_user(&app, "young-empty", "guest", "-3 hours").await;
    add_session(&app, "young-empty", "-1 minutes").await;
    // Kept: just created, session not issued yet.
    insert_user(&app, "just-created", "guest", "-1 minutes").await;
    add_plan(&app, "just-created").await;
    // Kept: accounts, whatever their age, plans or sessions.
    insert_user(&app, "account", "account", "-90 days").await;
    add_session(&app, "account", "-60 days").await;
    insert_user(&app, "account-no-session", "account", "-90 days").await;

    let (empty, with_plans) = finplan_server::auth::guest::purge(&app.state.db, 30)
        .await
        .unwrap();
    assert_eq!((empty, with_plans), (1, 2));

    for gone in ["empty-old", "idle-plan", "orphan-plan"] {
        assert!(!user_exists(&app, gone).await, "{gone} should be purged");
    }
    for kept in [
        "live-plan",
        "young-empty",
        "just-created",
        "account",
        "account-no-session",
    ] {
        assert!(user_exists(&app, kept).await, "{kept} should be kept");
    }
    let plans: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scenarios WHERE user_id = 'idle-plan'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(plans, 0);

    // Nothing left to take on a second pass.
    assert_eq!(
        finplan_server::auth::guest::purge(&app.state.db, 30)
            .await
            .unwrap(),
        (0, 0)
    );

    app.state.telemetry.guests_purged(empty, with_plans);
    let metrics = app.state.telemetry.encode().unwrap();
    assert!(
        metrics.contains("guests_purged_total{kind=\"empty\"} 1"),
        "{metrics}"
    );
    assert!(metrics.contains("guests_purged_total{kind=\"with_plans\"} 2"));
}
