use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::modals::{FieldType, FormField, FormModal};

use super::helpers::{HelpText, MultiLineHelp, calculate_scroll, render_cursor_line};
use super::{ConfirmedValue, ModalResult, centered_rect};

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

    // Render label with inline hints for discoverability
    let label_style = if is_focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };

    // Build label spans with contextual hints
    let mut label_spans = vec![Span::styled(&field.label, label_style)];

    // Add inline hints based on field type and state
    match &field.field_type {
        FieldType::ReadOnly => {
            label_spans.push(Span::styled(
                " (read-only)",
                Style::default().fg(Color::DarkGray),
            ));
        }
        FieldType::Select if is_focused => {
            label_spans.push(Span::styled(" [</>]", Style::default().fg(Color::Cyan)));
        }
        FieldType::Amount(_) if is_focused => {
            label_spans.push(Span::styled(
                " [Enter to edit]",
                Style::default().fg(Color::Cyan),
            ));
        }
        _ if is_focused && is_editing => {
            label_spans.push(Span::styled(
                " [EDITING]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        _ if is_focused => {
            label_spans.push(Span::styled(
                " [Enter to edit]",
                Style::default().fg(Color::Green),
            ));
        }
        _ => {}
    }

    let label = Paragraph::new(Line::from(label_spans));
    frame.render_widget(label, chunks[0]);

    // Render input field with enhanced styling for edit mode
    let (border_color, border_modifier, fg_color) = match &field.field_type {
        FieldType::ReadOnly => (Color::DarkGray, Modifier::empty(), Color::DarkGray),
        FieldType::Select if is_focused => (Color::Yellow, Modifier::empty(), Color::Cyan),
        FieldType::Amount(_) if is_focused => (Color::Yellow, Modifier::empty(), Color::Cyan),
        _ if is_focused && is_editing => (Color::Cyan, Modifier::BOLD, Color::White), // Bold border when editing
        _ if is_focused => (Color::Yellow, Modifier::empty(), Color::White),
        _ => (Color::DarkGray, Modifier::empty(), Color::White),
    };

    let input_block = Block::default().borders(Borders::ALL).border_style(
        Style::default()
            .fg(border_color)
            .add_modifier(border_modifier),
    );

    let input_inner = input_block.inner(chunks[1]);
    frame.render_widget(input_block, chunks[1]);

    // Render value based on field type
    if field.field_type == FieldType::Select {
        render_select_value(frame, input_inner, field, is_focused, fg_color);
    } else if matches!(field.field_type, FieldType::Amount(_)) {
        render_amount_value(frame, input_inner, field, is_focused, fg_color);
    } else if is_focused && is_editing && field.field_type != FieldType::ReadOnly {
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

fn render_select_value(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    field: &FormField,
    is_focused: bool,
    fg_color: Color,
) {
    let idx = field.selected_index();
    let total = field.options.len();

    let line = if is_focused && total > 0 {
        // Show navigation hints when focused
        Line::from(vec![
            Span::styled("◀ ", Style::default().fg(Color::DarkGray)),
            Span::styled(&field.value, Style::default().fg(fg_color)),
            Span::styled(" ▶", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("  ({}/{})", idx + 1, total),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else if field.value.is_empty() {
        Line::from(Span::styled("(none)", Style::default().fg(Color::DarkGray)))
    } else {
        Line::from(Span::styled(&field.value, Style::default().fg(fg_color)))
    };

    frame.render_widget(Paragraph::new(line), area);
}

fn render_amount_value(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    field: &FormField,
    is_focused: bool,
    fg_color: Color,
) {
    // Amount fields show their summary value (stored in field.value)
    // with a hint to press Enter to edit
    let line = if is_focused {
        // Truncate if too long for the display area
        let max_len = area.width.saturating_sub(8) as usize;
        let display_val = if field.value.len() > max_len {
            format!("{}...", &field.value[..max_len.saturating_sub(3)])
        } else {
            field.value.clone()
        };
        Line::from(vec![
            Span::styled(display_val, Style::default().fg(fg_color)),
            Span::styled(" [Edit]", Style::default().fg(Color::DarkGray)),
        ])
    } else if field.value.is_empty() {
        Line::from(Span::styled("$0.00", Style::default().fg(Color::DarkGray)))
    } else {
        // Truncate for non-focused display too
        let max_len = area.width as usize;
        let display_val = if field.value.len() > max_len {
            format!("{}...", &field.value[..max_len.saturating_sub(3)])
        } else {
            field.value.clone()
        };
        Line::from(Span::styled(display_val, Style::default().fg(fg_color)))
    };

    frame.render_widget(Paragraph::new(line), area);
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
        return ModalResult::Confirmed(
            modal.action,
            Box::new(ConfirmedValue::Form(Box::new(modal.clone()))),
        );
    }

    let field = &mut modal.fields[modal.focused_field];

    // Handle Amount fields - Enter opens the amount editor
    if matches!(field.field_type, FieldType::Amount(_)) {
        match key.code {
            KeyCode::Enter => {
                return ModalResult::AmountFieldActivated(modal.focused_field);
            }
            KeyCode::Tab | KeyCode::Down => {
                // Move to next field
                let start = modal.focused_field;
                loop {
                    modal.focused_field = (modal.focused_field + 1) % modal.fields.len();
                    if !matches!(
                        modal.fields[modal.focused_field].field_type,
                        FieldType::ReadOnly
                    ) || modal.focused_field == start
                    {
                        break;
                    }
                }
                return ModalResult::Continue;
            }
            KeyCode::BackTab | KeyCode::Up => {
                // Move to previous field
                let start = modal.focused_field;
                loop {
                    if modal.focused_field == 0 {
                        modal.focused_field = modal.fields.len() - 1;
                    } else {
                        modal.focused_field -= 1;
                    }
                    if !matches!(
                        modal.fields[modal.focused_field].field_type,
                        FieldType::ReadOnly
                    ) || modal.focused_field == start
                    {
                        break;
                    }
                }
                return ModalResult::Continue;
            }
            KeyCode::Esc => return ModalResult::Cancelled,
            _ => return ModalResult::Continue,
        }
    }

    // Handle Select fields specially - use Left/Right for option cycling
    if field.field_type == FieldType::Select {
        let field_index = modal.focused_field;
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                field.select_prev();
                return ModalResult::FieldChanged(field_index);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                field.select_next();
                return ModalResult::FieldChanged(field_index);
            }
            KeyCode::Enter | KeyCode::Tab | KeyCode::Down => {
                // Move to next field
                let start = modal.focused_field;
                loop {
                    modal.focused_field = (modal.focused_field + 1) % modal.fields.len();
                    if !matches!(
                        modal.fields[modal.focused_field].field_type,
                        FieldType::ReadOnly
                    ) || modal.focused_field == start
                    {
                        break;
                    }
                }
                return ModalResult::Continue;
            }
            KeyCode::BackTab | KeyCode::Up => {
                // Move to previous field
                let start = modal.focused_field;
                loop {
                    if modal.focused_field == 0 {
                        modal.focused_field = modal.fields.len() - 1;
                    } else {
                        modal.focused_field -= 1;
                    }
                    if !matches!(
                        modal.fields[modal.focused_field].field_type,
                        FieldType::ReadOnly
                    ) || modal.focused_field == start
                    {
                        break;
                    }
                }
                return ModalResult::Continue;
            }
            KeyCode::Esc => return ModalResult::Cancelled,
            _ => return ModalResult::Continue,
        }
    }

    match key.code {
        KeyCode::Enter => {
            // Exit edit mode, keep changes
            modal.editing = false;
            modal.editing_original_value = None;
            ModalResult::Continue
        }
        KeyCode::Esc => {
            // Cancel edit and revert to original value
            if let Some(original) = modal.editing_original_value.take() {
                modal.fields[modal.focused_field].value = original;
                modal.fields[modal.focused_field].cursor_pos =
                    modal.fields[modal.focused_field].value.len();
            }
            modal.editing = false;
            ModalResult::Continue
        }
        KeyCode::Tab | KeyCode::Down => {
            // Exit edit mode and move to next field
            modal.editing = false;
            modal.editing_original_value = None;
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
        KeyCode::BackTab | KeyCode::Up => {
            // Exit edit mode and move to previous field
            modal.editing = false;
            modal.editing_original_value = None;
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
            let valid = match &field.field_type {
                FieldType::Currency | FieldType::Percentage => {
                    c.is_ascii_digit() || c == '.' || c == '-'
                }
                FieldType::Text => true,
                FieldType::ReadOnly | FieldType::Select | FieldType::Amount(_) => false,
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
        return ModalResult::Confirmed(
            modal.action,
            Box::new(ConfirmedValue::Form(Box::new(modal.clone()))),
        );
    }

    let current_field_type = &modal.fields[modal.focused_field].field_type;

    // Handle Select field navigation with left/right
    if matches!(current_field_type, FieldType::Select) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                modal.fields[modal.focused_field].select_prev();
                return ModalResult::Continue;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                modal.fields[modal.focused_field].select_next();
                return ModalResult::Continue;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Enter | KeyCode::Char('e') => {
            // Handle different field types
            match current_field_type {
                // Amount fields open a nested modal for editing
                FieldType::Amount(_) => {
                    return ModalResult::AmountFieldActivated(modal.focused_field);
                }
                // Text, Currency, Percentage fields enter inline edit mode
                FieldType::Text | FieldType::Currency | FieldType::Percentage => {
                    modal.editing = true;
                    // Store original value for Esc to revert
                    modal.editing_original_value =
                        Some(modal.fields[modal.focused_field].value.clone());
                    // Move cursor to end of value
                    let field = &mut modal.fields[modal.focused_field];
                    field.cursor_pos = field.value.len();
                }
                // ReadOnly and Select don't enter edit mode on Enter
                FieldType::ReadOnly | FieldType::Select => {}
            }
            ModalResult::Continue
        }
        KeyCode::Esc => ModalResult::Cancelled,
        KeyCode::Tab | KeyCode::Char('j') | KeyCode::Down => {
            // Skip read-only fields when navigating
            let start = modal.focused_field;
            loop {
                modal.focused_field = (modal.focused_field + 1) % modal.fields.len();
                if !matches!(
                    modal.fields[modal.focused_field].field_type,
                    FieldType::ReadOnly
                ) || modal.focused_field == start
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
                if !matches!(
                    modal.fields[modal.focused_field].field_type,
                    FieldType::ReadOnly
                ) || modal.focused_field == start
                {
                    break;
                }
            }
            ModalResult::Continue
        }
        _ => ModalResult::Continue,
    }
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
