use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::state::MessageModal;

use super::{centered_rect, ModalResult};

const MODAL_WIDTH: u16 = 50;
const MODAL_MIN_HEIGHT: u16 = 7;

/// Render the message modal
pub fn render_message_modal(frame: &mut Frame, modal: &MessageModal) {
    let area = frame.area();

    // Calculate height based on message length
    let message_lines = modal.message.len() / (MODAL_WIDTH as usize - 4) + 1;
    let height = (MODAL_MIN_HEIGHT + message_lines as u16).min(area.height - 2);

    let modal_area = centered_rect(MODAL_WIDTH, height, area);

    // Clear the area behind the modal
    frame.render_widget(Clear, modal_area);

    // Choose border color based on error status
    let border_color = if modal.is_error {
        Color::Red
    } else {
        Color::Green
    };

    // Create the modal block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(format!(" {} ", modal.title));

    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    // Layout for modal content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Spacing
            Constraint::Min(1),    // Message
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Help text
        ])
        .split(inner);

    // Render message
    let message = Paragraph::new(modal.message.as_str())
        .wrap(Wrap { trim: true });
    frame.render_widget(message, chunks[1]);

    // Render help text
    let help = Paragraph::new(Line::from(vec![
        Span::styled("[Enter]", Style::default().fg(Color::Green)),
        Span::raw(" or "),
        Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
        Span::raw(" to dismiss"),
    ]));
    frame.render_widget(help, chunks[3]);
}

/// Handle key events for message modal
pub fn handle_message_key(key: KeyEvent) -> ModalResult {
    match key.code {
        KeyCode::Enter | KeyCode::Esc => ModalResult::Cancelled,
        _ => ModalResult::Continue,
    }
}
