//! Private contact messages submitted by signed-in users.

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::auth::activity::{ActivityFields, Submitted};
use crate::auth::session::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::observability::{EventFields, Operation, Resource};
use crate::state::AppState;

const MAX_MESSAGE_CHARS: usize = 2_000;
const MAX_MESSAGES_PER_HOUR: i64 = 5;
const RETRY_AFTER_SECONDS: u64 = 60 * 60;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/contact-messages", post(create))
        // A 2,000-character Unicode message can occupy more than 2,000 bytes.
        // This remains far below Axum's global default while leaving room for
        // JSON escaping and the topic field.
        .layer(DefaultBodyLimit::max(16 * 1024))
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename_all = "snake_case")]
pub enum ContactTopic {
    Question,
    Feedback,
    BugReport,
}

impl ContactTopic {
    fn as_str(self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Feedback => "feedback",
            Self::BugReport => "bug_report",
        }
    }
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct CreateContactMessage {
    pub topic: ContactTopic,
    pub message: String,
}

impl ActivityFields for CreateContactMessage {
    const FIELDS: &'static [&'static str] = &["topic", "message"];
}

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename_all = "snake_case")]
pub enum ContactMessageStatus {
    New,
    Reviewed,
    Closed,
}

/// A receipt only: the submitted message is intentionally not echoed back.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ContactMessageReceipt {
    pub id: i64,
    pub topic: ContactTopic,
    pub status: ContactMessageStatus,
    pub created_at: String,
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(Submitted { body, fields }): Json<Submitted<CreateContactMessage>>,
) -> ApiResult<(StatusCode, Json<ContactMessageReceipt>)> {
    let message = body.message.trim();
    if message.is_empty() {
        return Err(ApiError::bad_request("message cannot be empty"));
    }
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(ApiError::bad_request(format!(
            "message cannot exceed {MAX_MESSAGE_CHARS} characters"
        )));
    }

    // BEGIN IMMEDIATE serializes the count and insert, so two simultaneous
    // submissions cannot both slip through the same remaining slot.
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contact_messages
          WHERE user_id = ?1 AND created_at >= datetime('now', '-1 hour')",
    )
    .bind(&user.id)
    .fetch_one(&mut *tx)
    .await?;
    if recent >= MAX_MESSAGES_PER_HOUR {
        return Err(ApiError::rate_limited(
            "Too many messages. Try again in an hour.",
            RETRY_AFTER_SECONDS,
        ));
    }

    let (id, created_at): (i64, String) = sqlx::query_as(
        "INSERT INTO contact_messages (user_id, topic, message)
         VALUES (?1, ?2, ?3)
         RETURNING id, created_at",
    )
    .bind(&user.id)
    .bind(body.topic.as_str())
    .bind(message)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    state.telemetry.mutation(
        Resource::ContactMessage,
        Operation::Created,
        &EventFields {
            user_id: Some(&user.id),
            resource_id: Some(id),
            fields: &fields,
            ..Default::default()
        },
    );

    Ok((
        StatusCode::CREATED,
        Json(ContactMessageReceipt {
            id,
            topic: body.topic,
            status: ContactMessageStatus::New,
            created_at,
        }),
    ))
}
