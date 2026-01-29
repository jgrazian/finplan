use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Constraint,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};

use crate::modals::{ModalAction, ScenarioPickerModal};

use super::helpers::{HelpText, render_modal_frame};
use super::{ConfirmedValue, ModalResult};

const MODAL_WIDTH: u16 = 50;
const MODAL_MIN_HEIGHT: u16 = 10;
const MODAL_MAX_HEIGHT: u16 = 20;

/// Render the scenario picker modal
pub fn render_scenario_picker_modal(frame: &mut Frame, modal: &ScenarioPickerModal) {
    let area = frame.area();

    // Calculate height based on number of scenarios
    let content_height = modal.scenarios.len() as u16
        + if modal.action == ModalAction::SAVE_AS {
            3
        } else {
            1
        };
    let height = (MODAL_MIN_HEIGHT + content_height)
        .min(MODAL_MAX_HEIGHT)
        .min(area.height - 2);

    // Calculate new name input height
    let new_name_height =
        if modal.action == ModalAction::SAVE_AS && modal.is_new_scenario_selected() {
            3
        } else {
            1
        };

    // Render the modal frame
    let mf = render_modal_frame(
        frame,
        &modal.title,
        MODAL_WIDTH,
        height,
        Color::Cyan,
        &[
            Constraint::Length(1),               // Spacing
            Constraint::Min(3),                  // Scenario list
            Constraint::Length(new_name_height), // New name input (conditional)
            Constraint::Length(2),               // Help text
        ],
    );

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
    if modal.action == ModalAction::SAVE_AS {
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
    frame.render_widget(list, mf.chunks[1]);

    // Show new name input when "New scenario" is selected
    if modal.action == ModalAction::SAVE_AS && modal.is_new_scenario_selected() {
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
        frame.render_widget(input, mf.chunks[2]);
    }

    // Render help text
    let help = if modal.editing_new_name {
        HelpText::new()
            .key("[Enter]", Color::Green, "Confirm")
            .key("[Esc]", Color::Yellow, "Back")
            .build()
    } else {
        HelpText::new()
            .key("[j/k]", Color::Cyan, "Navigate")
            .key("[Enter]", Color::Green, "Select")
            .key("[Esc]", Color::Yellow, "Cancel")
            .build()
    };
    frame.render_widget(help, mf.chunks[3]);
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
                    return ModalResult::Confirmed(
                        modal.action,
                        Box::new(ConfirmedValue::Text(name)),
                    );
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
                    ModalResult::Confirmed(modal.action, Box::new(ConfirmedValue::Text(name)))
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
