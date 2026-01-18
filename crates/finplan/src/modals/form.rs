use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::state::{FieldType, FormField, FormModal};

use super::helpers::{calculate_scroll, render_cursor_line, HelpText, MultiLineHelp};
use super::{centered_rect, ModalResult};

/// Render the form modal
pub fn render_form_modal(frame: &mut Frame, modal: &FormModal) {
    let area = frame.area();

    // Calculate height based on number of fields
    // Each field: 1 line label + 3 lines input box (1 content + 2 border) = 4 lines
    let field_height = modal.fields.len() as u16 * 4;
    let modal_height = (field_height + 10).min(35); // title + spacing + help + extra padding
    let modal_width = 70; // Wider to fit help text

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
    let mut constraints = vec![Constraint::Length(1)]; // Top spacing
    for _ in &modal.fields {
        constraints.push(Constraint::Length(4)); // Each field: 1 label + 3 input box
    }
    constraints.push(Constraint::Min(1)); // Spacing
    constraints.push(Constraint::Length(2)); // Help text (2 lines)

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // Render each field
    for (idx, field) in modal.fields.iter().enumerate() {
        let is_focused = idx == modal.focused_field;
        let chunk_idx = idx + 1;

        render_field(frame, chunks[chunk_idx], field, is_focused, modal.editing);
    }

    // Render help text at the bottom
    let help_idx = modal.fields.len() + 2;
    let help = if modal.editing {
        MultiLineHelp::new()
            .line(
                HelpText::new()
                    .key("EDITING:", Color::Cyan, "Type to enter text")
                    .key("[F10/Ctrl+S]", Color::Cyan, "Submit"),
            )
            .line(
                HelpText::new()
                    .key("[Enter]", Color::Green, "Done field")
                    .key("[Esc]", Color::Yellow, "Cancel"),
            )
            .build()
    } else {
        MultiLineHelp::new()
            .line(
                HelpText::new()
                    .key("[j/k/Tab]", Color::DarkGray, "Navigate")
                    .key("[Enter]", Color::Green, "Edit field"),
            )
            .line(
                HelpText::new()
                    .key("[F10/Ctrl+S]", Color::Cyan, "Submit")
                    .key("[Esc]", Color::Yellow, "Cancel"),
            )
            .build()
    };
    frame.render_widget(help, chunks[help_idx]);
}

fn render_field(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    field: &FormField,
    is_focused: bool,
    is_editing: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(3)]) // 3 = 1 for content + 2 for borders
        .split(area);

    // Render label
    let label_style = if is_focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };

    let label = Paragraph::new(Line::from(Span::styled(&field.label, label_style)));
    frame.render_widget(label, chunks[0]);

    // Render input field
    let (border_color, fg_color) = match field.field_type {
        FieldType::ReadOnly => (Color::DarkGray, Color::DarkGray),
        _ if is_focused && is_editing => (Color::Cyan, Color::White),
        _ if is_focused => (Color::Yellow, Color::White),
        _ => (Color::DarkGray, Color::White),
    };

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let input_inner = input_block.inner(chunks[1]);
    frame.render_widget(input_block, chunks[1]);

    // Render value with cursor if editing
    if is_focused && is_editing && field.field_type != FieldType::ReadOnly {
        render_editing_value(frame, input_inner, field);
    } else {
        let display_value = format_display_value(field);
        let value = Paragraph::new(Line::from(Span::styled(
            display_value,
            Style::default().fg(fg_color),
        )));
        frame.render_widget(value, input_inner);
    }
}

fn render_editing_value(frame: &mut Frame, area: ratatui::layout::Rect, field: &FormField) {
    let input_width = (area.width as usize).saturating_sub(1);
    let scrolled = calculate_scroll(&field.value, field.cursor_pos, input_width + 2);

    // Build the input line with cursor
    let input_line = render_cursor_line(&scrolled.display_value, scrolled.cursor_pos, "");
    frame.render_widget(Paragraph::new(input_line), area);
}

fn format_display_value(field: &FormField) -> String {
    match field.field_type {
        FieldType::Currency => {
            if let Ok(val) = field.value.parse::<f64>() {
                format!("${:.2}", val)
            } else if field.value.is_empty() {
                "$0.00".to_string()
            } else {
                field.value.clone()
            }
        }
        FieldType::Percentage => {
            if let Ok(val) = field.value.parse::<f64>() {
                format!("{}%", val)
            } else if field.value.is_empty() {
                "0%".to_string()
            } else {
                format!("{}%", field.value)
            }
        }
        _ => field.value.clone(),
    }
}

/// Handle key events for form modal
pub fn handle_form_key(key: KeyEvent, modal: &mut FormModal) -> ModalResult {
    if modal.editing {
        handle_editing_key(key, modal)
    } else {
        handle_navigation_key(key, modal)
    }
}

fn handle_editing_key(key: KeyEvent, modal: &mut FormModal) -> ModalResult {
    // Check for submit keys first - works in both editing and navigation mode
    // Support multiple key combos since Ctrl+Enter is unreliable in some terminals
    let is_submit = matches!(
        (key.code, key.modifiers.contains(KeyModifiers::CONTROL)),
        (KeyCode::Enter, true) | (KeyCode::Char('s'), true)
    ) || key.code == KeyCode::F(10);

    if is_submit {
        return ModalResult::Confirmed(modal.action, serialize_form(modal));
    }

    let field = &mut modal.fields[modal.focused_field];

    match key.code {
        KeyCode::Enter => {
            // Exit edit mode (plain Enter without Ctrl)
            modal.editing = false;
            ModalResult::Continue
        }
        KeyCode::Esc => {
            // Cancel edit and revert to original (for now, just exit edit mode)
            modal.editing = false;
            ModalResult::Continue
        }
        KeyCode::Backspace => {
            if field.cursor_pos > 0 {
                field.cursor_pos -= 1;
                field.value.remove(field.cursor_pos);
            }
            ModalResult::Continue
        }
        KeyCode::Delete => {
            if field.cursor_pos < field.value.len() {
                field.value.remove(field.cursor_pos);
            }
            ModalResult::Continue
        }
        KeyCode::Left => {
            if field.cursor_pos > 0 {
                field.cursor_pos -= 1;
            }
            ModalResult::Continue
        }
        KeyCode::Right => {
            if field.cursor_pos < field.value.len() {
                field.cursor_pos += 1;
            }
            ModalResult::Continue
        }
        KeyCode::Home => {
            field.cursor_pos = 0;
            ModalResult::Continue
        }
        KeyCode::End => {
            field.cursor_pos = field.value.len();
            ModalResult::Continue
        }
        KeyCode::Char(c) => {
            // Validate character based on field type
            let valid = match field.field_type {
                FieldType::Currency | FieldType::Percentage => {
                    c.is_ascii_digit() || c == '.' || c == '-'
                }
                FieldType::Text => true,
                FieldType::ReadOnly => false,
            };

            if valid {
                field.value.insert(field.cursor_pos, c);
                field.cursor_pos += 1;
            }
            ModalResult::Continue
        }
        _ => ModalResult::Continue,
    }
}

fn handle_navigation_key(key: KeyEvent, modal: &mut FormModal) -> ModalResult {
    // Check for submit keys first
    let is_submit = matches!(
        (key.code, key.modifiers.contains(KeyModifiers::CONTROL)),
        (KeyCode::Enter, true) | (KeyCode::Char('s'), true)
    ) || key.code == KeyCode::F(10);

    if is_submit {
        return ModalResult::Confirmed(modal.action, serialize_form(modal));
    }

    match key.code {
        KeyCode::Enter | KeyCode::Char('e') => {
            // Enter edit mode for current field if not read-only
            if modal.fields[modal.focused_field].field_type != FieldType::ReadOnly {
                modal.editing = true;
                // Move cursor to end of value
                let field = &mut modal.fields[modal.focused_field];
                field.cursor_pos = field.value.len();
            }
            ModalResult::Continue
        }
        KeyCode::Esc => ModalResult::Cancelled,
        KeyCode::Tab | KeyCode::Char('j') | KeyCode::Down => {
            // Skip read-only fields when navigating
            let start = modal.focused_field;
            loop {
                modal.focused_field = (modal.focused_field + 1) % modal.fields.len();
                if modal.fields[modal.focused_field].field_type != FieldType::ReadOnly
                    || modal.focused_field == start
                {
                    break;
                }
            }
            ModalResult::Continue
        }
        KeyCode::BackTab | KeyCode::Char('k') | KeyCode::Up => {
            // Skip read-only fields when navigating
            let start = modal.focused_field;
            loop {
                if modal.focused_field == 0 {
                    modal.focused_field = modal.fields.len() - 1;
                } else {
                    modal.focused_field -= 1;
                }
                if modal.fields[modal.focused_field].field_type != FieldType::ReadOnly
                    || modal.focused_field == start
                {
                    break;
                }
            }
            ModalResult::Continue
        }
        _ => ModalResult::Continue,
    }
}

/// Serialize form fields to a string representation
/// Format: field1_value|field2_value|field3_value
fn serialize_form(modal: &FormModal) -> String {
    modal
        .fields
        .iter()
        .map(|f| f.value.clone())
        .collect::<Vec<_>>()
        .join("|")
}

// ========== Validation Helpers ==========

/// Parse a percentage string (e.g., "5.0") to a decimal (e.g., 0.05)
pub fn parse_percentage(s: &str) -> Result<f64, &'static str> {
    let s = s.trim().trim_end_matches('%');
    s.parse::<f64>()
        .map(|v| v / 100.0)
        .map_err(|_| "Invalid percentage")
}

/// Parse a currency string (e.g., "$1,234.56") to a float
pub fn parse_currency(s: &str) -> Result<f64, &'static str> {
    let s = s.trim().trim_start_matches('$').replace(',', "");
    s.parse::<f64>().map_err(|_| "Invalid currency amount")
}

/// Format a decimal as a percentage string for display
pub fn format_percentage_for_edit(rate: f64) -> String {
    format!("{:.2}", rate * 100.0)
}

/// Format a currency value for editing (no $ sign)
pub fn format_currency_for_edit(value: f64) -> String {
    format!("{:.2}", value)
}
