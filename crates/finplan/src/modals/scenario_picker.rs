use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::state::{ModalAction, ScenarioPickerModal};

use super::{ModalResult, centered_rect};

const MODAL_WIDTH: u16 = 50;
const MODAL_MIN_HEIGHT: u16 = 10;
const MODAL_MAX_HEIGHT: u16 = 20;

/// Render the scenario picker modal
pub fn render_scenario_picker_modal(frame: &mut Frame, modal: &ScenarioPickerModal) {
    let area = frame.area();

    // Calculate height based on number of scenarios
    let content_height = modal.scenarios.len() as u16
        + if modal.action == ModalAction::SaveAs {
            3
        } else {
            1
        };
    let height = (MODAL_MIN_HEIGHT + content_height)
        .min(MODAL_MAX_HEIGHT)
        .min(area.height - 2);

    let modal_area = centered_rect(MODAL_WIDTH, height, area);

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
            Constraint::Min(3),    // Scenario list
            Constraint::Length(
                if modal.action == ModalAction::SaveAs && modal.is_new_scenario_selected() {
                    3
                } else {
                    1
                },
            ), // New name input (conditional)
            Constraint::Length(2), // Help text
        ])
        .split(inner);

    // Build list items
    let mut items: Vec<ListItem> = modal
        .scenarios
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let style = if i == modal.selected_index && !modal.editing_new_name {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(format!("  {}  ", name), style)))
        })
        .collect();

    // Add "New scenario" option for save
    if modal.action == ModalAction::SaveAs {
        let is_selected = modal.selected_index == modal.scenarios.len();
        let style = if is_selected && !modal.editing_new_name {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };
        items.push(ListItem::new(Line::from(Span::styled(
            "  + New scenario  ",
            style,
        ))));
    }

    let list = List::new(items);
    frame.render_widget(list, chunks[1]);

    // Show new name input when "New scenario" is selected
    if modal.action == ModalAction::SaveAs && modal.is_new_scenario_selected() {
        let input_style = if modal.editing_new_name {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let name_value = modal.new_name.as_deref().unwrap_or("");
        let cursor = if modal.editing_new_name { "_" } else { "" };

        let input_line = Line::from(vec![
            Span::raw("  Name: "),
            Span::styled(format!("{}{}", name_value, cursor), input_style),
        ]);

        let input = Paragraph::new(input_line);
        frame.render_widget(input, chunks[2]);
    }

    // Render help text
    let help_text = if modal.editing_new_name {
        vec![
            Span::styled("[Enter]", Style::default().fg(Color::Green)),
            Span::raw(" Confirm  "),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
            Span::raw(" Back"),
        ]
    } else {
        vec![
            Span::styled("[j/k]", Style::default().fg(Color::Cyan)),
            Span::raw(" Navigate  "),
            Span::styled("[Enter]", Style::default().fg(Color::Green)),
            Span::raw(" Select  "),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
            Span::raw(" Cancel"),
        ]
    };

    let help = Paragraph::new(Line::from(help_text));
    frame.render_widget(help, chunks[3]);
}

/// Handle key events for scenario picker modal
pub fn handle_scenario_picker_key(key: KeyEvent, modal: &mut ScenarioPickerModal) -> ModalResult {
    if modal.editing_new_name {
        // Text input mode for new scenario name
        match key.code {
            KeyCode::Enter => {
                if let Some(name) = &modal.new_name
                    && !name.is_empty()
                {
                    let name = name.clone();
                    return ModalResult::Confirmed(modal.action, name);
                }
                ModalResult::Continue
            }
            KeyCode::Esc => {
                modal.editing_new_name = false;
                ModalResult::Continue
            }
            KeyCode::Backspace => {
                if let Some(ref mut name) = modal.new_name {
                    name.pop();
                }
                ModalResult::Continue
            }
            KeyCode::Char(c) => {
                if let Some(ref mut name) = modal.new_name {
                    // Only allow valid identifier characters
                    if c.is_alphanumeric() || c == '_' || c == '-' || c == ' ' {
                        name.push(c);
                    }
                }
                ModalResult::Continue
            }
            _ => ModalResult::Continue,
        }
    } else {
        // Navigation mode
        match key.code {
            KeyCode::Enter => {
                if modal.is_new_scenario_selected() {
                    // Switch to text input mode for new name
                    modal.editing_new_name = true;
                    ModalResult::Continue
                } else if let Some(name) = modal.selected_name() {
                    ModalResult::Confirmed(modal.action, name)
                } else {
                    ModalResult::Continue
                }
            }
            KeyCode::Esc => ModalResult::Cancelled,
            KeyCode::Char('j') | KeyCode::Down => {
                modal.move_down();
                ModalResult::Continue
            }
            KeyCode::Char('k') | KeyCode::Up => {
                modal.move_up();
                ModalResult::Continue
            }
            _ => ModalResult::Continue,
        }
    }
}
