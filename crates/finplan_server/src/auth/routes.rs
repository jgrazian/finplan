//! Registration, login, logout and identity endpoints.

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::session::{self, CurrentUser};
use super::{hash_password, normalize_email, verify_password};
use crate::error::{ApiError, ApiResult};
use crate::seed;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
}

#[derive(Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
}

async fn register(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> ApiResult<impl IntoResponse> {
    let email = normalize_email(&body.email)?;
    let password_hash = hash_password(&body.password)?;
    let id = uuid::Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(&id)
    .bind(&email)
    .bind(&password_hash)
    .bind(&body.display_name)
    .execute(&state.db)
    .await
    .map_err(|e| {
        crate::error::on_unique_violation(e, "an account with that email already exists")
    })?;

    // Give the new account the shared profile/tax library so their first
    // scenario has something to reference.
    seed::seed_user_library(&state.db, &id).await?;

    let token = session::issue(&state.db, &id).await?;
    let headers = session::set_cookie_header(&token, state.config.secure_cookies);

    Ok((
        axum::http::StatusCode::CREATED,
        headers,
        Json(UserResponse {
            id,
            email,
            display_name: body.display_name,
        }),
    ))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> ApiResult<impl IntoResponse> {
    let email = normalize_email(&body.email)?;

    let row: Option<(String, String, Option<String>)> =
        sqlx::query_as("SELECT id, password_hash, display_name FROM users WHERE email = ?1")
            .bind(&email)
            .fetch_optional(&state.db)
            .await?;

    // Verify against a dummy hash when the user is missing so that a wrong email
    // and a wrong password take comparable time.
    let Some((id, password_hash, display_name)) = row else {
        let _ = verify_password(&body.password, DUMMY_HASH);
        return Err(ApiError::Forbidden("invalid email or password".into()));
    };

    if !verify_password(&body.password, &password_hash) {
        return Err(ApiError::Forbidden("invalid email or password".into()));
    }

    let token = session::issue(&state.db, &id).await?;
    let headers = session::set_cookie_header(&token, state.config.secure_cookies);

    Ok((
        headers,
        Json(UserResponse {
            id,
            email,
            display_name,
        }),
    ))
}

/// A syntactically valid Argon2id hash of an unreachable password, used purely
/// to equalize the timing of the "no such user" branch of `login`.
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0c2E$\
mS/UPHZKGZ4V0EBS2JXsYQ9EIzM7YkqPBpZTMBiFY6Q";

async fn logout(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> ApiResult<impl IntoResponse> {
    if let Some(cookies) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
    {
        for pair in cookies.split(';') {
            if let Some(token) = pair
                .trim()
                .strip_prefix(session::COOKIE_NAME)
                .and_then(|r| r.strip_prefix('='))
                && !token.is_empty()
            {
                session::revoke(&state.db, token).await?;
            }
        }
    }

    let mut out = axum::http::HeaderMap::new();
    out.insert(
        axum::http::header::SET_COOKIE,
        session::clear_cookie(state.config.secure_cookies),
    );
    Ok((out, axum::http::StatusCode::NO_CONTENT))
}

async fn me(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<UserResponse>> {
    let display_name: Option<String> =
        sqlx::query_scalar("SELECT display_name FROM users WHERE id = ?1")
            .bind(&user.id)
            .fetch_optional(&state.db)
            .await?
            .flatten();

    Ok(Json(UserResponse {
        id: user.id,
        email: user.email,
        display_name,
    }))
}
