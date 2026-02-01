/// Trait for screens that handle their own modal results.
///
/// This allows screens to handle modal results locally instead of going through
/// the central dispatch in app.rs, making the code more modular and easier to extend.
use crate::actions::ActionResult;
use crate::state::AppState;

use super::{ConfirmedValue, ModalAction};

/// Trait for handling modal results within a screen
pub trait ModalHandler {
    /// Check if this handler should handle the given action
    fn handles(&self, action: &ModalAction) -> bool;

    /// Handle the modal result, returning the action result
    ///
    /// Use `value.as_str()` for picker/text modals and `value.as_form()` for form modals.
    fn handle_modal_result(
        &self,
        state: &mut AppState,
        action: ModalAction,
        value: &ConfirmedValue,
    ) -> ActionResult;
}
