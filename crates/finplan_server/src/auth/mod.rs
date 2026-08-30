//! Authentication: Argon2id password hashing and opaque session tokens.

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
    let Ok(parsed) = PasswordHash::new(stored) else {
        tracing::error!("stored password hash is not a valid PHC string");
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
        && email.matches('@').count() == 1
        && !email.starts_with('@')
        && !email.ends_with('@')
        && email.split('@').nth(1).is_some_and(|d| d.contains('.'));

    if !valid {
        return Err(ApiError::bad_request("invalid email address"));
    }
    Ok(email)
}
