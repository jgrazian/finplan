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
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::Invalid(message)
            | PlanError::Unprocessable(message)
            | PlanError::Conflict(message) => f.write_str(message),
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
}

pub type PlanResult<T> = Result<T, PlanError>;
