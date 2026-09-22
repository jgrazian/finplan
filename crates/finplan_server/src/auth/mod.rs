//! Authentication: Argon2id password hashing and opaque session tokens.

pub(crate) mod activity;
pub mod protection;
pub mod recovery;
pub mod routes;
pub mod session;

use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use password_hash::{PasswordHash, SaltString};
use rand_core::OsRng;

use crate::error::{ApiError, ApiResult};

/// Minimum accepted password length. Long-but-simple beats short-but-gnarly, so
/// the only rule is length.
pub const MIN_PASSWORD_LEN: usize = 10;

pub fn hash_password(password: &str) -> ApiResult<String> {
    if password.len() > 1024 {
        return Err(ApiError::bad_request("password must be at most 1024 bytes"));
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ApiError::bad_request(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::internal(format!("password hashing failed: {e}")))
}

/// Verify a password against a stored PHC string.
///
/// A malformed stored hash is treated as a failed verification rather than an
/// error, so a corrupt row cannot be distinguished from a wrong password.
#[must_use]
pub fn verify_password(password: &str, stored: &str) -> bool {
    if password.len() > 1024 {
        return false;
    }
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn normalize_email(email: &str) -> ApiResult<String> {
    let email = email.trim().to_lowercase();
    // Deliberately minimal: a syntactic check here only rejects obvious typos.
    let valid = email.len() >= 3
        && email.len() <= 254
        && email.matches('@').count() == 1
        && !email.starts_with('@')
        && !email.ends_with('@')
        && email.split('@').nth(1).is_some_and(|d| d.contains('.'));

    if !valid {
        return Err(ApiError::bad_request("invalid email address"));
    }
    Ok(email)
}

/// Hash work runs off executor threads, with a process-wide memory/concurrency cap.
static PASSWORD_WORK: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();
pub async fn hash_password_async(password: String) -> ApiResult<String> {
    let permit = PASSWORD_WORK
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(4)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| ApiError::Conflict("Authentication busy. Retry shortly.".into()))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash_password(&password)
    })
    .await
    .map_err(|_| ApiError::internal("password worker failed"))?
}
pub async fn verify_password_async(
    password: String,
    stored: String,
    telemetry: &crate::observability::Telemetry,
) -> ApiResult<bool> {
    if PasswordHash::new(&stored).is_err() {
        telemetry.error(
            crate::observability::Component::Auth,
            crate::observability::ErrorClass::Internal,
        );
    }
    let permit = PASSWORD_WORK
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(4)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| ApiError::Conflict("Authentication busy. Retry shortly.".into()))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        verify_password(&password, &stored)
    })
    .await
    .map_err(|_| ApiError::internal("password worker failed"))
}
