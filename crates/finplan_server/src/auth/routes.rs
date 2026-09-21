//! Registration, login, logout, identity — and the account itself: profile,
//! defaults, password, sessions and deletion.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::session::{self, CurrentUser};
use super::{hash_password_async, normalize_email, verify_password_async};
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::seed;
use crate::state::AppState;
use ts_rs::TS;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me).delete(destroy_account))
        .route("/profile", put(update_profile))
        .route("/preferences", put(update_preferences))
        .route("/password", post(change_password))
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}", delete(revoke_session))
        .merge(super::recovery::router())
}

#[derive(Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

/// The credentials needed to create an account.
///
/// Keeping this separate from `Credentials` makes confirmation mandatory for
/// registration without imposing an irrelevant field on sign-in requests.
#[derive(Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct RegisterCredentials {
    pub email: String,
    pub password: String,
    pub password_confirmation: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

/// Which ground the interface is drawn on.
///
/// `System` is not a third palette: it is the absence of a choice, which the
/// stylesheet answers with `prefers-color-scheme`. Storing it as a value
/// rather than a null keeps "follow the machine" a decision the user made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, TS)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
#[ts(export)]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

/// Which accent ramp the palette is built from.
///
/// Ground, ink and dividers are shared; only this ramp turns, so one value
/// names the whole set. The three are the same ramp rotated in OKLCH, which is
/// why a step means the same weight whichever hue is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, TS)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
#[ts(export)]
pub enum Accent {
    Blue,
    Green,
    Purple,
}

/// The signed-in user: who they are, plus the defaults that seed new work.
#[derive(Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub email_verified_at: Option<String>,
    pub display_name: Option<String>,
    /// Seeds a new scenario's birth date; an existing scenario keeps its own.
    pub birth_date: Option<String>,
    pub default_iterations: i64,
    pub default_duration_years: i64,
    /// Re-run the active scenario by itself once an edit has settled.
    pub auto_run: bool,
    /// Appearance is the account's, not the scenario's: the same plan looks
    /// the same wherever it is opened.
    pub theme_mode: ThemeMode,
    pub accent: Accent,
    pub created_at: String,
}

const USER_COLUMNS: &str =
    "id, email, email_verified_at, display_name, birth_date, default_iterations,
     default_duration_years, auto_run, theme_mode, accent, created_at";

async fn load_user(state: &AppState, id: &str) -> ApiResult<Json<UserResponse>> {
    let row: Option<UserResponse> =
        sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    row.map(Json).ok_or(ApiError::NotFound("user"))
}

async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterCredentials>,
) -> ApiResult<impl IntoResponse> {
    if body.password != body.password_confirmation {
        return Err(ApiError::bad_request("passwords do not match"));
    }
    let email = normalize_email(&body.email)?;
    if state.config.hosted {
        super::protection::account_attempt(&email)?;
    }
    let password_hash = hash_password_async(body.password.clone()).await?;
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
    .map_err(|e| on_unique_violation(e, "an account with that email already exists"))?;

    // Give the new account the shared profile/tax library so their first
    // scenario has something to reference.
    seed::seed_user_library(&state.db, &id).await?;

    let token = session::issue(&state.db, &id, session::user_agent_of(&headers).as_deref()).await?;
    let cookie = session::set_cookie_header(&token, state.config.secure_cookies);
    let user = load_user(&state, &id).await?;

    Ok((axum::http::StatusCode::CREATED, cookie, user))
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Credentials>,
) -> ApiResult<impl IntoResponse> {
    let email = normalize_email(&body.email)?;
    if state.config.hosted {
        super::protection::account_attempt(&email)?;
    }

    let row: Option<(String, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?1")
            .bind(&email)
            .fetch_optional(&state.db)
            .await?;

    // Verify against a dummy hash when the user is missing so that a wrong email
    // and a wrong password take comparable time.
    let Some((id, password_hash)) = row else {
        let _ = verify_password_async(body.password.clone(), DUMMY_HASH.to_owned()).await?;
        return Err(ApiError::Forbidden("invalid email or password".into()));
    };

    if !verify_password_async(body.password.clone(), password_hash).await? {
        return Err(ApiError::Forbidden("invalid email or password".into()));
    }

    let token = session::issue(&state.db, &id, session::user_agent_of(&headers).as_deref()).await?;
    let cookie = session::set_cookie_header(&token, state.config.secure_cookies);
    let user = load_user(&state, &id).await?;

    Ok((cookie, user))
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
    load_user(&state, &user.id).await
}

// ===========================================================================
// Profile and preferences
// ===========================================================================

/// A full replacement of the editable identity block, not a merge.
///
/// The account form is a Save/Discard pair over every field at once, so an
/// absent value means "clear this" rather than "leave it alone" — which is the
/// only way an emptied birth date or display name can ever be sent.
#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateUserProfile {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub birth_date: Option<String>,
}

async fn update_profile(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<UpdateUserProfile>,
) -> ApiResult<Json<UserResponse>> {
    let display_name = blank_to_none(body.display_name);
    let birth_date = match blank_to_none(body.birth_date) {
        Some(date) => Some(validate_date(&date)?),
        None => None,
    };

    sqlx::query(
        "UPDATE users
            SET display_name = ?1, birth_date = ?2, updated_at = datetime('now')
          WHERE id = ?3",
    )
    .bind(&display_name)
    .bind(&birth_date)
    .bind(&user.id)
    .execute(&state.db)
    .await?;

    load_user(&state, &user.id).await
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct UpdatePreferences {
    pub default_iterations: i64,
    pub default_duration_years: i64,
    pub auto_run: bool,
    pub theme_mode: ThemeMode,
    pub accent: Accent,
}

async fn update_preferences(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<UpdatePreferences>,
) -> ApiResult<Json<UserResponse>> {
    if body.default_iterations < 1 || body.default_iterations as usize > state.config.max_iterations
    {
        return Err(ApiError::bad_request(format!(
            "default iterations must be between 1 and {}",
            state.config.max_iterations
        )));
    }
    if !(1..=120).contains(&body.default_duration_years) {
        return Err(ApiError::bad_request(
            "default duration must be between 1 and 120 years",
        ));
    }

    sqlx::query(
        "UPDATE users
            SET default_iterations = ?1, default_duration_years = ?2, auto_run = ?3,
                theme_mode = ?4, accent = ?5,
                updated_at = datetime('now')
          WHERE id = ?6",
    )
    .bind(body.default_iterations)
    .bind(body.default_duration_years)
    .bind(body.auto_run)
    .bind(body.theme_mode)
    .bind(body.accent)
    .bind(&user.id)
    .execute(&state.db)
    .await?;

    load_user(&state, &user.id).await
}

// ===========================================================================
// Password
// ===========================================================================

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct PasswordChange {
    pub current_password: String,
    pub new_password: String,
    pub new_password_confirmation: String,
}

/// Change the password, and end every other session while doing it.
///
/// Someone changing a password is usually revoking access, so leaving the old
/// sessions alive would defeat the act. The session that made the request
/// survives, so the change does not sign you out of the screen you are on.
async fn change_password(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<PasswordChange>,
) -> ApiResult<impl IntoResponse> {
    if body.new_password != body.new_password_confirmation {
        return Err(ApiError::bad_request("passwords do not match"));
    }

    let stored: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?1")
        .bind(&user.id)
        .fetch_one(&state.db)
        .await?;

    if !verify_password_async(body.current_password, stored).await? {
        return Err(ApiError::Forbidden("current password is incorrect".into()));
    }

    let password_hash = hash_password_async(body.new_password).await?;
    sqlx::query("UPDATE users SET password_hash = ?1, updated_at = datetime('now') WHERE id = ?2")
        .bind(&password_hash)
        .bind(&user.id)
        .execute(&state.db)
        .await?;

    sqlx::query("DELETE FROM sessions WHERE user_id = ?1 AND public_id <> ?2")
        .bind(&user.id)
        .bind(&user.session_id)
        .execute(&state.db)
        .await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ===========================================================================
// Sessions
// ===========================================================================

#[derive(Serialize, sqlx::FromRow, TS)]
#[ts(export)]
pub struct SessionInfo {
    /// Opaque, and unrelated to the token: listing devices hands out nothing
    /// derived from a credential.
    pub id: String,
    /// Verbatim `User-Agent`; the client names the device from it.
    pub user_agent: Option<String>,
    pub created_at: String,
    pub last_seen: String,
    pub expires_at: String,
    /// The session this request arrived on.
    pub current: bool,
}

async fn list_sessions(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<Vec<SessionInfo>>> {
    let rows: Vec<SessionInfo> = sqlx::query_as(
        "SELECT public_id AS id, user_agent, created_at, last_seen, expires_at,
                (public_id = ?2) AS current
           FROM sessions
          WHERE user_id = ?1 AND expires_at > datetime('now')
          ORDER BY last_seen DESC",
    )
    .bind(&user.id)
    .bind(&user.session_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// Revoke one session. Revoking your own is simply a sign-out.
async fn revoke_session(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let deleted = sqlx::query("DELETE FROM sessions WHERE user_id = ?1 AND public_id = ?2")
        .bind(&user.id)
        .bind(&id)
        .execute(&state.db)
        .await?
        .rows_affected();

    if deleted == 0 {
        return Err(ApiError::NotFound("session"));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ===========================================================================
// Deletion
// ===========================================================================

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct DeleteAccount {
    /// Re-typed rather than inferred from the cookie: this is unrecoverable.
    pub password: String,
}

async fn destroy_account(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<DeleteAccount>,
) -> ApiResult<impl IntoResponse> {
    let stored: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?1")
        .bind(&user.id)
        .fetch_one(&state.db)
        .await?;

    if !verify_password_async(body.password, stored).await? {
        return Err(ApiError::Forbidden("password is incorrect".into()));
    }

    // Every scenario, run, profile and session hangs off `users` with
    // ON DELETE CASCADE, so this one statement takes the whole account.
    sqlx::query("DELETE FROM users WHERE id = ?1")
        .bind(&user.id)
        .execute(&state.db)
        .await?;

    let mut out = axum::http::HeaderMap::new();
    out.insert(
        axum::http::header::SET_COOKIE,
        session::clear_cookie(state.config.secure_cookies),
    );
    Ok((out, axum::http::StatusCode::NO_CONTENT))
}

fn blank_to_none(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn validate_date(text: &str) -> ApiResult<String> {
    text.parse::<jiff::civil::Date>()
        .map(|d| d.to_string())
        .map_err(|e| ApiError::bad_request(format!("invalid birth_date '{text}': {e}")))
}
