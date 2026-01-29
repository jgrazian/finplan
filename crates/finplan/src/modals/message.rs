use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Constraint,
    style::Color,
    widgets::{Paragraph, Wrap},
};

use crate::modals::MessageModal;

use super::ModalResult;
use super::helpers::{HelpText, render_modal_frame};

const MODAL_WIDTH: u16 = 50;
const MODAL_MIN_HEIGHT: u16 = 7;

/// Render the message modal
pub fn render_message_modal(frame: &mut Frame, modal: &MessageModal) {
    // Calculate height based on message length
    let message_lines = modal.message.len() / (MODAL_WIDTH as usize - 4) + 1;
    let height = (MODAL_MIN_HEIGHT + message_lines as u16).min(frame.area().height - 2);

    // Choose border color based on error status
    let border_color = if modal.is_error {
        Color::Red
    } else {
        Color::Green
    };

    // Render the modal frame
    let mf = render_modal_frame(
        frame,
        &modal.title,
        MODAL_WIDTH,
        height,
        border_color,
        &[
            Constraint::Length(1), // Spacing
            Constraint::Min(1),    // Message
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Help text
        ],
    );

    // Render message
    let message = Paragraph::new(modal.message.as_str()).wrap(Wrap { trim: true });
    frame.render_widget(message, mf.chunks[1]);

    // Render help text
    let help = HelpText::new()
        .key("[Enter]", Color::Green, "or")
        .key("[Esc]", Color::Yellow, "to dismiss")
        .build();
    frame.render_widget(help, mf.chunks[3]);
}

/// Handle key events for message modal
pub fn handle_message_key(key: KeyEvent) -> ModalResult {
    match key.code {
        KeyCode::Enter | KeyCode::Esc => ModalResult::Cancelled,
        _ => ModalResult::Continue,
    }
}
