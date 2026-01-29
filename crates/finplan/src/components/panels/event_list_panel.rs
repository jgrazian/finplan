//! Event list panel component extracted from EventsScreen.
//!
//! Renders the event list with selection and filtering.

use crate::components::EventResult;
use crate::data::events_data::{AmountData, EffectData, EventTag, SpecialAmount, TriggerData};
use crate::modals::context::ModalContext;
use crate::state::{
    AppState, ConfirmModal, EventsPanel, FormField, FormModal, ModalAction, ModalState, PickerModal,
};
use crate::util::styles::focused_block_with_help;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
};

/// Event list panel component.
pub struct EventListPanel;

impl EventListPanel {
    /// Render the event list panel.
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.events_state.focused_panel == EventsPanel::EventList;

        let items: Vec<ListItem> = if state.data().events.is_empty() {
            vec![
                ListItem::new(Line::from(Span::styled(
                    "No events configured.",
                    Style::default().fg(Color::DarkGray),
                ))),
                ListItem::new(Line::from(Span::styled(
                    "Press 'a' to add.",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        } else {
            state
                .data()
                .events
                .iter()
                .enumerate()
                .map(|(idx, event)| {
                    let enabled_prefix = if event.enabled { "✓" } else { "x" };

                    // Create inline effect preview
                    let effect_preview = if event.effects.is_empty() {
                        "No effects".to_string()
                    } else {
                        let first_effect = Self::format_effect_short(&event.effects[0]);
                        if event.effects.len() > 1 {
                            format!("{} +{}", first_effect, event.effects.len() - 1)
                        } else {
                            first_effect
                        }
                    };

                    let base_style = if !event.enabled {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default()
                    };

                    let style = if idx == state.events_state.selected_event_index {
                        base_style.fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        base_style
                    };

                    // Compact format: [✓] Name: effect_preview
                    let line = Line::from(vec![
                        Span::styled(format!("[{}] ", enabled_prefix), style),
                        Span::styled(&event.name.0, style.add_modifier(Modifier::BOLD)),
                        Span::styled(": ", style),
                        Span::styled(effect_preview, style),
                    ]);

                    ListItem::new(line)
                })
                .collect()
        };

        let help_text = " [a]dd [e]dit [d]el [c]opy [Shift+J/K] Reorder [t]oggle ";
        let block = focused_block_with_help(" EVENTS ", is_focused, help_text);

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }

    /// Handle key events for the event list panel.
    pub fn handle_key(key: KeyEvent, state: &mut AppState) -> EventResult {
        let events_len = state.data().events.len();
        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);

        match key.code {
            // Move down (Shift+J or Shift+Down)
            KeyCode::Char('J') if has_shift => {
                let idx = state.events_state.selected_event_index;
                if events_len >= 2 && idx < events_len - 1 {
                    state.data_mut().events.swap(idx, idx + 1);
                    state.events_state.selected_event_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Down if has_shift => {
                let idx = state.events_state.selected_event_index;
                if events_len >= 2 && idx < events_len - 1 {
                    state.data_mut().events.swap(idx, idx + 1);
                    state.events_state.selected_event_index = idx + 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            // Move up (Shift+K or Shift+Up)
            KeyCode::Char('K') if has_shift => {
                let idx = state.events_state.selected_event_index;
                if events_len >= 2 && idx > 0 {
                    state.data_mut().events.swap(idx, idx - 1);
                    state.events_state.selected_event_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Up if has_shift => {
                let idx = state.events_state.selected_event_index;
                if events_len >= 2 && idx > 0 {
                    state.data_mut().events.swap(idx, idx - 1);
                    state.events_state.selected_event_index = idx - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if events_len > 0 {
                    state.events_state.selected_event_index =
                        (state.events_state.selected_event_index + 1) % events_len;
                }
                EventResult::Handled
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if events_len > 0 {
                    if state.events_state.selected_event_index == 0 {
                        state.events_state.selected_event_index = events_len - 1;
                    } else {
                        state.events_state.selected_event_index -= 1;
                    }
                }
                EventResult::Handled
            }
            KeyCode::Char('t') => {
                // Toggle enabled status
                let idx = state.events_state.selected_event_index;
                if let Some(event) = state.data_mut().events.get_mut(idx) {
                    event.enabled = !event.enabled;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('a') => {
                // Add new event - show trigger type picker
                let trigger_types = vec![
                    "Date".to_string(),
                    "Age".to_string(),
                    "Repeating".to_string(),
                    "Manual".to_string(),
                    "Account Balance".to_string(),
                    "Net Worth".to_string(),
                    "Relative to Event".to_string(),
                    "Quick Events".to_string(),
                ];
                state.modal = ModalState::Picker(PickerModal::new(
                    "Select Trigger Type",
                    trigger_types,
                    ModalAction::PICK_TRIGGER_TYPE,
                ));
                EventResult::Handled
            }
            KeyCode::Char('e') => {
                // Edit selected event
                if let Some(event) = state
                    .data()
                    .events
                    .get(state.events_state.selected_event_index)
                {
                    let trigger_summary = Self::format_trigger_short(&event.trigger);
                    let effects_summary = format!("{} effect(s)", event.effects.len());

                    let yes_no = vec!["No".to_string(), "Yes".to_string()];
                    let form = FormModal::new(
                        "Edit Event",
                        vec![
                            FormField::text("Name", &event.name.0),
                            FormField::text(
                                "Description",
                                event.description.as_deref().unwrap_or(""),
                            ),
                            FormField::select(
                                "Once Only",
                                yes_no.clone(),
                                if event.once { "Yes" } else { "No" },
                            ),
                            FormField::select(
                                "Enabled",
                                yes_no,
                                if event.enabled { "Yes" } else { "No" },
                            ),
                            FormField::read_only("Trigger", &trigger_summary),
                            FormField::read_only("Effects", &effects_summary),
                        ],
                        ModalAction::EDIT_EVENT,
                    )
                    .with_typed_context(ModalContext::event_index(
                        state.events_state.selected_event_index,
                    ));

                    state.modal = ModalState::Form(form);
                }
                EventResult::Handled
            }
            KeyCode::Char('d') => {
                // Delete selected event with confirmation
                if let Some(event) = state
                    .data()
                    .events
                    .get(state.events_state.selected_event_index)
                {
                    state.modal = ModalState::Confirm(
                        ConfirmModal::new(
                            "Delete Event",
                            &format!("Delete event '{}'?\n\nThis cannot be undone.", event.name.0),
                            ModalAction::DELETE_EVENT,
                        )
                        .with_typed_context(ModalContext::event_index(
                            state.events_state.selected_event_index,
                        )),
                    );
                }
                EventResult::Handled
            }
            KeyCode::Char('c') => {
                // Copy selected event
                let idx = state.events_state.selected_event_index;
                if let Some(event) = state.data().events.get(idx).cloned() {
                    let mut new_event = event;
                    new_event.name = EventTag(format!("{} (Copy)", new_event.name.0));
                    state.data_mut().events.push(new_event);
                    // Select the newly copied event
                    state.events_state.selected_event_index = state.data().events.len() - 1;
                    state.mark_modified();
                }
                EventResult::Handled
            }
            KeyCode::Char('f') => {
                // Manage effects for selected event
                let event_idx = state.events_state.selected_event_index;
                if let Some(event) = state.data().events.get(event_idx) {
                    // Build list of current effects + add option
                    let mut options: Vec<String> = event
                        .effects
                        .iter()
                        .enumerate()
                        .map(|(i, effect)| format!("{}. {}", i + 1, Self::format_effect(effect)))
                        .collect();
                    options.push("[ + Add New Effect ]".to_string());

                    state.modal = ModalState::Picker(PickerModal::new(
                        &format!("Manage Effects - {}", event.name.0),
                        options,
                        ModalAction::MANAGE_EFFECTS,
                    ));
                }
                EventResult::Handled
            }
            _ => EventResult::NotHandled,
        }
    }

    // ========== Helper Functions ==========

    /// Format an effect in short form for inline preview.
    pub fn format_effect_short(effect: &EffectData) -> String {
        match effect {
            EffectData::Income { to, amount, .. } => {
                format!("Income -> {}: {}", to.0, Self::format_amount_short(amount))
            }
            EffectData::Expense { from, amount } => {
                format!(
                    "Expense <- {}: {}",
                    from.0,
                    Self::format_amount_short(amount)
                )
            }
            EffectData::AssetPurchase { asset, amount, .. } => {
                format!("Buy {}: {}", asset.0, Self::format_amount_short(amount))
            }
            EffectData::AssetSale { asset, amount, .. } => {
                if let Some(a) = asset {
                    format!("Sell {}: {}", a.0, Self::format_amount_short(amount))
                } else {
                    format!("Liquidate: {}", Self::format_amount_short(amount))
                }
            }
            EffectData::Sweep { to, amount, .. } => {
                format!("Sweep -> {}: {}", to.0, Self::format_amount_short(amount))
            }
            EffectData::TriggerEvent { event } => format!("Trigger {}", event.0),
            EffectData::PauseEvent { event } => format!("Pause {}", event.0),
            EffectData::ResumeEvent { event } => format!("Resume {}", event.0),
            EffectData::TerminateEvent { event } => format!("End {}", event.0),
            EffectData::ApplyRmd { .. } => "Apply RMD".to_string(),
            EffectData::AdjustBalance { account, amount } => {
                format!(
                    "Adjust {}: {}",
                    account.0,
                    Self::format_amount_short(amount)
                )
            }
            EffectData::CashTransfer { from, to, amount } => {
                format!(
                    "Transfer {} -> {}: {}",
                    from.0,
                    to.0,
                    Self::format_amount_short(amount)
                )
            }
        }
    }

    /// Format amount in short form.
    pub fn format_amount_short(amount: &AmountData) -> String {
        match amount {
            AmountData::Fixed(val) => {
                if *val >= 1_000_000.0 {
                    format!("${:.1}M", val / 1_000_000.0)
                } else if *val >= 1_000.0 {
                    format!("${:.0}K", val / 1_000.0)
                } else {
                    format!("${:.0}", val)
                }
            }
            AmountData::Special(special) => match special {
                SpecialAmount::SourceBalance => "SrcBal".to_string(),
                SpecialAmount::ZeroTargetBalance => "ZeroTgt".to_string(),
                SpecialAmount::TargetToBalance { .. } => "TgtBal".to_string(),
                SpecialAmount::AccountBalance { .. } => "AcctBal".to_string(),
                SpecialAmount::AccountCashBalance { .. } => "CashBal".to_string(),
            },
        }
    }

    /// Format trigger in short form.
    fn format_trigger_short(trigger: &TriggerData) -> String {
        match trigger {
            TriggerData::Date { date } => format!("Date: {}", date),
            TriggerData::Age { years, .. } => format!("Age: {}", years),
            TriggerData::Repeating { interval, .. } => {
                format!("Repeating: {}", Self::format_interval(interval))
            }
            TriggerData::Manual => "Manual".to_string(),
            TriggerData::AccountBalance { account, .. } => format!("Acct Bal: {}", account.0),
            TriggerData::AssetBalance { account, asset, .. } => {
                format!("Asset Bal: {}/{}", account.0, asset.0)
            }
            TriggerData::NetWorth { .. } => "Net Worth".to_string(),
            TriggerData::RelativeToEvent { event, .. } => format!("Relative: {}", event.0),
            TriggerData::And { conditions } => format!("AND ({})", conditions.len()),
            TriggerData::Or { conditions } => format!("OR ({})", conditions.len()),
        }
    }

    /// Format interval for display.
    fn format_interval(interval: &crate::data::events_data::IntervalData) -> String {
        use crate::data::events_data::IntervalData;
        match interval {
            IntervalData::Never => "Never".to_string(),
            IntervalData::Weekly => "Weekly".to_string(),
            IntervalData::BiWeekly => "Bi-weekly".to_string(),
            IntervalData::Monthly => "Monthly".to_string(),
            IntervalData::Quarterly => "Quarterly".to_string(),
            IntervalData::Yearly => "Yearly".to_string(),
        }
    }

    /// Format effect for full display in effect list.
    fn format_effect(effect: &EffectData) -> String {
        // Use the short format for now, can be expanded later
        Self::format_effect_short(effect)
    }
}
