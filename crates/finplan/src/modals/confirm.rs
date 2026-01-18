use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::state::ConfirmModal;

use super::{centered_rect, ModalResult};

const MODAL_WIDTH: u16 = 60;
const MODAL_HEIGHT: u16 = 10;

/// Render the confirm modal
pub fn render_confirm_modal(frame: &mut Frame, modal: &ConfirmModal) {
    let area = frame.area();
    let modal_area = centered_rect(MODAL_WIDTH, MODAL_HEIGHT, area);

    // Clear the area behind the modal
    frame.render_widget(Clear, modal_area);

    // Create the modal block with warning color
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(format!(" {} ", modal.title));

    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    // Layout for modal content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Spacing
            Constraint::Min(2),    // Message
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Help text
        ])
        .split(inner);

    // Render message
    let message = Paragraph::new(Line::from(vec![
        Span::styled(
            &modal.message,
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]))
    .wrap(Wrap { trim: true });
    frame.render_widget(message, chunks[1]);

    // Render help text
    let help = Paragraph::new(Line::from(vec![
        Span::styled("[y]", Style::default().fg(Color::Red)),
        Span::raw(" Confirm  "),
        Span::styled("[n/Esc]", Style::default().fg(Color::Green)),
        Span::raw(" Cancel"),
    ]));
    frame.render_widget(help, chunks[3]);
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
