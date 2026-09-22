//! Session tokens and the `CurrentUser` request extractor.

use axum::extract::FromRequestParts;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue};
use base64ct::{Base64UrlUnpadded, Encoding};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::observability::{Component, ErrorClass, RequestContext};
use crate::state::AppState;

pub const COOKIE_NAME: &str = "finplan_session";

/// How long a freshly issued session stays valid.
pub const SESSION_TTL_DAYS: i64 = 30;

/// A freshly minted session: the plaintext token goes to the client exactly
/// once, only its SHA-256 is persisted.
pub struct NewSession {
    pub token: String,
    pub token_hash: String,
}

#[must_use]
pub fn generate_token() -> NewSession {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = Base64UrlUnpadded::encode_string(&bytes);
    let token_hash = hash_token(&token);
    NewSession { token, token_hash }
}

#[must_use]
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    Base64UrlUnpadded::encode_string(&digest)
}

/// Persist a new session row and return the plaintext token.
///
/// The user agent is stored verbatim so the account screen can list the
/// devices holding a key to this account; turning it into "Chrome · macOS" is
/// presentation and stays in the client.
pub async fn issue(db: &Db, user_id: &str, user_agent: Option<&str>) -> ApiResult<String> {
    let session = generate_token();
    sqlx::query(
        "INSERT INTO sessions (token_hash, public_id, user_id, expires_at, user_agent)
         VALUES (?1, ?2, ?3, datetime('now', ?4), ?5)",
    )
    .bind(&session.token_hash)
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_id)
    .bind(format!("+{SESSION_TTL_DAYS} days"))
    .bind(user_agent)
    .execute(db)
    .await?;

    Ok(session.token)
}

/// The caller's `User-Agent`, clipped to something a row can hold.
#[must_use]
pub fn user_agent_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(300).collect::<String>())
        .filter(|v| !v.is_empty())
}

/// Return the internal owner only when a persisted session was revoked.
pub async fn revoke(db: &Db, token: &str) -> ApiResult<Option<String>> {
    let user_id =
        sqlx::query_scalar("DELETE FROM sessions WHERE token_hash = ?1 RETURNING user_id")
            .bind(hash_token(token))
            .fetch_optional(db)
            .await?;
    Ok(user_id)
}

/// Delete every session whose expiry has passed. Called at startup and by the
/// periodic janitor task.
pub async fn purge_expired(db: &Db) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= datetime('now')")
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}

pub fn build_cookie(token: &str, secure: bool) -> HeaderValue {
    let max_age = SESSION_TTL_DAYS * 24 * 60 * 60;
    let mut cookie =
        format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}");
    if secure {
        cookie.push_str("; Secure");
    }
    HeaderValue::from_str(&cookie).expect("session cookie is header-safe")
}

pub fn clear_cookie(secure: bool) -> HeaderValue {
    let mut cookie = format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    if secure {
        cookie.push_str("; Secure");
    }
    HeaderValue::from_str(&cookie).expect("session cookie is header-safe")
}

pub fn set_cookie_header(token: &str, secure: bool) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, build_cookie(token, secure));
    headers
}

/// Pull the session token out of the cookie header, or an
/// `Authorization: Bearer` header for non-browser clients.
fn extract_token(parts: &Parts) -> Option<String> {
    if let Some(cookies) = parts.headers.get(COOKIE).and_then(|v| v.to_str().ok()) {
        for pair in cookies.split(';') {
            let pair = pair.trim();
            if let Some(value) = pair
                .strip_prefix(COOKIE_NAME)
                .and_then(|r| r.strip_prefix('='))
                && !value.is_empty()
            {
                return Some(value.to_string());
            }
        }
    }

    parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// The authenticated caller. Extracting this rejects the request with 401 when
/// no valid, unexpired session is presented.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: String,
    pub email: String,
    /// The opaque id of the session this request arrived on, so the account
    /// screen can mark one row in the device list "this device".
    pub session_id: String,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_token(parts).ok_or(ApiError::Unauthorized)?;
        let token_hash = hash_token(&token);

        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT u.id, u.email, s.public_id
               FROM sessions s
               JOIN users u ON u.id = s.user_id
              WHERE s.token_hash = ?1 AND s.expires_at > datetime('now')",
        )
        .bind(&token_hash)
        .fetch_optional(&state.db)
        .await?;

        let (id, email, session_id) = row.ok_or(ApiError::Unauthorized)?;

        RequestContext::authenticate(&id);

        // Touch the session so idle-time can be reasoned about later.
        match sqlx::query("UPDATE sessions SET last_seen = datetime('now') WHERE token_hash = ?1")
            .bind(&token_hash)
            .execute(&state.db)
            .await
        {
            Ok(_) => state
                .telemetry
                .recovered_error(Component::Session, ErrorClass::Database),
            Err(_) => state
                .telemetry
                .recoverable_error(Component::Session, ErrorClass::Database),
        }

        Ok(CurrentUser {
            id,
            email,
            session_id,
        })
    }
}
