// Actions module - domain-specific handler implementations
//
// This module organizes business logic for handling modal results into
// domain-specific files, reducing the size of app.rs.

mod account;
mod config;
mod effect;
mod event;
mod holding;
pub mod optimize;
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

use crate::modals::ConfirmedValue;
use crate::state::context::{ModalContext, TriggerBuilderState, TriggerContext};
use crate::state::{FormModal, ModalState};

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
    /// The typed context from the modal (when using ModalContextValue::Typed)
    pub typed_modal_context: Option<&'a ModalContext>,
    /// The typed value submitted from the modal
    confirmed_value: &'a ConfirmedValue,
    /// Legacy string value for backwards compatibility
    legacy_value: String,
}

impl<'a> ActionContext<'a> {
    pub fn new(
        modal_context: Option<&'a ModalContext>,
        confirmed_value: &'a ConfirmedValue,
    ) -> Self {
        let legacy_value = confirmed_value.to_legacy_string();
        Self {
            typed_modal_context: modal_context,
            confirmed_value,
            legacy_value,
        }
    }

    /// Get the form if this was a form modal
    pub fn form(&self) -> Option<&FormModal> {
        self.confirmed_value.as_form()
    }

    /// Get the selected/entered string value (for pickers and text inputs)
    pub fn selected(&self) -> Option<&str> {
        self.confirmed_value.as_str()
    }

    /// Get the confirmed value
    pub fn confirmed_value(&self) -> &ConfirmedValue {
        self.confirmed_value
    }

    /// Get the legacy string value (for backwards compatibility during migration)
    /// This will be removed once all handlers are migrated to typed extraction
    pub fn value(&self) -> &str {
        &self.legacy_value
    }

    /// Split the legacy value by pipe delimiter (for backwards compatibility)
    /// This will be removed once all handlers are migrated to typed extraction
    pub fn value_parts(&self) -> Vec<&str> {
        self.legacy_value.split('|').collect()
    }

    /// Parse the context as an index (from typed or legacy context)
    /// Tries event, account, and profile indices in that order
    pub fn index(&self) -> Option<usize> {
        // Try typed context first
        if let Some(typed) = self.typed_modal_context {
            // Try extracting event index
            if let Some(idx) = typed.as_event_index() {
                return Some(idx);
            }
            // Try account index
            if let Some(idx) = typed.as_account_index() {
                return Some(idx);
            }
            // Try profile index
            if let Some(idx) = typed.as_profile_index() {
                return Some(idx);
            }
        }
        None
    }

    /// Parse the context as holding indices (account_idx, holding_idx)
    pub fn holding_indices(&self) -> Option<(usize, usize)> {
        // Try typed context first
        if let Some(typed) = self.typed_modal_context
            && let Some(indices) = typed.as_holding_index()
        {
            return Some(indices);
        }
        None
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
}
