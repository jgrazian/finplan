//! The admin listener: it answers only loopback hosts and its own origin,
//! renders every page, and its actions change what the API then reports.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use clap::Parser;
use finplan_server::{billing, config::ServerConfig, state::AppState};
use tower::ServiceExt;

#[derive(Parser)]
struct ConfigArgs {
    #[command(flatten)]
    config: ServerConfig,
}

const HOST: &str = "127.0.0.1:8090";
const ORIGIN: &str = "http://127.0.0.1:8090";

async fn setup() -> (Router, AppState) {
    let config = ConfigArgs::parse_from([
        "test",
        "--hosted",
        "--secure-cookies",
        "--cors-origins",
        "https://app.example.com",
        "--database-url",
        "sqlite::memory:",
        "--db-pool-size",
        "1",
    ])
    .config;
    let (_, state) = finplan_server::build(config).await.unwrap();
    for sql in [
        "INSERT INTO users(id,email,password_hash,display_name) VALUES
            ('u1','one@example.com','!','<b>One</b>'),('u2','two@example.com','!',NULL)",
        "INSERT INTO users(id,email,password_hash,kind) VALUES ('g1','g@guest.invalid','!','guest')",
        "INSERT INTO sessions(token_hash,public_id,user_id,expires_at)
            VALUES ('h1','p1','u1',datetime('now','+1 day'))",
        "INSERT INTO monthly_goal_seeks(user_id,month,used) VALUES ('u1',strftime('%Y-%m','now'),1)",
        "INSERT INTO monthly_offload_spend(user_id,month,used) VALUES ('u1',strftime('%Y-%m','now'),500)",
        "INSERT INTO contact_messages(user_id,topic,message) VALUES ('u1','bug_report','It <broke>')",
        "INSERT INTO ai_usage(user_id,feature,scenario_id,model,turns,input_tokens,output_tokens,
            cache_read_tokens,cache_write_tokens,cost_usd,unpriced_turns)
            VALUES ('u1','review',1,'test/model',3,9000,800,0,0,0.0421,0)",
    ] {
        sqlx::query(sql).execute(&state.db).await.unwrap();
    }
    let router = finplan_server::admin::router(state.clone());
    (router, state)
}

async fn get(router: &Router, uri: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(
            Request::get(uri)
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

/// POST a form from the listener's own origin; the redirect's location, or
/// the failure's status.
async fn post(router: &Router, uri: &str, form: &str) -> Result<String, StatusCode> {
    let response = router
        .clone()
        .oneshot(
            Request::post(uri)
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    match response.status() {
        StatusCode::SEE_OTHER => Ok(response.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .to_owned()),
        status => Err(status),
    }
}

#[tokio::test]
async fn every_page_renders_and_escapes_what_users_wrote() {
    let (router, _) = setup().await;
    for uri in [
        "/",
        "/users",
        "/users?kind=all&sort=spend&q=one",
        "/users?kind=guest&sort=seen",
        "/users/u1",
        "/users/g1",
        "/usage",
        "/usage?month=2020-01",
        "/usage?month=garbage",
        "/usage/months",
        "/subscriptions",
        "/inbox",
        "/inbox?status=all",
    ] {
        let (status, body) = get(&router, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(!body.contains("<b>One</b>"), "{uri} left markup unescaped");
        assert!(!body.contains("It <broke>"), "{uri} left markup unescaped");
    }
    let (_, user) = get(&router, "/users/u1").await;
    assert!(user.contains("&lt;b&gt;One&lt;/b&gt;"));
    assert!(user.contains("$0.0421"));
    let (_, usage) = get(&router, "/usage").await;
    assert!(usage.contains("9,000"));
    assert_eq!(get(&router, "/users/nobody").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn foreign_hosts_and_origins_are_refused() {
    let (router, state) = setup().await;
    let response = router
        .clone()
        .oneshot(
            Request::get("/")
                .header(header::HOST, "rebound.example.com:8090")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    for origin in [Some("https://evil.example.com"), Some("null"), None] {
        let mut request = Request::post("/users/u1/sessions/revoke").header(header::HOST, HOST);
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{origin:?}");
    }
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(sessions, 1, "a refused sign-out signs nobody out");
}

#[tokio::test]
async fn actions_change_what_the_api_reports() {
    let (router, state) = setup().await;
    let pro = |id: &'static str| {
        let state = state.clone();
        async move {
            billing::entitlements(&state.db, id, &state.config)
                .await
                .unwrap()
                .pro
        }
    };
    assert!(!pro("u1").await);

    // Grant, then revoke.
    assert_eq!(
        post(&router, "/users/u1/pro", "until=2000-01-01").await,
        Err(StatusCode::BAD_REQUEST),
        "a day already over"
    );
    assert_eq!(
        post(&router, "/users/g1/pro", "until=2999-01-01").await,
        Err(StatusCode::CONFLICT),
        "guests cannot hold Pro"
    );
    assert_eq!(
        post(&router, "/users/u1/pro", "until=2999-01-01").await,
        Ok("/users/u1?notice=granted".into())
    );
    assert!(pro("u1").await);
    assert_eq!(
        post(&router, "/users/u1/pro/revoke", "").await,
        Ok("/users/u1?notice=revoked".into())
    );
    assert!(!pro("u1").await);
    assert_eq!(
        post(&router, "/users/u1/pro/revoke", "").await,
        Err(StatusCode::CONFLICT)
    );

    // A payment provider's live subscription is not overwritten.
    sqlx::query(
        "INSERT INTO subscriptions(user_id,provider,subscription_id,state,access_until,revision)
         VALUES ('u2','paddle','sub_1','active',4000000000,3)",
    )
    .execute(&state.db)
    .await
    .unwrap();
    assert_eq!(
        post(&router, "/users/u2/pro", "until=2999-01-01").await,
        Err(StatusCode::CONFLICT)
    );

    // Allowances.
    assert_eq!(
        post(&router, "/users/u1/quotas/reset", "").await,
        Ok("/users/u1?notice=quotas_reset".into())
    );
    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM monthly_goal_seeks) + (SELECT count(*) FROM monthly_offload_spend)",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(left, 0);

    // Sessions.
    assert_eq!(
        post(&router, "/users/u1/sessions/revoke", "").await,
        Ok("/users/u1?notice=signed_out".into())
    );
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(sessions, 0);

    // Inbox.
    let message: i64 = sqlx::query_scalar("SELECT id FROM contact_messages")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(
        post(
            &router,
            &format!("/inbox/{message}/status"),
            "status=closed&back=%2Finbox%3Fstatus%3Dnew"
        )
        .await,
        Ok("/inbox?status=new&notice=status".into())
    );
    assert_eq!(
        post(
            &router,
            &format!("/inbox/{message}/status"),
            "status=reviewed&back=https%3A%2F%2Fevil.example.com"
        )
        .await,
        Ok("/inbox?notice=status".into()),
        "only local paths are followed back"
    );
    assert_eq!(
        post(&router, &format!("/inbox/{message}/status"), "status=gone").await,
        Err(StatusCode::BAD_REQUEST)
    );

    // Deletion needs the email; the AI spend outlives the user.
    assert_eq!(
        post(&router, "/users/u1/delete", "confirm=wrong%40example.com").await,
        Err(StatusCode::BAD_REQUEST)
    );
    assert_eq!(
        post(&router, "/users/u1/delete", "confirm=ONE%40example.com").await,
        Ok("/users?notice=deleted".into())
    );
    let (users, spend): (i64, f64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users WHERE id = 'u1'), (SELECT SUM(cost_usd) FROM ai_usage)",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!((users, spend), (0, 0.0421));
    let (status, page) = get(&router, "/usage").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("(deleted user)"));
}

#[tokio::test]
async fn the_monthly_rollup_fills_gaps_and_totals_every_month() {
    let (router, state) = setup().await;
    // Two months back, beside setup's pass this month; last month spent nothing.
    sqlx::query(
        "INSERT INTO ai_usage(user_id,feature,model,turns,input_tokens,output_tokens,
            cache_read_tokens,cache_write_tokens,cost_usd,unpriced_turns,created_at)
         VALUES ('u2','plan_chat','test/model',2,1000,100,0,0,2.5,1,
                 datetime('now','start of month','-2 months','+3 days'))",
    )
    .execute(&state.db)
    .await
    .unwrap();
    let month = |back: &str| {
        let state = state.clone();
        let back = back.to_owned();
        async move {
            sqlx::query_scalar::<_, String>("SELECT strftime('%Y-%m','now','start of month',?1)")
                .bind(back)
                .fetch_one(&state.db)
                .await
                .unwrap()
        }
    };
    let (status, page) = get(&router, "/usage/months").await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<&str> = page.split("<tr>").skip(2).collect();
    assert_eq!(rows.len(), 4, "three months and the total");
    assert!(rows[0].contains(&month("+0 months").await));
    assert!(rows[0].contains("$0.04<"), "totals are to the cent");
    assert!(rows[1].contains(&month("-1 months").await));
    assert!(rows[1].contains("$0.00"), "an idle month is a row of zeros");
    assert!(rows[2].contains(&month("-2 months").await));
    assert!(rows[2].contains("$2.50") && rows[2].contains("+1?"));
    assert!(rows[3].contains("Total") && rows[3].contains("$2.54"));
    assert!(rows[3].contains("10,000"), "input tokens summed");

    // An empty ledger renders an empty rollup, not an error.
    sqlx::query("DELETE FROM ai_usage")
        .execute(&state.db)
        .await
        .unwrap();
    let (status, page) = get(&router, "/usage/months").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("No AI passes recorded yet."));
}

#[test]
fn the_listener_must_be_loopback() {
    let config = |bind: &str| {
        ConfigArgs::parse_from(["test", "--admin-bind", bind])
            .config
            .validate()
    };
    assert!(config("127.0.0.1:8090").is_ok());
    assert!(config("[::1]:8090").is_ok());
    assert!(config("0.0.0.0:8090").is_err());
    assert!(config("192.168.1.10:8090").is_err());
}
