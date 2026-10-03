//! Guest access (spec 17): a placeholder user with a normal session, so the
//! workbench is usable before anyone signs up, and the routes that turn a guest
//! into an account in place or clean up the ones nobody came back for.

use axum::extract::{ConnectInfo, Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use std::net::SocketAddr;

use super::routes::{RegisterCredentials, load_user};
use super::session::{self, CurrentUser};
use super::{hash_password_async, normalize_email, protection};
use crate::db::Db;
use crate::error::{ApiError, ApiResult, on_unique_violation};
use crate::observability::{AuthAction, AuthOutcome, EventFields, RequestContext};
use crate::seed;
use crate::state::AppState;

/// `.invalid` is reserved (RFC 2606), so a placeholder can never be a mailbox.
const PLACEHOLDER_DOMAIN: &str = "@guest.invalid";

/// The `default_iterations` of an account, from the column default in
/// `0001_init.sql`.
const ACCOUNT_DEFAULT_ITERATIONS: i64 = 2000;

/// A drive-by guest that never built a plan is dropped after this long.
const EMPTY_GUEST_HOURS: i64 = 24;

/// Grace before a guest can be judged idle. `create` inserts the user a moment
/// before it issues the session, and that gap must not read as "no sessions".
const NEW_GUEST_GRACE_HOURS: i64 = 1;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/guest", post(create))
        .route("/guest/claim", post(claim))
}

/// Placeholder addresses are not for people: refuse them at registration and
/// claim, or someone could take another guest's address.
pub(super) fn refuse_placeholder_email(email: &str) -> ApiResult<()> {
    if email.ends_with(PLACEHOLDER_DOMAIN) {
        return Err(ApiError::bad_request("invalid email address"));
    }
    Ok(())
}

/// What a guest's `default_iterations` is set to at creation, so the web's run
/// effort does not start above the guest cap. Self-hosted guests are uncapped
/// and keep the column default.
fn guest_default_iterations(state: &AppState) -> i64 {
    if state.config.hosted {
        state
            .config
            .guest_max_iterations
            .min(state.config.max_iterations) as i64
    } else {
        ACCOUNT_DEFAULT_ITERATIONS
    }
}

async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> ApiResult<impl IntoResponse> {
    if state.config.hosted {
        if !state.config.guest_access {
            return Err(ApiError::Forbidden(
                "Guest access is not available. Sign in or create an account.".into(),
            ));
        }
        protection::guest_attempt(&state, &protection::peer_of(connect.map(|Extension(c)| c)))?;
    }

    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, kind, default_iterations)
         VALUES (?1, ?2, '!', 'guest', ?3)",
    )
    .bind(&id)
    .bind(format!("guest-{id}{PLACEHOLDER_DOMAIN}"))
    .bind(guest_default_iterations(&state))
    .execute(&state.db)
    .await?;

    RequestContext::authenticate(&id);
    state.telemetry.auth(
        AuthAction::GuestCreated,
        AuthOutcome::Succeeded,
        &EventFields {
            user_id: Some(&id),
            ..Default::default()
        },
    );
    seed::seed_user_library(&state.db, &id).await?;

    let token = session::issue(&state.db, &id, session::user_agent_of(&headers).as_deref()).await?;
    let cookie = session::set_cookie_header(&token, state.config.secure_cookies);
    let user = load_user(&state, &id).await?;

    Ok((StatusCode::CREATED, cookie, user))
}

/// Turn the caller's guest into an account. The user id does not change, so
/// plans, runs and suggestions stay where they are.
async fn claim(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: CurrentUser,
    Json(body): Json<RegisterCredentials>,
) -> ApiResult<impl IntoResponse> {
    if !user.guest {
        return Err(ApiError::Conflict(
            "You already have an account. Sign out to create another.".into(),
        ));
    }
    if !state.config.registration_open {
        return Err(ApiError::Forbidden(
            "New registrations are currently closed. Existing users can still sign in.".into(),
        ));
    }
    if body.password != body.password_confirmation {
        return Err(ApiError::bad_request("passwords do not match"));
    }
    let email = normalize_email(&body.email)?;
    refuse_placeholder_email(&email)?;
    if state.config.hosted {
        protection::account_attempt(&state, &email)?;
    }
    let password_hash = hash_password_async(body.password.clone()).await?;

    let mut tx = state.db.begin().await?;
    // A guest whose effort preference is still the cap it was created with never
    // chose it; an account starts from the ordinary default instead.
    let changed = sqlx::query(
        "UPDATE users
            SET email = ?1, password_hash = ?2, display_name = ?3, kind = 'account',
                default_iterations = CASE WHEN default_iterations = ?4 THEN ?5
                                          ELSE default_iterations END,
                updated_at = datetime('now')
          WHERE id = ?6 AND kind = 'guest'",
    )
    .bind(&email)
    .bind(&password_hash)
    .bind(&body.display_name)
    .bind(guest_default_iterations(&state))
    .bind(ACCOUNT_DEFAULT_ITERATIONS)
    .bind(&user.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        on_unique_violation(
            e,
            "An account with that email exists. Sign in to bring this plan with you.",
        )
    })?
    .rows_affected();
    if changed == 0 {
        return Err(ApiError::Conflict(
            "You already have an account. Sign out to create another.".into(),
        ));
    }
    // The guest's session is revoked and a fresh one issued, the same as a login.
    sqlx::query("DELETE FROM sessions WHERE user_id = ?1")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    state.telemetry.auth(
        AuthAction::GuestClaimed,
        AuthOutcome::Succeeded,
        &EventFields {
            user_id: Some(&user.id),
            ..Default::default()
        },
    );

    let token = session::issue(
        &state.db,
        &user.id,
        session::user_agent_of(&headers).as_deref(),
    )
    .await?;
    let cookie = session::set_cookie_header(&token, state.config.secure_cookies);
    let user = load_user(&state, &user.id).await?;

    Ok((cookie, user))
}

/// A sign-in from a browser that still holds a guest's cookie deletes that
/// guest: the web exported its plan before calling login and imports it into
/// the account afterwards, so nothing is left behind for the janitor.
pub(super) async fn replace_guest(
    state: &AppState,
    headers: &HeaderMap,
    signing_in: &str,
) -> ApiResult<()> {
    let Some(token) = session::token_of(headers) else {
        return Ok(());
    };
    let replaced: Option<String> = sqlx::query_scalar(
        "DELETE FROM users
          WHERE kind = 'guest' AND id <> ?1
            AND id IN (SELECT user_id FROM sessions WHERE token_hash = ?2)
      RETURNING id",
    )
    .bind(signing_in)
    .bind(session::hash_token(&token))
    .fetch_optional(&state.db)
    .await?;
    if let Some(guest) = replaced {
        tracing::info!(
            event = "auth.guest_replaced",
            guest_id = guest,
            user_id = signing_in
        );
    }
    Ok(())
}

/// Delete guests nobody is coming back for, and report `(empty, with_plans)`:
/// whether each had an active plan when it went. Accounts are never touched.
///
/// A guest goes when it has no active plan and is over a day old, or when no
/// session of it has been seen within `retention_days` (which includes having
/// none left). The second rule leaves guests under an hour old alone, see
/// `NEW_GUEST_GRACE_HOURS`.
pub async fn purge(db: &Db, retention_days: i64) -> Result<(u64, u64), sqlx::Error> {
    const NO_ACTIVE_PLAN: &str = "NOT EXISTS (SELECT 1 FROM scenarios s
        WHERE s.user_id = users.id AND s.status = 'active')";
    let expired = format!(
        "kind = 'guest' AND (
            ({NO_ACTIVE_PLAN} AND created_at <= datetime('now', '-{EMPTY_GUEST_HOURS} hours'))
            OR (created_at <= datetime('now', '-{NEW_GUEST_GRACE_HOURS} hours')
                AND NOT EXISTS (SELECT 1 FROM sessions x
                    WHERE x.user_id = users.id AND x.last_seen > datetime('now', ?1)))
        )"
    );
    let cutoff = format!("-{} days", retention_days.max(1));

    // One transaction, so a guest that gains a plan between the two statements
    // cannot be counted as empty. Every row the second statement finds has a
    // plan, because the first took the ones without.
    let mut tx = db.begin().await?;
    let empty = sqlx::query(&format!(
        "DELETE FROM users WHERE {expired} AND {NO_ACTIVE_PLAN}"
    ))
    .bind(&cutoff)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let with_plans = sqlx::query(&format!("DELETE FROM users WHERE {expired}"))
        .bind(&cutoff)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    tx.commit().await?;
    Ok((empty, with_plans))
}
