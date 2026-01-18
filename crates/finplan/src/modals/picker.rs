use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::state::PickerModal;

use super::{centered_rect, ModalResult};

/// Render the picker modal
pub fn render_picker_modal(frame: &mut Frame, modal: &PickerModal) {
    let area = frame.area();

    // Calculate height based on number of options (min 6, max 15)
    let content_height = (modal.options.len() as u16).clamp(3, 12);
    let modal_height = content_height + 7; // title + borders + help text + padding
    let modal_width = 60; // Wider for better readability

    let modal_area = centered_rect(modal_width, modal_height, area);

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
            Constraint::Min(1),    // Options list
            Constraint::Length(1), // Spacing
            Constraint::Length(2), // Help text (2 lines)
        ])
        .split(inner);

    // Render options list
    let items: Vec<ListItem> = modal
        .options
        .iter()
        .enumerate()
        .map(|(idx, option)| {
            let style = if idx == modal.selected_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let prefix = if idx == modal.selected_index { "> " } else { "  " };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, option),
                style,
            )))
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, chunks[1]);

    // Render help text (2 lines for clarity)
    let help = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("[j/k/↑/↓]", Style::default().fg(Color::DarkGray)),
            Span::raw(" Navigate"),
        ]),
        Line::from(vec![
            Span::styled("[Enter]", Style::default().fg(Color::Green)),
            Span::raw(" Select  "),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
            Span::raw(" Cancel"),
        ]),
    ]);
    frame.render_widget(help, chunks[3]);
}

/// Handle key events for picker modal
pub fn handle_picker_key(key: KeyEvent, modal: &mut PickerModal) -> ModalResult {
    match key.code {
        KeyCode::Enter => {
            if let Some(selected) = modal.options.get(modal.selected_index) {
                ModalResult::Confirmed(modal.action, selected.clone())
            } else {
                ModalResult::Cancelled
            }
        }
        KeyCode::Esc => ModalResult::Cancelled,
        KeyCode::Char('j') | KeyCode::Down => {
            if !modal.options.is_empty() {
                modal.selected_index = (modal.selected_index + 1) % modal.options.len();
            }
            ModalResult::Continue
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if !modal.options.is_empty() {
                if modal.selected_index == 0 {
                    modal.selected_index = modal.options.len() - 1;
                } else {
                    modal.selected_index -= 1;
                }
            }
            ModalResult::Continue
        }
        KeyCode::Home => {
            modal.selected_index = 0;
            ModalResult::Continue
        }
        KeyCode::End => {
            if !modal.options.is_empty() {
                modal.selected_index = modal.options.len() - 1;
            }
            ModalResult::Continue
        }
        _ => ModalResult::Continue,
    }
}
