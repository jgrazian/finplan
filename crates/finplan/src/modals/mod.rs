mod message;
mod scenario_picker;
mod text_input;

use crossterm::event::KeyEvent;
use ratatui::{
    layout::Rect,
    Frame,
};

use crate::state::{AppState, ModalAction, ModalState};

pub use message::render_message_modal;
pub use scenario_picker::render_scenario_picker_modal;
pub use text_input::render_text_input_modal;

/// Result of handling a modal key event
#[derive(Debug, PartialEq, Eq)]
pub enum ModalResult {
    /// Modal confirmed with action and value
    Confirmed(ModalAction, String),
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
    }
}

/// Handle key events for the active modal
pub fn handle_modal_key(key: KeyEvent, state: &mut AppState) -> ModalResult {
    match &mut state.modal {
        ModalState::None => ModalResult::Continue,
        ModalState::TextInput(modal) => text_input::handle_text_input_key(key, modal),
        ModalState::Message(_) => message::handle_message_key(key),
        ModalState::ScenarioPicker(modal) => scenario_picker::handle_scenario_picker_key(key, modal),
    }
}

/// Create a centered rectangle within the given area
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
