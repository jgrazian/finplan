//! API error type and its HTTP representation.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),

    #[error("authentication required")]
    Unauthorized,

    #[error("{0}")]
    Forbidden(String),

    #[error("{0} not found")]
    NotFound(&'static str),

    #[error("{0}")]
    Conflict(String),

    #[error("{message}")]
    RateLimited {
        message: String,
        retry_after_seconds: u64,
    },

    /// The stored scenario cannot be lowered into a `SimulationConfig`.
    #[error("{0}")]
    Unprocessable(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        ApiError::BadRequest(msg.into())
    }

    pub fn unprocessable(msg: impl Into<String>) -> Self {
        ApiError::Unprocessable(msg.into())
    }

    pub fn rate_limited(message: impl Into<String>, retry_after_seconds: u64) -> Self {
        ApiError::RateLimited {
            message: message.into(),
            retry_after_seconds,
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        ApiError::Internal(msg.into())
    }

    /// A foreign-key violation means the caller named a row that does not
    /// exist — an unset account or event id, say. That is the request's fault,
    /// not the server's, so it must not surface as a 500.
    fn is_dangling_reference(&self) -> bool {
        matches!(self, ApiError::Database(sqlx::Error::Database(db)) if db.is_foreign_key_violation())
    }

    fn status(&self) -> StatusCode {
        if self.is_dangling_reference() {
            return StatusCode::BAD_REQUEST;
        }
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            ApiError::Unprocessable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Database(_) | ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        if self.is_dangling_reference() {
            return "bad_request";
        }
        match self {
            ApiError::BadRequest(_) => "bad_request",
            ApiError::Unauthorized => "unauthorized",
            ApiError::Forbidden(_) => "forbidden",
            ApiError::NotFound(_) => "not_found",
            ApiError::Conflict(_) => "conflict",
            ApiError::RateLimited { .. } => "rate_limited",
            ApiError::Unprocessable(_) => "unprocessable",
            ApiError::Database(_) | ApiError::Internal(_) => "internal",
        }
    }
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct ErrorDetail {
    pub code: &'static str,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code();
        let retry_after = match &self {
            ApiError::RateLimited {
                retry_after_seconds,
                ..
            } => Some(*retry_after_seconds),
            _ => None,
        };

        // Never render raw SQL errors or internal strings to logs: they can
        // include SQL parameters, financial data, or user-authored text. The
        // outer HTTP middleware records one classified failure for this response.
        if self.is_dangling_reference() {
            return (
                status,
                Json(ErrorBody {
                    error: ErrorDetail {
                        code,
                        message: "a referenced account, asset or event does not exist".to_string(),
                    },
                }),
            )
                .into_response();
        }

        let message = match &self {
            ApiError::Database(_) | ApiError::Internal(_) => "internal server error".to_string(),
            other => other.to_string(),
        };

        let mut response = (
            status,
            Json(ErrorBody {
                error: ErrorDetail { code, message },
            }),
        )
            .into_response();
        if let Some(seconds) = retry_after {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&seconds.to_string()).expect("retry-after is numeric"),
            );
        }
        let class = match self {
            ApiError::Database(_) => Some(crate::observability::ErrorClass::Database),
            ApiError::Internal(_) => Some(crate::observability::ErrorClass::Internal),
            _ => None,
        };
        if let Some(class) = class {
            response
                .extensions_mut()
                .insert(crate::observability::HttpFailure(class));
        }
        response
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// Map a UNIQUE-constraint violation onto a 409 with a caller-supplied message.
pub fn on_unique_violation(err: sqlx::Error, msg: &str) -> ApiError {
    if let sqlx::Error::Database(ref db) = err
        && db.is_unique_violation()
    {
        return ApiError::Conflict(msg.to_string());
    }
    ApiError::Database(err)
}
