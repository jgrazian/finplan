use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::state::TextInputModal;

use super::{ModalResult, centered_rect};

const MODAL_WIDTH: u16 = 60;
const MODAL_HEIGHT: u16 = 9;

/// Render the text input modal
pub fn render_text_input_modal(frame: &mut Frame, modal: &TextInputModal) {
    let area = frame.area();
    let modal_area = centered_rect(MODAL_WIDTH, MODAL_HEIGHT, area);

    // Clear the area behind the modal
    frame.render_widget(Clear, modal_area);

    // Create the modal block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(" {} ", modal.title));

    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    // Layout for modal content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Prompt
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Input field
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Help text
        ])
        .split(inner);

    // Render prompt
    let prompt = Paragraph::new(Line::from(Span::styled(
        &modal.prompt,
        Style::default().add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(prompt, chunks[1]);

    // Render input field with cursor
    let input_width = (chunks[3].width as usize).saturating_sub(2);
    let display_value = if modal.value.len() > input_width {
        // Scroll to show cursor
        let start = modal.cursor_pos.saturating_sub(input_width / 2);
        let end = (start + input_width).min(modal.value.len());
        let start = end.saturating_sub(input_width);
        &modal.value[start..end]
    } else {
        &modal.value
    };

    // Calculate cursor position in display
    let cursor_display_pos = if modal.value.len() > input_width {
        let start = modal.cursor_pos.saturating_sub(input_width / 2);
        let end = (start + input_width).min(modal.value.len());
        let start = end.saturating_sub(input_width);
        modal.cursor_pos - start
    } else {
        modal.cursor_pos
    };

    // Build the input line with cursor
    let mut spans = Vec::new();
    spans.push(Span::raw(" "));

    let chars: Vec<char> = display_value.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i == cursor_display_pos {
            spans.push(Span::styled(
                c.to_string(),
                Style::default().bg(Color::White).fg(Color::Black),
            ));
        } else {
            spans.push(Span::raw(c.to_string()));
        }
    }

    // If cursor is at the end, show a cursor block
    if cursor_display_pos >= chars.len() {
        spans.push(Span::styled(
            " ",
            Style::default().bg(Color::White).fg(Color::Black),
        ));
    }

    let input_line = Line::from(spans);
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let input_inner = input_block.inner(chunks[3]);
    frame.render_widget(input_block, chunks[3]);
    frame.render_widget(Paragraph::new(input_line), input_inner);

    // Render help text
    let help = Paragraph::new(Line::from(vec![
        Span::styled("[Enter]", Style::default().fg(Color::Green)),
        Span::raw(" Confirm  "),
        Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
        Span::raw(" Cancel"),
    ]));
    frame.render_widget(help, chunks[5]);
}

/// Handle key events for text input modal
pub fn handle_text_input_key(key: KeyEvent, modal: &mut TextInputModal) -> ModalResult {
    match key.code {
        KeyCode::Enter => {
            let value = modal.value.clone();
            let action = modal.action;
            ModalResult::Confirmed(action, value)
        }
        KeyCode::Esc => ModalResult::Cancelled,
        KeyCode::Backspace => {
            modal.backspace();
            ModalResult::Continue
        }
        KeyCode::Delete => {
            modal.delete();
            ModalResult::Continue
        }
        KeyCode::Left => {
            modal.move_cursor_left();
            ModalResult::Continue
        }
        KeyCode::Right => {
            modal.move_cursor_right();
            ModalResult::Continue
        }
        KeyCode::Home => {
            modal.move_cursor_home();
            ModalResult::Continue
        }
        KeyCode::End => {
            modal.move_cursor_end();
            ModalResult::Continue
        }
        KeyCode::Char(c) => {
            modal.insert_char(c);
            ModalResult::Continue
        }
        _ => ModalResult::Continue,
    }
}
