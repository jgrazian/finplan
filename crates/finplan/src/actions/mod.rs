// Actions module - domain-specific handler implementations
//
// This module organizes business logic for handling modal results into
// domain-specific files, reducing the size of app.rs.

mod account;
mod config;
mod effect;
mod event;
mod holding;
mod profile;
mod scenario;
pub mod wizard;

pub use account::*;
pub use config::*;
pub use effect::*;
pub use event::*;
pub use holding::*;
pub use profile::*;
pub use scenario::*;

use crate::state::ModalState;

/// Result of an action handler
///
/// Actions can either complete (returning a new modal state or None to close),
/// or require additional state changes that must be handled by the caller.
pub enum ActionResult {
    /// Action completed, set modal to this state (None closes the modal)
    Done(Option<ModalState>),
    /// Action requires marking the state as modified
    Modified(Option<ModalState>),
    /// Action failed with an error message
    Error(String),
}

impl ActionResult {
    /// Create a result that closes the modal
    pub fn close() -> Self {
        ActionResult::Done(None)
    }

    /// Create a result that shows a new modal
    pub fn modal(state: ModalState) -> Self {
        ActionResult::Done(Some(state))
    }

    /// Create a result that closes the modal and marks state as modified
    pub fn modified() -> Self {
        ActionResult::Modified(None)
    }

    /// Create a result that shows a new modal and marks state as modified
    pub fn modified_with_modal(state: ModalState) -> Self {
        ActionResult::Modified(Some(state))
    }

    /// Create an error result
    pub fn error(msg: impl Into<String>) -> Self {
        ActionResult::Error(msg.into())
    }
}

/// Context passed to action handlers
pub struct ActionContext<'a> {
    /// The string context from the modal (often contains indices or type info)
    pub modal_context: Option<&'a str>,
    /// The value submitted from the modal (pipe-delimited form fields)
    pub value: &'a str,
}

impl<'a> ActionContext<'a> {
    pub fn new(modal_context: Option<&'a String>, value: &'a str) -> Self {
        Self {
            modal_context: modal_context.as_ref().map(|s| s.as_str()),
            value,
        }
    }

    /// Parse the context as an index
    pub fn index(&self) -> Option<usize> {
        self.modal_context?.parse().ok()
    }

    /// Parse the context as colon-separated indices
    pub fn indices(&self) -> Vec<usize> {
        self.modal_context
            .map(|s| s.split(':').filter_map(|p| p.parse().ok()).collect())
            .unwrap_or_default()
    }

    /// Get the context string or empty string
    pub fn context_str(&self) -> &str {
        self.modal_context.unwrap_or("")
    }

    /// Split the value by pipe delimiter
    pub fn value_parts(&self) -> Vec<&str> {
        self.value.split('|').collect()
    }
}
