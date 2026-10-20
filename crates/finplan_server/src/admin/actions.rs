//! The admin listener's writes. Each answers with a redirect back to a page
//! and a notice code (`pages::notice_text`), and logs `admin.action`.

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use serde::Deserialize;

use super::html::{Failure, PageResult, seg};
use super::pages::MESSAGE_STATUSES;
use crate::state::AppState;

/// The provider a grant made here is stored under. Only these rows are
/// revoked here: a provider's own are reconciled from the provider, which
/// would undo any change made here.
const MANUAL: &str = "manual";

fn back_to_user(id: &str, notice: &str) -> Redirect {
    Redirect::to(&format!("/users/{}?notice={notice}", seg(id)))
}

async fn kind_of(state: &AppState, id: &str) -> PageResult<String> {
    sqlx::query_scalar("SELECT kind FROM users WHERE id = ?1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| Failure::not_found("user"))
}

#[derive(Deserialize)]
pub struct GrantPro {
    /// The last day of access, UTC.
    until: String,
}

/// Comp Pro through the end of `until`, as a `manual` subscription. A live
/// subscription from a payment provider is left alone.
pub async fn grant_pro(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<GrantPro>,
) -> PageResult<Redirect> {
    if kind_of(&state, &id).await? == "guest" {
        return Err(Failure::new(
            StatusCode::CONFLICT,
            "A guest cannot hold Pro: guest limits apply whatever the subscription.",
        ));
    }
    let until: jiff::civil::Date = form
        .until
        .trim()
        .parse()
        .map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "The date is not valid."))?;
    let until = until.to_string();
    let mut tx = state.db.begin().await?;
    let existing: Option<(String, String)> =
        sqlx::query_as("SELECT provider, state FROM subscriptions WHERE user_id = ?1")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((provider, current)) = existing
        && provider != MANUAL
        && current != "expired"
    {
        return Err(Failure::new(
            StatusCode::CONFLICT,
            format!(
                "This user has a {current} subscription with {provider}; change it with the provider."
            ),
        ));
    }
    // Access runs to the end of the chosen day: the first second of the next.
    let granted = sqlx::query(
        "INSERT INTO subscriptions (user_id, provider, subscription_id, state, access_until, revision)
         SELECT ?1, ?2, ?2 || '-' || ?1, 'active', unixepoch(?3, '+1 day'), 0
          WHERE unixepoch(?3, '+1 day') > unixepoch()
         ON CONFLICT (user_id) DO UPDATE
            SET provider = excluded.provider, subscription_id = excluded.subscription_id,
                state = 'active', access_until = excluded.access_until,
                revision = subscriptions.revision + 1",
    )
    .bind(&id)
    .bind(MANUAL)
    .bind(&until)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if granted == 0 {
        return Err(Failure::new(
            StatusCode::BAD_REQUEST,
            "Choose a day that has not ended yet.",
        ));
    }
    tx.commit().await?;
    tracing::info!(event = "admin.action", action = "grant_pro", user_id = %id, until);
    Ok(back_to_user(&id, "granted"))
}

/// End a manual grant now.
pub async fn revoke_pro(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> PageResult<Redirect> {
    let revoked = sqlx::query(
        "UPDATE subscriptions
            SET state = 'expired', access_until = MIN(access_until, unixepoch()),
                revision = revision + 1
          WHERE user_id = ?1 AND provider = ?2 AND state <> 'expired'",
    )
    .bind(&id)
    .bind(MANUAL)
    .execute(&state.db)
    .await?
    .rows_affected();
    if revoked == 0 {
        return Err(Failure::new(
            StatusCode::CONFLICT,
            "There is no manual Pro grant to revoke.",
        ));
    }
    tracing::info!(event = "admin.action", action = "revoke_pro", user_id = %id);
    Ok(back_to_user(&id, "revoked"))
}

/// Give back this month's goal seeks, AI drafts, plan chat messages and
/// offload budget.
pub async fn reset_quotas(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> PageResult<Redirect> {
    kind_of(&state, &id).await?;
    let mut tx = state.db.begin().await?;
    for table in [
        "monthly_goal_seeks",
        "monthly_ai_drafts",
        "monthly_ai_plan_chats",
        "monthly_offload_spend",
    ] {
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE user_id = ?1 AND month = strftime('%Y-%m', 'now')"
        ))
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    tracing::info!(event = "admin.action", action = "reset_quotas", user_id = %id);
    Ok(back_to_user(&id, "quotas_reset"))
}

/// Revoke every session; the user signs in again (a guest cannot).
pub async fn sign_out(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> PageResult<Redirect> {
    kind_of(&state, &id).await?;
    let sessions = sqlx::query("DELETE FROM sessions WHERE user_id = ?1")
        .bind(&id)
        .execute(&state.db)
        .await?
        .rows_affected();
    tracing::info!(event = "admin.action", action = "sign_out", user_id = %id, sessions);
    Ok(back_to_user(&id, "signed_out"))
}

#[derive(Deserialize)]
pub struct ConfirmDelete {
    /// The user's email, typed again.
    confirm: String,
}

/// Delete the user; every plan, run, session and allowance goes with it
/// (ON DELETE CASCADE), as when a user deletes their own account. Their AI
/// spend stays in the ledger, unowned.
pub async fn delete_user(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<ConfirmDelete>,
) -> PageResult<Redirect> {
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = ?1")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| Failure::not_found("user"))?;
    if !form.confirm.trim().eq_ignore_ascii_case(&email) {
        return Err(Failure::new(
            StatusCode::BAD_REQUEST,
            "The email typed does not match; nothing was deleted.",
        ));
    }
    let deleted = sqlx::query("DELETE FROM users WHERE id = ?1")
        .bind(&id)
        .execute(&state.db)
        .await?
        .rows_affected();
    tracing::info!(event = "admin.action", action = "delete_user", user_id = %id, deleted);
    Ok(Redirect::to("/users?notice=deleted"))
}

#[derive(Deserialize)]
pub struct MessageStatus {
    status: String,
    /// The page to return to; only this listener's own paths are followed.
    back: Option<String>,
}

pub async fn set_message_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<MessageStatus>,
) -> PageResult<Redirect> {
    if !MESSAGE_STATUSES.contains(&form.status.as_str()) {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "Unknown status."));
    }
    let updated = sqlx::query("UPDATE contact_messages SET status = ?2 WHERE id = ?1")
        .bind(id)
        .bind(&form.status)
        .execute(&state.db)
        .await?
        .rows_affected();
    if updated == 0 {
        return Err(Failure::not_found("message"));
    }
    tracing::info!(event = "admin.action", action = "message_status", message_id = id, status = %form.status);
    let back = form
        .back
        .filter(|b| b.starts_with('/') && !b.starts_with("//") && !b.contains('\\'))
        .unwrap_or_else(|| "/inbox".into());
    let joiner = if back.contains('?') { '&' } else { '?' };
    Ok(Redirect::to(&format!("{back}{joiner}notice=status")))
}
