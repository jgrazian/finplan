//! The enrollment switch must not disable existing users or hosted protection.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use clap::Parser;
use finplan_server::config::ServerConfig;
use tower::ServiceExt;

#[derive(Parser)]
struct ConfigArgs {
    #[command(flatten)]
    config: ServerConfig,
}

#[tokio::test]
async fn closed_enrollment_preserves_login_and_does_not_create_accounts() {
    let config = ConfigArgs::parse_from([
        "test",
        "--hosted",
        "--secure-cookies",
        "--access-mode",
        "beta",
        "--registration-open",
        "false",
        "--cors-origins",
        "https://beta.example.com",
        "--database-url",
        "sqlite::memory:",
        "--db-pool-size",
        "1",
    ])
    .config;
    let (router, state) = finplan_server::build(config).await.unwrap();
    let hash = finplan_server::auth::hash_password_async("existing-password".into())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO users(id,email,password_hash) VALUES ('existing','existing@example.com',?)",
    )
    .bind(hash)
    .execute(&state.db)
    .await
    .unwrap();
    for (path, body, expected) in [
        (
            "register",
            serde_json::json!({"email":"new@example.com","password":"new-password","password_confirmation":"new-password"}),
            StatusCode::FORBIDDEN,
        ),
        (
            "login",
            serde_json::json!({"email":"existing@example.com","password":"existing-password"}),
            StatusCode::OK,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/auth/{path}"))
                    .header("origin", "https://beta.example.com")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if path == "login" {
            assert!(
                response.headers()["set-cookie"]
                    .to_str()
                    .unwrap()
                    .contains("Secure")
            );
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
