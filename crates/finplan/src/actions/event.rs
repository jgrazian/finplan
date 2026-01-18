// Event actions - trigger type picking, CRUD operations

use crate::data::events_data::{
    AccountTag, EventData, EventTag, IntervalData, OffsetData, ThresholdData, TriggerData,
};
use crate::modals::parse_currency;
use crate::screens::events::EventsScreen;
use crate::state::{AppState, FormField, FormModal, ModalAction, ModalState, PickerModal};

use super::{ActionContext, ActionResult};

/// Handle trigger type selection - shows appropriate form or picker
pub fn handle_trigger_type_pick(state: &AppState, trigger_type: &str) -> ActionResult {
    let (title, fields, context) = match trigger_type {
        "Date" => (
            "New Event - Date Trigger",
            vec![
                FormField::text("Event Name", ""),
                FormField::text("Description", ""),
                FormField::text("Date (YYYY-MM-DD)", "2025-01-01"),
                FormField::text("Once Only (Y/N)", "N"),
            ],
            "Date".to_string(),
        ),
        "Age" => (
            "New Event - Age Trigger",
            vec![
                FormField::text("Event Name", ""),
                FormField::text("Description", ""),
                FormField::text("Age (years)", "65"),
                FormField::text("Once Only (Y/N)", "Y"),
            ],
            "Age".to_string(),
        ),
        "Repeating" => {
            // Show interval picker first
            let intervals = vec![
                "Weekly".to_string(),
                "Bi-Weekly".to_string(),
                "Monthly".to_string(),
                "Quarterly".to_string(),
                "Yearly".to_string(),
            ];
            return ActionResult::modal(ModalState::Picker(PickerModal::new(
                "Select Repeat Interval",
                intervals,
                ModalAction::PICK_INTERVAL,
            )));
        }
        "Manual" => (
            "New Event - Manual Trigger",
            vec![
                FormField::text("Event Name", ""),
                FormField::text("Description", ""),
                FormField::text("Once Only (Y/N)", "N"),
            ],
            "Manual".to_string(),
        ),
        "Account Balance" => {
            // Get account list
            let accounts = EventsScreen::get_account_names(state);
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            return ActionResult::modal(ModalState::Picker(PickerModal::new(
                "Select Account for Balance Trigger",
                accounts,
                ModalAction::PICK_ACCOUNT_FOR_EFFECT,
            )));
        }
        "Net Worth" => (
            "New Event - Net Worth Trigger",
            vec![
                FormField::text("Event Name", ""),
                FormField::text("Description", ""),
                FormField::currency("Threshold", 1000000.0),
                FormField::text("Comparison (>=/<= )", ">="),
                FormField::text("Once Only (Y/N)", "Y"),
            ],
            "NetWorth".to_string(),
        ),
        "Relative to Event" => {
            // Get event list
            let events = EventsScreen::get_event_names(state);
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            return ActionResult::modal(ModalState::Picker(PickerModal::new(
                "Select Reference Event",
                events,
                ModalAction::PICK_EVENT_REFERENCE,
            )));
        }
        _ => return ActionResult::close(),
    };

    ActionResult::modal(ModalState::Form(
        FormModal::new(title, fields, ModalAction::CREATE_EVENT)
            .with_context(&context)
            .start_editing(),
    ))
}

/// Handle interval selection for repeating events
pub fn handle_interval_pick(interval: &str) -> ActionResult {
    let interval_str = interval.to_string();
    ActionResult::modal(ModalState::Form(
        FormModal::new(
            &format!("New Event - {} Repeating", interval),
            vec![
                FormField::text("Event Name", ""),
                FormField::text("Description", ""),
                FormField::read_only("Interval", &interval_str),
                FormField::text("Start Date (YYYY-MM-DD, optional)", ""),
                FormField::text("End Age (years, optional)", ""),
            ],
            ModalAction::CREATE_EVENT,
        )
        .with_context(&format!("Repeating|{}", interval))
        .start_editing(),
    ))
}

/// Handle account selection for balance trigger
pub fn handle_account_for_effect_pick(account: &str) -> ActionResult {
    ActionResult::modal(ModalState::Form(
        FormModal::new(
            "New Event - Account Balance Trigger",
            vec![
                FormField::text("Event Name", ""),
                FormField::text("Description", ""),
                FormField::read_only("Account", account),
                FormField::currency("Threshold", 100000.0),
                FormField::text("Comparison (>=/<= )", ">="),
                FormField::text("Once Only (Y/N)", "Y"),
            ],
            ModalAction::CREATE_EVENT,
        )
        .with_context(&format!("AccountBalance|{}", account))
        .start_editing(),
    ))
}

/// Handle event reference selection for relative triggers
pub fn handle_event_reference_pick(event_ref: &str) -> ActionResult {
    ActionResult::modal(ModalState::Form(
        FormModal::new(
            "New Event - Relative to Event",
            vec![
                FormField::text("Event Name", ""),
                FormField::text("Description", ""),
                FormField::read_only("Reference Event", event_ref),
                FormField::text("Offset Years", "0"),
                FormField::text("Offset Months", "0"),
                FormField::text("Once Only (Y/N)", "Y"),
            ],
            ModalAction::CREATE_EVENT,
        )
        .with_context(&format!("RelativeToEvent|{}", event_ref))
        .start_editing(),
    ))
}

/// Handle event creation
pub fn handle_create_event(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let parts = ctx.value_parts();
    let trigger_type = ctx.context_str();

    // Parse trigger type and create appropriate event
    let (trigger, name, description, once) = match trigger_type {
        "Date" => parse_date_trigger(&parts),
        "Age" => parse_age_trigger(&parts),
        "Manual" => parse_manual_trigger(&parts),
        "NetWorth" => parse_net_worth_trigger(&parts),
        s if s.starts_with("Repeating|") => parse_repeating_trigger(s, &parts),
        s if s.starts_with("AccountBalance|") => parse_account_balance_trigger(s, &parts),
        s if s.starts_with("RelativeToEvent|") => parse_relative_trigger(s, &parts),
        _ => return ActionResult::close(),
    };

    if name.is_empty() {
        return ActionResult::error("Event name cannot be empty");
    }

    let event = EventData {
        name: EventTag(name),
        description,
        trigger,
        effects: vec![],
        once,
        enabled: true,
    };

    state.data_mut().events.push(event);
    // Select the newly created event
    state.events_state.selected_event_index = state.data().events.len() - 1;
    ActionResult::modified()
}

/// Handle event editing
pub fn handle_edit_event(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let idx = match ctx.index() {
        Some(i) => i,
        None => return ActionResult::close(),
    };

    let parts = ctx.value_parts();

    if let Some(event) = state.data_mut().events.get_mut(idx) {
        // Parts: [name, description, once, enabled, trigger (ro), effects (ro)]
        if let Some(name) = parts.first()
            && !name.is_empty()
        {
            event.name = EventTag(name.to_string());
        }
        event.description = parts
            .get(1)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        if let Some(once_str) = parts.get(2) {
            event.once = once_str.to_uppercase().starts_with('Y');
        }
        if let Some(enabled_str) = parts.get(3) {
            event.enabled = enabled_str.to_uppercase().starts_with('Y');
        }
        ActionResult::modified()
    } else {
        ActionResult::close()
    }
}

/// Handle event deletion
pub fn handle_delete_event(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    if let Some(idx) = ctx.index() {
        let events_len = state.data().events.len();
        if idx < events_len {
            state.data_mut().events.remove(idx);
            let new_len = state.data().events.len();
            // Adjust selected index
            if state.events_state.selected_event_index >= new_len && new_len > 0 {
                state.events_state.selected_event_index = new_len - 1;
            }
            return ActionResult::modified();
        }
    }
    ActionResult::close()
}

// Helper functions for parsing trigger data

fn parse_date_trigger(parts: &[&str]) -> (TriggerData, String, Option<String>, bool) {
    let name = parts.first().unwrap_or(&"").to_string();
    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let date = parts.get(2).unwrap_or(&"2025-01-01").to_string();
    let once = parts
        .get(3)
        .map(|s| s.to_uppercase().starts_with('Y'))
        .unwrap_or(false);

    (TriggerData::Date { date }, name, desc, once)
}

fn parse_age_trigger(parts: &[&str]) -> (TriggerData, String, Option<String>, bool) {
    let name = parts.first().unwrap_or(&"").to_string();
    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let years: u8 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(65);
    let once = parts
        .get(3)
        .map(|s| s.to_uppercase().starts_with('Y'))
        .unwrap_or(true);

    (
        TriggerData::Age {
            years,
            months: None,
        },
        name,
        desc,
        once,
    )
}

fn parse_manual_trigger(parts: &[&str]) -> (TriggerData, String, Option<String>, bool) {
    let name = parts.first().unwrap_or(&"").to_string();
    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let once = parts
        .get(2)
        .map(|s| s.to_uppercase().starts_with('Y'))
        .unwrap_or(false);

    (TriggerData::Manual, name, desc, once)
}

fn parse_net_worth_trigger(parts: &[&str]) -> (TriggerData, String, Option<String>, bool) {
    let name = parts.first().unwrap_or(&"").to_string();
    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let threshold_val = parts
        .get(2)
        .and_then(|s| parse_currency(s).ok())
        .unwrap_or(1000000.0);
    let comparison = parts.get(3).unwrap_or(&">=");
    let once = parts
        .get(4)
        .map(|s| s.to_uppercase().starts_with('Y'))
        .unwrap_or(true);

    let threshold = if comparison.contains("<=") {
        ThresholdData::LessThanOrEqual {
            value: threshold_val,
        }
    } else {
        ThresholdData::GreaterThanOrEqual {
            value: threshold_val,
        }
    };

    (TriggerData::NetWorth { threshold }, name, desc, once)
}

fn parse_repeating_trigger(
    context: &str,
    parts: &[&str],
) -> (TriggerData, String, Option<String>, bool) {
    let interval_str = context.strip_prefix("Repeating|").unwrap_or("Monthly");
    let interval = match interval_str {
        "Weekly" => IntervalData::Weekly,
        "Bi-Weekly" => IntervalData::BiWeekly,
        "Monthly" => IntervalData::Monthly,
        "Quarterly" => IntervalData::Quarterly,
        "Yearly" => IntervalData::Yearly,
        _ => IntervalData::Monthly,
    };

    let name = parts.first().unwrap_or(&"").to_string();
    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    // parts[2] is the read-only interval field
    let start_date = parts.get(3).filter(|s| !s.is_empty());
    let end_age: Option<u8> = parts.get(4).and_then(|s| s.parse().ok());

    let start = start_date.map(|d| {
        Box::new(TriggerData::Date {
            date: d.to_string(),
        })
    });
    let end = end_age.map(|years| {
        Box::new(TriggerData::Age {
            years,
            months: None,
        })
    });

    (
        TriggerData::Repeating {
            interval,
            start,
            end,
        },
        name,
        desc,
        false,
    )
}

fn parse_account_balance_trigger(
    context: &str,
    parts: &[&str],
) -> (TriggerData, String, Option<String>, bool) {
    let account_name = context.strip_prefix("AccountBalance|").unwrap_or("");
    let name = parts.first().unwrap_or(&"").to_string();
    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    // parts[2] is the read-only account field
    let threshold_val = parts
        .get(3)
        .and_then(|s| parse_currency(s).ok())
        .unwrap_or(100000.0);
    let comparison = parts.get(4).unwrap_or(&">=");
    let once = parts
        .get(5)
        .map(|s| s.to_uppercase().starts_with('Y'))
        .unwrap_or(true);

    let threshold = if comparison.contains("<=") {
        ThresholdData::LessThanOrEqual {
            value: threshold_val,
        }
    } else {
        ThresholdData::GreaterThanOrEqual {
            value: threshold_val,
        }
    };

    (
        TriggerData::AccountBalance {
            account: AccountTag(account_name.to_string()),
            threshold,
        },
        name,
        desc,
        once,
    )
}

fn parse_relative_trigger(
    context: &str,
    parts: &[&str],
) -> (TriggerData, String, Option<String>, bool) {
    let event_ref = context.strip_prefix("RelativeToEvent|").unwrap_or("");
    let name = parts.first().unwrap_or(&"").to_string();
    let desc = parts
        .get(1)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    // parts[2] is the read-only event ref field
    let offset_years: i32 = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    let offset_months: i32 = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let once = parts
        .get(5)
        .map(|s| s.to_uppercase().starts_with('Y'))
        .unwrap_or(true);

    let offset = if offset_years != 0 {
        OffsetData::Years {
            value: offset_years,
        }
    } else {
        OffsetData::Months {
            value: offset_months,
        }
    };

    (
        TriggerData::RelativeToEvent {
            event: EventTag(event_ref.to_string()),
            offset,
        },
        name,
        desc,
        once,
    )
}
