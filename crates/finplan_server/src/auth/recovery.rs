//! Single-use credential actions. Delivery is explicitly a local development sink.
use super::{normalize_email, session};
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
    let Some(directory) = state
        .config
        .local_mail_sink
        .as_ref()
        .filter(|_| !state.config.hosted)
    else {
        return Err(ApiError::Conflict(
            "Email delivery is not configured. Contact the service operator.".into(),
        ));
    };
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
    // Separate protected files avoid interleaved mail and do not expose tokens in logs/API.
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    std::fs::create_dir_all(directory)
        .map_err(|_| ApiError::internal("local mail sink unavailable"))?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| ApiError::internal("local mail sink unavailable"))?;
    let path = std::path::Path::new(directory).join(format!("{}.json", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| ApiError::internal("local mail sink unavailable"))?;
    let message = serde_json::json!({"to": email, "purpose": purpose, "token": token.token, "expires_in_seconds": 1800});
    file.write_all(message.to_string().as_bytes())
        .map_err(|_| ApiError::internal("local mail sink unavailable"))?;
    tx.commit().await?;
    Ok(())
}
async fn request_reset(
    State(state): State<AppState>,
    Json(body): Json<RecoveryEmail>,
) -> ApiResult<StatusCode> {
    issue(&state, &body.email, "reset").await?;
    Ok(StatusCode::ACCEPTED)
}
async fn request_verify(
    State(state): State<AppState>,
    Json(body): Json<RecoveryEmail>,
) -> ApiResult<StatusCode> {
    issue(&state, &body.email, "verify").await?;
    Ok(StatusCode::ACCEPTED)
}
/// Consumption and protected action share one write transaction; rollback leaves
/// a token usable after an operational failure, and racing consumption loses.
pub async fn consume(
    db: &Db,
    token: &str,
    purpose: &str,
    password_hash: Option<&str>,
) -> ApiResult<()> {
    let mut tx = db.begin().await?;
    let row: Option<(String, String)> = sqlx::query_as("DELETE FROM auth_action_tokens WHERE token_hash = ? AND purpose = ? AND expires_at > unixepoch() RETURNING user_id, email")
        .bind(session::hash_token(token)).bind(purpose).fetch_optional(&mut *tx).await?;
    let (user, email) = row.ok_or_else(|| ApiError::bad_request("invalid or expired token"))?;
    if purpose == "reset" {
        let hash = password_hash.ok_or_else(|| ApiError::bad_request("password is required"))?;
        let updated = sqlx::query("UPDATE users SET password_hash = ?, updated_at = datetime('now') WHERE id = ? AND email = ?")
            .bind(hash).bind(&user).bind(&email).execute(&mut *tx).await?;
        if updated.rows_affected() != 1 {
            return Err(ApiError::bad_request("invalid or expired token"));
        }
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(&user)
            .execute(&mut *tx)
            .await?;
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
    Ok(())
}
async fn reset(
    State(state): State<AppState>,
    Json(body): Json<ResetPassword>,
) -> ApiResult<StatusCode> {
    let hash = super::hash_password_async(body.new_password).await?;
    consume(&state.db, &body.token, "reset", Some(&hash)).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn verify(
    State(state): State<AppState>,
    Json(body): Json<VerificationToken>,
) -> ApiResult<StatusCode> {
    consume(&state.db, &body.token, "verify", None).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
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
