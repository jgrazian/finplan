use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Constraint,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};

use crate::data::keybindings_data::KeybindingsConfig;
use crate::modals::PickerModal;

use super::helpers::{HelpText, MultiLineHelp, render_modal_frame};
use super::{ConfirmedValue, ModalResult};

/// Render the picker modal
pub fn render_picker_modal(frame: &mut Frame, modal: &mut PickerModal) {
    let screen_height = frame.area().height;
    // overhead: title border (1) + bottom border (1) + spacing (1) + spacing (1) + help (2) + scroll indicators (2)
    let overhead: u16 = 8;
    let max_content = screen_height.saturating_sub(overhead + 2); // extra margin
    let content_height = (modal.options.len() as u16).clamp(3, max_content);
    let modal_height = content_height + overhead;
    let modal_width = 60;

    let viewport = content_height as usize;
    modal.viewport_height = viewport;
    // Re-sync scroll offset for actual viewport (e.g. if terminal resized)
    modal.ensure_visible();

    let can_scroll_up = modal.scroll_offset > 0;
    let can_scroll_down = modal.scroll_offset + viewport < modal.options.len();

    // Render the modal frame
    let mf = render_modal_frame(
        frame,
        &modal.title,
        modal_width,
        modal_height,
        Color::Cyan,
        &[
            Constraint::Length(1), // Scroll-up indicator
            Constraint::Min(1),    // Options list
            Constraint::Length(1), // Scroll-down indicator
            Constraint::Length(1), // Spacing
            Constraint::Length(2), // Help text (2 lines)
        ],
    );

    // Scroll-up indicator
    if can_scroll_up {
        let indicator = Paragraph::new(Line::from(Span::styled(
            "  ▲ more",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(indicator, mf.chunks[0]);
    }

    // Render visible options
    let end = (modal.scroll_offset + viewport).min(modal.options.len());
    let items: Vec<ListItem> = modal.options[modal.scroll_offset..end]
        .iter()
        .enumerate()
        .map(|(vi, option)| {
            let actual_idx = modal.scroll_offset + vi;
            let style = if actual_idx == modal.selected_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let prefix = if actual_idx == modal.selected_index {
                "> "
            } else {
                "  "
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, option),
                style,
            )))
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, mf.chunks[1]);

    // Scroll-down indicator
    if can_scroll_down {
        let indicator = Paragraph::new(Line::from(Span::styled(
            "  ▼ more",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(indicator, mf.chunks[2]);
    }

    // Render help text (2 lines)
    let help = MultiLineHelp::new()
        .line(HelpText::new().key("[j/k/↑/↓]", Color::DarkGray, "Navigate"))
        .line(HelpText::new().key("[Enter]", Color::Green, "Select").key(
            "[Esc]",
            Color::Yellow,
            "Cancel",
        ))
        .build();
    frame.render_widget(help, mf.chunks[4]);
}

/// Handle key events for picker modal
pub fn handle_picker_key(
    key: KeyEvent,
    modal: &mut PickerModal,
    keybindings: &KeybindingsConfig,
) -> ModalResult {
    // Check for confirm
    if KeybindingsConfig::matches(&key, &keybindings.navigation.confirm) {
        if let Some(selected) = modal.options.get(modal.selected_index) {
            return ModalResult::Confirmed(
                modal.action,
                Box::new(ConfirmedValue::Picker(selected.clone())),
            );
        } else {
            return ModalResult::Cancelled;
        }
    }

    // Check for cancel
    if KeybindingsConfig::matches(&key, &keybindings.global.cancel) {
        return ModalResult::Cancelled;
    }

    // Check for navigation down
    if KeybindingsConfig::matches(&key, &keybindings.navigation.down) {
        if !modal.options.is_empty() {
            modal.selected_index = (modal.selected_index + 1) % modal.options.len();
            if modal.selected_index == 0 {
                modal.scroll_offset = 0; // Wrapped to top
            }
            modal.ensure_visible();
        }
        return ModalResult::Continue;
    }

    // Check for navigation up
    if KeybindingsConfig::matches(&key, &keybindings.navigation.up) {
        if !modal.options.is_empty() {
            if modal.selected_index == 0 {
                modal.selected_index = modal.options.len() - 1;
            } else {
                modal.selected_index -= 1;
            }
            modal.ensure_visible();
        }
        return ModalResult::Continue;
    }

    // Home/End navigation (keep hardcoded as they're standard terminal keys)
    match key.code {
        KeyCode::Home => {
            modal.selected_index = 0;
            modal.ensure_visible();
            ModalResult::Continue
        }
        KeyCode::End => {
            if !modal.options.is_empty() {
                modal.selected_index = modal.options.len() - 1;
            }
            modal.ensure_visible();
            ModalResult::Continue
        }
        _ => ModalResult::Continue,
    }
}
