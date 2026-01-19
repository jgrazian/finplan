use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Constraint,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::state::ConfirmModal;

use super::ModalResult;
use super::helpers::{HelpText, render_modal_frame};

const MODAL_WIDTH: u16 = 60;
const MODAL_HEIGHT: u16 = 10;

/// Render the confirm modal
pub fn render_confirm_modal(frame: &mut Frame, modal: &ConfirmModal) {
    // Render the modal frame with warning color
    let mf = render_modal_frame(
        frame,
        &modal.title,
        MODAL_WIDTH,
        MODAL_HEIGHT,
        Color::Red,
        &[
            Constraint::Length(1), // Spacing
            Constraint::Min(2),    // Message
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Help text
        ],
    );

    // Render message
    let message = Paragraph::new(Line::from(vec![Span::styled(
        &modal.message,
        Style::default().add_modifier(Modifier::BOLD),
    )]))
    .wrap(Wrap { trim: true });
    frame.render_widget(message, mf.chunks[1]);

    // Render help text
    let help = HelpText::new()
        .key("[y]", Color::Red, "Confirm")
        .key("[n/Esc]", Color::Green, "Cancel")
        .build();
    frame.render_widget(help, mf.chunks[3]);
}

/// Handle key events for confirm modal
pub fn handle_confirm_key(key: KeyEvent, modal: &ConfirmModal) -> ModalResult {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            ModalResult::Confirmed(modal.action, String::new())
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => ModalResult::Cancelled,
        _ => ModalResult::Continue,
    }
}
