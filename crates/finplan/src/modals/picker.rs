use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Constraint,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
};

use crate::data::keybindings_data::KeybindingsConfig;
use crate::modals::PickerModal;

use super::helpers::{HelpText, MultiLineHelp, render_modal_frame};
use super::{ConfirmedValue, ModalResult};

/// Render the picker modal
pub fn render_picker_modal(frame: &mut Frame, modal: &PickerModal) {
    // Calculate height based on number of options (min 6, max 15)
    let content_height = (modal.options.len() as u16).clamp(3, 12);
    let modal_height = content_height + 7; // title + borders + help text + padding
    let modal_width = 60;

    // Render the modal frame
    let mf = render_modal_frame(
        frame,
        &modal.title,
        modal_width,
        modal_height,
        Color::Cyan,
        &[
            Constraint::Length(1), // Spacing
            Constraint::Min(1),    // Options list
            Constraint::Length(1), // Spacing
            Constraint::Length(2), // Help text (2 lines)
        ],
    );

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

            let prefix = if idx == modal.selected_index {
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

    // Render help text (2 lines)
    let help = MultiLineHelp::new()
        .line(HelpText::new().key("[j/k/↑/↓]", Color::DarkGray, "Navigate"))
        .line(HelpText::new().key("[Enter]", Color::Green, "Select").key(
            "[Esc]",
            Color::Yellow,
            "Cancel",
        ))
        .build();
    frame.render_widget(help, mf.chunks[3]);
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
        }
        return ModalResult::Continue;
    }

    // Home/End navigation (keep hardcoded as they're standard terminal keys)
    match key.code {
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
