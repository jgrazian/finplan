use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Custom error types for the FinPlan API
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Portfolio not found: {0}")]
    PortfolioNotFound(String),

    #[error("Simulation not found: {0}")]
    SimulationNotFound(String),

    #[error("Simulation run not found: {0}")]
    SimulationRunNotFound(String),

    #[error("Invalid parameter: {field} - {message}")]
    ValidationError { field: String, message: String },

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Mutex lock error")]
    LockError,

    #[error("Internal server error")]
    InternalError,
}

impl From<rusqlite::Error> for ApiError {
    fn from(err: rusqlite::Error) -> Self {
        match err {
            rusqlite::Error::QueryReturnedNoRows => ApiError::DatabaseError(err.to_string()),
            _ => ApiError::DatabaseError(err.to_string()),
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        ApiError::SerializationError(err.to_string())
    }
}

impl<T> From<std::sync::PoisonError<T>> for ApiError {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        ApiError::LockError
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            ApiError::PortfolioNotFound(_)
            | ApiError::SimulationNotFound(_)
            | ApiError::SimulationRunNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),

            ApiError::ValidationError { .. } => (StatusCode::BAD_REQUEST, self.to_string()),

            ApiError::SerializationError(_) => (StatusCode::BAD_REQUEST, self.to_string()),

            ApiError::DatabaseError(_) => {
                eprintln!("Database error: {}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal database error".to_string(),
                )
            }

            ApiError::LockError => {
                eprintln!("Lock error: {}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }

            ApiError::InternalError => {
                eprintln!("Internal error: {}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };

        let body = Json(json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

/// Helper type for API results
pub type ApiResult<T> = Result<T, ApiError>;
