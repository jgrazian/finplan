// Effect actions - managing effects on events

use crate::data::events_data::{AccountTag, AmountData, EffectData, EventTag};
use crate::modals::parse_currency;
use crate::screens::events::EventsScreen;
use crate::state::{AppState, ConfirmModal, FormField, FormModal, ModalAction, ModalState, PickerModal};

use super::{ActionContext, ActionResult};

/// Handle effect management picker selection
pub fn handle_manage_effects(state: &AppState, selected: &str) -> ActionResult {
    let event_idx = state.events_state.selected_event_index;

    if selected == "[ + Add New Effect ]" {
        // Show effect type picker
        let effect_types = vec![
            "Income".to_string(),
            "Expense".to_string(),
            "Trigger Event".to_string(),
            "Pause Event".to_string(),
            "Resume Event".to_string(),
            "Terminate Event".to_string(),
        ];
        return ActionResult::modal(ModalState::Picker(PickerModal::new(
            "Select Effect Type",
            effect_types,
            ModalAction::PICK_EFFECT_TYPE_FOR_ADD,
        )));
    }

    // Parse effect index from "N. description" format
    if let Some(effect_idx) = selected
        .split('.')
        .next()
        .and_then(|s| s.parse::<usize>().ok())
    {
        let effect_idx = effect_idx - 1; // Convert to 0-based index
        if let Some(event) = state.data().events.get(event_idx)
            && let Some(effect) = event.effects.get(effect_idx)
        {
            let effect_desc = EventsScreen::format_effect(effect);
            return ActionResult::modal(ModalState::Confirm(
                ConfirmModal::new(
                    "Delete Effect",
                    &format!("Delete effect: {}?", effect_desc),
                    ModalAction::DELETE_EFFECT,
                )
                .with_context(&format!("{}:{}", event_idx, effect_idx)),
            ));
        }
    }
    ActionResult::close()
}

/// Handle effect type selection for adding new effect
pub fn handle_effect_type_for_add(state: &AppState, effect_type: &str) -> ActionResult {
    let event_idx = state.events_state.selected_event_index;
    let accounts = EventsScreen::get_account_names(state);
    let events = EventsScreen::get_event_names(state);

    let first_account = accounts.first().map(|s| s.as_str()).unwrap_or("");
    let first_event = events.first().map(|s| s.as_str()).unwrap_or("");

    match effect_type {
        "Income" => {
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Income Effect",
                    vec![
                        FormField::text("To Account", first_account),
                        FormField::currency("Amount", 0.0),
                        FormField::text("Gross (Y/N)", "N"),
                        FormField::text("Taxable (Y/N)", "Y"),
                    ],
                    ModalAction::ADD_EFFECT,
                )
                .with_context(&format!("Income|{}", event_idx))
                .start_editing(),
            ))
        }
        "Expense" => {
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Expense Effect",
                    vec![
                        FormField::text("From Account", first_account),
                        FormField::currency("Amount", 0.0),
                    ],
                    ModalAction::ADD_EFFECT,
                )
                .with_context(&format!("Expense|{}", event_idx))
                .start_editing(),
            ))
        }
        "Trigger Event" => {
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Trigger Effect",
                    vec![FormField::text("Event to Trigger", first_event)],
                    ModalAction::ADD_EFFECT,
                )
                .with_context(&format!("TriggerEvent|{}", event_idx))
                .start_editing(),
            ))
        }
        "Pause Event" => {
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Pause Effect",
                    vec![FormField::text("Event to Pause", first_event)],
                    ModalAction::ADD_EFFECT,
                )
                .with_context(&format!("PauseEvent|{}", event_idx))
                .start_editing(),
            ))
        }
        "Resume Event" => {
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Resume Effect",
                    vec![FormField::text("Event to Resume", first_event)],
                    ModalAction::ADD_EFFECT,
                )
                .with_context(&format!("ResumeEvent|{}", event_idx))
                .start_editing(),
            ))
        }
        "Terminate Event" => {
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Terminate Effect",
                    vec![FormField::text("Event to Terminate", first_event)],
                    ModalAction::ADD_EFFECT,
                )
                .with_context(&format!("TerminateEvent|{}", event_idx))
                .start_editing(),
            ))
        }
        _ => ActionResult::close(),
    }
}

/// Handle adding an effect to an event
pub fn handle_add_effect(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let context_str = ctx.context_str();
    let ctx_parts: Vec<&str> = context_str.split('|').collect();
    let effect_type = ctx_parts.first().copied().unwrap_or("");
    let event_idx: usize = ctx_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

    let form_parts = ctx.value_parts();

    let effect = match effect_type {
        "Income" => {
            let to_account = form_parts.first().unwrap_or(&"").to_string();
            let amount = form_parts
                .get(1)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);
            let gross = form_parts
                .get(2)
                .map(|s| s.to_uppercase().starts_with('Y'))
                .unwrap_or(false);
            let taxable = form_parts
                .get(3)
                .map(|s| s.to_uppercase().starts_with('Y'))
                .unwrap_or(true);

            Some(EffectData::Income {
                to: AccountTag(to_account),
                amount: AmountData::Fixed(amount),
                gross,
                taxable,
            })
        }
        "Expense" => {
            let from_account = form_parts.first().unwrap_or(&"").to_string();
            let amount = form_parts
                .get(1)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);

            Some(EffectData::Expense {
                from: AccountTag(from_account),
                amount: AmountData::Fixed(amount),
            })
        }
        "TriggerEvent" => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            Some(EffectData::TriggerEvent {
                event: EventTag(event_name),
            })
        }
        "PauseEvent" => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            Some(EffectData::PauseEvent {
                event: EventTag(event_name),
            })
        }
        "ResumeEvent" => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            Some(EffectData::ResumeEvent {
                event: EventTag(event_name),
            })
        }
        "TerminateEvent" => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            Some(EffectData::TerminateEvent {
                event: EventTag(event_name),
            })
        }
        _ => None,
    };

    if let Some(effect) = effect
        && let Some(event) = state.data_mut().events.get_mut(event_idx)
    {
        event.effects.push(effect);
        ActionResult::modified()
    } else {
        ActionResult::close()
    }
}

/// Handle effect deletion
pub fn handle_delete_effect(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    // Context format: "event_idx:effect_idx"
    let indices = ctx.indices();

    if indices.len() != 2 {
        return ActionResult::close();
    }

    let (event_idx, effect_idx) = (indices[0], indices[1]);

    if let Some(event) = state.data_mut().events.get_mut(event_idx)
        && effect_idx < event.effects.len()
    {
        event.effects.remove(effect_idx);
        ActionResult::modified()
    } else {
        ActionResult::close()
    }
}
