use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Constraint,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::state::TextInputModal;

use super::ModalResult;
use super::helpers::{HelpText, calculate_scroll, render_cursor_line, render_modal_frame};

const MODAL_WIDTH: u16 = 60;
const MODAL_HEIGHT: u16 = 9;

/// Render the text input modal
pub fn render_text_input_modal(frame: &mut Frame, modal: &TextInputModal) {
    // Render the modal frame
    let mf: super::helpers::ModalFrame = render_modal_frame(
        frame,
        &modal.title,
        MODAL_WIDTH,
        MODAL_HEIGHT,
        Color::Cyan,
        &[
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Prompt
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Input field
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Help text
        ],
    );

    // Render prompt
    let prompt = Paragraph::new(Line::from(Span::styled(
        &modal.prompt,
        Style::default().add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(prompt, mf.chunks[1]);

    // Calculate scroll for long inputs
    let input_width = (mf.chunks[3].width as usize).saturating_sub(2);
    let scrolled = calculate_scroll(&modal.value, modal.cursor_pos, input_width + 2);

    // Build the input line with cursor
    let input_line = render_cursor_line(&scrolled.display_value, scrolled.cursor_pos, " ");

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let input_inner = input_block.inner(mf.chunks[3]);
    frame.render_widget(input_block, mf.chunks[3]);
    frame.render_widget(Paragraph::new(input_line), input_inner);

    // Render help text
    let help = HelpText::new()
        .key("[Enter]", Color::Green, "Confirm")
        .key("[Esc]", Color::Yellow, "Cancel")
        .build();
    frame.render_widget(help, mf.chunks[5]);
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
