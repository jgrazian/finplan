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

use crate::state::context::{ModalContext, TriggerBuilderState, TriggerContext};
use crate::state::{ModalContextValue, ModalState};

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
    /// The typed context from the modal (when using ModalContextValue::Typed)
    pub typed_modal_context: Option<&'a ModalContext>,
    /// The value submitted from the modal (pipe-delimited form fields)
    pub value: &'a str,
}

impl<'a> ActionContext<'a> {
    pub fn new(modal_context: Option<&'a ModalContextValue>, value: &'a str) -> Self {
        let (legacy_str, typed) = match modal_context {
            Some(ModalContextValue::Legacy(s)) => (Some(s.as_str()), None),
            Some(ModalContextValue::Typed(ctx)) => (None, Some(ctx)),
            None => (None, None),
        };
        Self {
            modal_context: legacy_str,
            typed_modal_context: typed,
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

    /// Get the typed modal context
    pub fn typed_context(&self) -> Option<&ModalContext> {
        self.typed_modal_context
    }

    /// Get the trigger builder state from a typed context
    pub fn trigger_builder(&self) -> Option<&TriggerBuilderState> {
        self.typed_context().and_then(|ctx| {
            if let ModalContext::Trigger(TriggerContext::RepeatingBuilder(state)) = ctx {
                Some(state)
            } else {
                None
            }
        })
    }

    /// Split the value by pipe delimiter
    pub fn value_parts(&self) -> Vec<&str> {
        self.value.split('|').collect()
    }
}
