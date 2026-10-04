//! Errors, shaped as the server's.
//!
//! A failed call throws a JS string holding the JSON of an [`EngineError`]:
//! the HTTP `status` and `code` the server would have answered with, the
//! `message`, and `body`, the server's own error body (`{"error": {"code",
//! "message"}}`). `web/lib/api/http.ts`'s `ApiError(status, code, message,
//! body)` is built from exactly these four, so a screen cannot tell a local
//! refusal from a remote one.
//!
//! The one exception is a panic inside the engine: the instance is dead
//! afterwards, and what is thrown is an `EngineError` with status 500, code
//! `"panic"`, whose message is the panic's. Discard the instance (restart the
//! worker) when you see it.

use finplan_core::error::SimulationError;
use finplan_plan::PlanError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// What a failed call throws, as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EngineError {
    /// The HTTP status the server answers the same refusal with.
    pub status: u16,
    pub code: String,
    pub message: String,
    /// The server's error body, `{"error": {"code", "message"}}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub body: Option<Value>,
}

pub type EngineResult<T> = Result<T, EngineError>;

impl EngineError {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        EngineError {
            status,
            code: code.to_string(),
            body: Some(serde_json::json!({ "error": { "code": code, "message": message } })),
            message,
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, "bad_request", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(500, "internal", message)
    }

    /// The JSON a call throws.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"status":500,"code":"internal","message":"internal error"}"#.to_string()
        })
    }
}

impl From<PlanError> for EngineError {
    fn from(error: PlanError) -> Self {
        EngineError {
            status: error.status(),
            code: error.code().to_string(),
            message: error.message(),
            body: Some(error.body()),
        }
    }
}

impl From<SimulationError> for EngineError {
    /// A configuration the engine refuses is the caller's; anything else is a
    /// failed run (the server shows "Simulation failed" and keeps the reason
    /// in its log; here the reason is the message, since nothing leaves the
    /// device).
    fn from(error: SimulationError) -> Self {
        match error {
            SimulationError::Config(_) => EngineError::bad_request(error.to_string()),
            SimulationError::Cancelled => EngineError::new(409, "conflict", error.to_string()),
            other => EngineError::new(500, "simulation_failed", other.to_string()),
        }
    }
}

/// Read `text` as a `T`. A failure names a position, never the text, which is a
/// plan (the server's rule for request bodies too).
pub fn parse<T: serde::de::DeserializeOwned>(what: &str, text: &str) -> EngineResult<T> {
    serde_json::from_str(text).map_err(|error| {
        EngineError::bad_request(format!(
            "{what} is not the expected JSON (line {}, column {})",
            error.line(),
            error.column()
        ))
    })
}

/// Write `value` as JSON.
pub fn to_json<T: Serialize>(value: &T) -> EngineResult<String> {
    serde_json::to_string(value).map_err(|error| EngineError::internal(error.to_string()))
}
