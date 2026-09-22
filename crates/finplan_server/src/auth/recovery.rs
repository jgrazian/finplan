//! Single-use credential actions with SMTP or an explicit local development sink.
use super::{normalize_email, session};
use crate::observability::{AuthAction, AuthOutcome, EventFields, RequestContext};
use crate::{
    db::Db,
    error::{ApiError, ApiResult},
    state::AppState,
};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/forgot-password", post(request_reset))
        .route("/reset-password", post(reset))
        .route("/request-verification", post(request_verify))
        .route("/verify-email", post(verify))
}
#[derive(Deserialize, ts_rs::TS)]
#[ts(export)]
struct RecoveryEmail {
    email: String,
}
#[derive(Deserialize, ts_rs::TS)]
#[ts(export)]
struct ResetPassword {
    token: String,
    new_password: String,
}
#[derive(Deserialize, ts_rs::TS)]
#[ts(export)]
struct VerificationToken {
    token: String,
}

async fn issue(state: &AppState, raw_email: &str, purpose: &str) -> ApiResult<()> {
    let local_sink = state
        .config
        .local_mail_sink
        .as_deref()
        .filter(|_| !state.config.hosted);
    if local_sink.is_none() && !state.config.mail.available() {
        return Err(ApiError::Conflict(
            "Email delivery is not configured. Contact the service operator.".into(),
        ));
    }
    let Ok(email) = normalize_email(raw_email) else {
        return Ok(());
    };
    let user: Option<String> = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    let Some(user) = user else {
        return Ok(());
    };
    let token = session::generate_token();
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM auth_action_tokens WHERE user_id = ? AND purpose = ?")
        .bind(&user)
        .bind(purpose)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO auth_action_tokens VALUES (?, ?, ?, ?, unixepoch() + 1800)")
        .bind(&token.token_hash)
        .bind(&user)
        .bind(purpose)
        .bind(&email)
        .execute(&mut *tx)
        .await?;
    // Never hold SQLite's writer lock while waiting on an external mail server.
    tx.commit().await?;
    if let Err(error) = state
        .config
        .mail
        .deliver(local_sink, &email, purpose, &token.token)
        .await
    {
        // Remove only this issuance: another request may already have replaced it.
        sqlx::query("DELETE FROM auth_action_tokens WHERE token_hash = ?")
            .bind(&token.token_hash)
            .execute(&state.db)
            .await?;
        return Err(error);
    }
    Ok(())
}
async fn request_reset(
    State(state): State<AppState>,
    Json(body): Json<RecoveryEmail>,
) -> ApiResult<StatusCode> {
    issue(&state, &body.email, "reset").await?;
    state.telemetry.auth(
        AuthAction::ResetRequested,
        AuthOutcome::Succeeded,
        &EventFields::default(),
    );

    Ok(StatusCode::ACCEPTED)
}
async fn request_verify(
    State(state): State<AppState>,
    Json(body): Json<RecoveryEmail>,
) -> ApiResult<StatusCode> {
    issue(&state, &body.email, "verify").await?;
    state.telemetry.auth(
        AuthAction::VerificationRequested,
        AuthOutcome::Succeeded,
        &EventFields::default(),
    );

    Ok(StatusCode::ACCEPTED)
}
/// Consumption and protected action share one write transaction; rollback leaves
/// a token usable after an operational failure, and racing consumption loses.
/// Returns the internal user ID and number of revoked sessions after commit.
pub async fn consume(
    db: &Db,
    token: &str,
    purpose: &str,
    password_hash: Option<&str>,
) -> ApiResult<(String, u64)> {
    let mut tx = db.begin().await?;
    let row: Option<(String, String)> = sqlx::query_as("DELETE FROM auth_action_tokens WHERE token_hash = ? AND purpose = ? AND expires_at > unixepoch() RETURNING user_id, email")
        .bind(session::hash_token(token)).bind(purpose).fetch_optional(&mut *tx).await?;
    let (user, email) = row.ok_or_else(|| ApiError::bad_request("invalid or expired token"))?;
    let mut revoked = 0;
    if purpose == "reset" {
        let hash = password_hash.ok_or_else(|| ApiError::bad_request("password is required"))?;
        let updated = sqlx::query("UPDATE users SET password_hash = ?, updated_at = datetime('now') WHERE id = ? AND email = ?")
            .bind(hash).bind(&user).bind(&email).execute(&mut *tx).await?;
        if updated.rows_affected() != 1 {
            return Err(ApiError::bad_request("invalid or expired token"));
        }
        revoked = sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(&user)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query("DELETE FROM auth_action_tokens WHERE user_id = ?")
            .bind(&user)
            .execute(&mut *tx)
            .await?;
    } else {
        let updated = sqlx::query(
            "UPDATE users SET email_verified_at = datetime('now') WHERE id = ? AND email = ?",
        )
        .bind(&user)
        .bind(&email)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(ApiError::bad_request("invalid or expired token"));
        }
    }
    tx.commit().await?;
    Ok((user, revoked))
}
async fn reset(
    State(state): State<AppState>,
    Json(body): Json<ResetPassword>,
) -> ApiResult<StatusCode> {
    let hash = super::hash_password_async(body.new_password).await?;
    let (user_id, revoked) = consume(&state.db, &body.token, "reset", Some(&hash)).await?;
    RequestContext::authenticate(&user_id);
    state.telemetry.auth(
        AuthAction::ResetCompleted,
        AuthOutcome::Succeeded,
        &EventFields {
            user_id: Some(&user_id),
            count: Some(revoked),
            ..Default::default()
        },
    );

    Ok(StatusCode::NO_CONTENT)
}
async fn verify(
    State(state): State<AppState>,
    Json(body): Json<VerificationToken>,
) -> ApiResult<StatusCode> {
    let (user_id, _) = consume(&state.db, &body.token, "verify", None).await?;
    RequestContext::authenticate(&user_id);
    state.telemetry.auth(
        AuthAction::EmailVerified,
        AuthOutcome::Succeeded,
        &EventFields {
            user_id: Some(&user_id),
            ..Default::default()
        },
    );

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn test_state(sink: Option<String>) -> AppState {
        #[derive(clap::Parser)]
        struct Args {
            #[command(flatten)]
            config: crate::config::ServerConfig,
        }
        let mut config = <Args as clap::Parser>::parse_from(["recovery-test"]).config;
        config.database_url = "sqlite::memory:".into();
        config.db_pool_size = 1;
        config.local_mail_sink = sink;
        config.mail = Default::default();
        config.hosted = false;
        let (_, state) = crate::build(config).await.unwrap();
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES ('u','u@example.com','old')")
            .execute(&state.db)
            .await
            .unwrap();
        state
    }

    #[tokio::test]
    async fn delivery_issues_hashed_tokens_and_reissuance_revokes_previous_token() {
        let dir = tempfile::tempdir().unwrap();
        let sink = dir.path().join("mail");
        let state = test_state(Some(sink.display().to_string())).await;
        issue(&state, "missing@example.com", "reset").await.unwrap();
        assert!(!sink.exists());
        issue(&state, " U@example.com ", "reset").await.unwrap();
        let first_path = std::fs::read_dir(&sink)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let message: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&first_path).unwrap()).unwrap();
        let token = message["token"].as_str().unwrap();
        let (hash, expires): (String, i64) =
            sqlx::query_as("SELECT token_hash, expires_at - unixepoch() FROM auth_action_tokens")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(hash, session::hash_token(token));
        assert_ne!(hash, token);
        assert!((1790..=1800).contains(&expires));
        issue(&state, "u@example.com", "reset").await.unwrap();
        assert!(
            consume(&state.db, token, "reset", Some("new"))
                .await
                .is_err()
        );
        let second_path = std::fs::read_dir(&sink)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path != &first_path)
            .unwrap();
        let message: serde_json::Value =
            serde_json::from_slice(&std::fs::read(second_path).unwrap()).unwrap();
        consume(
            &state.db,
            message["token"].as_str().unwrap(),
            "reset",
            Some("new"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn delivery_failure_removes_issued_token_and_unconfigured_mail_is_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let sink = dir.path().join("not-a-directory");
        std::fs::write(&sink, "occupied").unwrap();
        let state = test_state(Some(sink.display().to_string())).await;
        assert!(issue(&state, "u@example.com", "verify").await.is_err());
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM auth_action_tokens")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let state = test_state(None).await;
        assert!(matches!(
            issue(&state, "u@example.com", "reset").await,
            Err(ApiError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn tokens_are_hashed_expiring_single_use_and_reset_ends_sessions() {
        let db = crate::db::connect("sqlite::memory:", 1).await.unwrap();
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES ('u','u@example.com','old')")
            .execute(&db)
            .await
            .unwrap();
        let session_token = session::issue(&db, "u", None).await.unwrap();
        for (raw, purpose, expires) in [
            ("expired", "reset", 0_i64),
            ("reset", "reset", 4_000_000_000),
            ("verify", "verify", 4_000_000_000),
        ] {
            sqlx::query("INSERT INTO auth_action_tokens VALUES (?,'u',?,'u@example.com',?)")
                .bind(session::hash_token(raw))
                .bind(purpose)
                .bind(expires)
                .execute(&db)
                .await
                .unwrap();
        }
        assert!(consume(&db, "expired", "reset", Some("new")).await.is_err());
        assert!(consume(&db, "verify", "reset", Some("new")).await.is_err());
        consume(&db, "verify", "verify", None).await.unwrap();
        assert!(consume(&db, "verify", "verify", None).await.is_err());
        consume(&db, "reset", "reset", Some("new")).await.unwrap();
        assert!(consume(&db, "reset", "reset", Some("again")).await.is_err());
        let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE token_hash=?")
            .bind(session::hash_token(&session_token))
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
        let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id='u'")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(hash, "new");
    }
}
