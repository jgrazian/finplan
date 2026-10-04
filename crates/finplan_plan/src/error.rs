//! What went wrong with a plan, independent of how it is reported.

use std::fmt;

/// A plan could not be built, edited or compiled. A server maps each variant
/// onto an HTTP status; the local app shows the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The request itself is wrong (a bad id, a value out of range).
    Invalid(String),
    /// The plan is well formed but cannot be lowered for the engine.
    Unprocessable(String),
    /// A named thing, such as "scenario", does not exist.
    NotFound(&'static str),
    /// The request clashes with the plan as it stands.
    Conflict(String),
    /// A bug in the plan code itself (a lowering that broke its own
    /// invariant), not something the caller can fix.
    Internal(String),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::Invalid(message)
            | PlanError::Unprocessable(message)
            | PlanError::Conflict(message) => f.write_str(message),
            PlanError::Internal(message) => write!(f, "internal error: {message}"),
            PlanError::NotFound(what) => write!(f, "{what} not found"),
        }
    }
}

impl std::error::Error for PlanError {}

impl PlanError {
    pub fn invalid(message: impl Into<String>) -> Self {
        PlanError::Invalid(message.into())
    }

    pub fn unprocessable(message: impl Into<String>) -> Self {
        PlanError::Unprocessable(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        PlanError::Internal(message.into())
    }

    /// The HTTP status the server answers this with. The browser raises the
    /// same status, so a screen cannot tell which home a refusal came from.
    pub fn status(&self) -> u16 {
        match self {
            PlanError::Invalid(_) => 400,
            PlanError::NotFound(_) => 404,
            PlanError::Conflict(_) => 409,
            PlanError::Unprocessable(_) => 422,
            PlanError::Internal(_) => 500,
        }
    }

    /// The machine-readable `error.code` of the server's error body.
    pub fn code(&self) -> &'static str {
        match self {
            PlanError::Invalid(_) => "bad_request",
            PlanError::NotFound(_) => "not_found",
            PlanError::Conflict(_) => "conflict",
            PlanError::Unprocessable(_) => "unprocessable",
            PlanError::Internal(_) => "internal",
        }
    }

    /// The `error.message` the server shows a caller. An internal error's
    /// detail is not for callers (it may hold plan text), so it reads as the
    /// server's own "internal server error"; [`Display`](fmt::Display) keeps
    /// the detail for a log.
    pub fn message(&self) -> String {
        match self {
            PlanError::Internal(_) => "internal server error".to_string(),
            other => other.to_string(),
        }
    }

    /// The server's error body, `{"error": {"code", "message"}}`.
    pub fn body(&self) -> serde_json::Value {
        serde_json::json!({ "error": { "code": self.code(), "message": self.message() } })
    }
}

pub type PlanResult<T> = Result<T, PlanError>;

#[cfg(test)]
mod tests {
    use super::PlanError;

    #[test]
    fn every_error_reports_status_code_and_message() {
        let cases = [
            (PlanError::invalid("no"), 400, "bad_request", "no"),
            (
                PlanError::NotFound("account"),
                404,
                "not_found",
                "account not found",
            ),
            (
                PlanError::Conflict("taken".into()),
                409,
                "conflict",
                "taken",
            ),
            (PlanError::unprocessable("odd"), 422, "unprocessable", "odd"),
            (
                PlanError::internal("secret"),
                500,
                "internal",
                "internal server error",
            ),
        ];
        for (error, status, code, message) in cases {
            assert_eq!(error.status(), status);
            assert_eq!(error.code(), code);
            assert_eq!(error.message(), message);
            assert_eq!(error.body()["error"]["code"], code);
        }
    }
}
