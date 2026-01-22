mod confirm;
mod form;
pub mod helpers;
mod message;
mod picker;
mod scenario_picker;
mod text_input;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::state::{AppState, FormModal, ModalAction, ModalState};

pub use confirm::render_confirm_modal;
pub use form::{
    format_currency_for_edit, format_percentage_for_edit, parse_currency, parse_percentage,
    render_form_modal,
};
pub use message::render_message_modal;
pub use picker::render_picker_modal;
pub use scenario_picker::render_scenario_picker_modal;
pub use text_input::render_text_input_modal;

/// Typed value returned when a modal is confirmed
#[derive(Debug, Clone)]
pub enum ConfirmedValue {
    /// Form modal with typed field access
    Form(FormModal),
    /// Picker modal - selected option string
    Picker(String),
    /// Text input modal - entered text
    Text(String),
    /// Confirm modal - just confirmed, no value
    Confirm,
}

impl ConfirmedValue {
    /// Get the form if this is a Form variant
    pub fn as_form(&self) -> Option<&FormModal> {
        match self {
            ConfirmedValue::Form(form) => Some(form),
            _ => None,
        }
    }

    /// Get the selected string for Picker or Text variants
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ConfirmedValue::Picker(s) | ConfirmedValue::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Get string representation (for backwards compatibility during migration)
    pub fn to_legacy_string(&self) -> String {
        match self {
            ConfirmedValue::Form(form) => form
                .fields
                .iter()
                .map(|f| f.value.clone())
                .collect::<Vec<_>>()
                .join("|"),
            ConfirmedValue::Picker(s) | ConfirmedValue::Text(s) => s.clone(),
            ConfirmedValue::Confirm => String::new(),
        }
    }
}

/// Result of handling a modal key event
#[derive(Debug)]
pub enum ModalResult {
    /// Modal confirmed with action and typed value
    Confirmed(ModalAction, Box<ConfirmedValue>),
    /// Modal was cancelled
    Cancelled,
    /// Key was handled, modal still active
    Continue,
}

/// Render the active modal as an overlay
pub fn render_modal(frame: &mut Frame, state: &AppState) {
    match &state.modal {
        ModalState::None => {}
        ModalState::TextInput(modal) => {
            render_text_input_modal(frame, modal);
        }
        ModalState::Message(modal) => {
            render_message_modal(frame, modal);
        }
        ModalState::ScenarioPicker(modal) => {
            render_scenario_picker_modal(frame, modal);
        }
        ModalState::Picker(modal) => {
            render_picker_modal(frame, modal);
        }
        ModalState::Form(modal) => {
            render_form_modal(frame, modal);
        }
        ModalState::Confirm(modal) => {
            render_confirm_modal(frame, modal);
        }
    }
}

/// Handle key events for the active modal
pub fn handle_modal_key(key: KeyEvent, state: &mut AppState) -> ModalResult {
    match &mut state.modal {
        ModalState::None => ModalResult::Continue,
        ModalState::TextInput(modal) => text_input::handle_text_input_key(key, modal),
        ModalState::Message(_) => message::handle_message_key(key),
        ModalState::ScenarioPicker(modal) => {
            scenario_picker::handle_scenario_picker_key(key, modal)
        }
        ModalState::Picker(modal) => picker::handle_picker_key(key, modal),
        ModalState::Form(modal) => form::handle_form_key(key, modal),
        ModalState::Confirm(modal) => confirm::handle_confirm_key(key, modal),
    }
}

/// Create a centered rectangle within the given area
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
