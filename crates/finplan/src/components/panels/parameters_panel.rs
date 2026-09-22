use crate::{
    actions::parameter,
    components::EventResult,
    data::keybindings_data::KeybindingsConfig,
    modals::{ConfirmModal, ModalAction, ModalState, ParameterAction},
    state::{AppState, EventsPanel},
    util::styles::focused_block_with_help,
};
use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{List, ListItem, ListState},
};

pub struct ParametersPanel;
impl ParametersPanel {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let focused = state.events_state.focused_panel == EventsPanel::Parameters;
        let kb = &state.keybindings.tabs.events;
        let label = |keys: &Vec<String>| keys.first().cloned().unwrap_or_default();
        let help = format!(
            "[{}]dd [{}]dit [{}]el",
            label(&kb.add),
            label(&kb.edit),
            label(&kb.delete)
        );
        let items: Vec<ListItem> = if state.data().named_parameters.is_empty() {
            vec![ListItem::new("No parameters configured.")]
        } else {
            state
                .data()
                .named_parameters
                .iter()
                .map(|p| {
                    ListItem::new(format!(
                        "{} ({}) = {}",
                        p.name,
                        parameter::kind_name(p.value),
                        parameter::display_value(p.value)
                    ))
                })
                .collect()
        };
        let list = List::new(items)
            .block(focused_block_with_help(" PARAMETERS ", focused, &help))
            .highlight_symbol("› ")
            .highlight_style(ratatui::style::Style::default().fg(ratatui::style::Color::Yellow));
        let mut selection =
            ListState::default().with_selected(Some(state.events_state.selected_parameter_index));
        frame.render_stateful_widget(list, area, &mut selection);
    }
    pub fn handle_key(key: KeyEvent, state: &mut AppState) -> EventResult {
        let len = state.data().named_parameters.len();
        let kb = &state.keybindings;
        let index = state.events_state.selected_parameter_index;
        if KeybindingsConfig::matches(&key, &kb.navigation.down) {
            if len > 0 {
                state.events_state.selected_parameter_index = (index + 1) % len;
            }
        } else if KeybindingsConfig::matches(&key, &kb.navigation.up) {
            if len > 0 {
                state.events_state.selected_parameter_index = (index + len - 1) % len;
            }
        } else if KeybindingsConfig::matches(&key, &kb.tabs.events.add) {
            state.modal = parameter::type_picker(state, None);
        } else if KeybindingsConfig::matches(&key, &kb.tabs.events.edit) {
            if index < len {
                state.modal = parameter::type_picker(state, Some(index));
            }
        } else if KeybindingsConfig::matches(&key, &kb.tabs.events.delete) {
            if let Some(p) = state.data().named_parameters.get(index) {
                state.modal = ModalState::Confirm(ConfirmModal::new(
                    "Delete Parameter",
                    &format!("Delete parameter '{}' ?", p.name),
                    ModalAction::Parameter(ParameterAction::Delete { index }),
                ));
            }
        } else {
            return EventResult::NotHandled;
        }
        EventResult::Handled
    }
}
