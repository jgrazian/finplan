// Event actions - trigger type picking, CRUD operations

use crate::data::events_data::{
    AccountTag, EventData, EventTag, IntervalData, OffsetData, ThresholdData, TriggerData,
};
use crate::modals::{
    FormField, FormModal, ModalAction, ModalContext, ModalState, PickerModal,
    context::{PartialTrigger, TriggerBuilderState, TriggerChildSlot, TriggerContext},
};
use crate::screens::events::EventsScreen;
use crate::state::AppState;
use crate::util::common::yes_no_options;

use super::{ActionContext, ActionResult};

fn balance_comparison_options() -> Vec<String> {
    vec![
        "Balance drops to or below".to_string(),
        "Balance rises to or above".to_string(),
    ]
}

/// Handle trigger type selection - shows appropriate form or picker
pub fn handle_trigger_type_pick(state: &AppState, trigger_type: &str) -> ActionResult {
    match trigger_type {
        "Date" => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "New Event - Date Trigger",
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::text("Date (YYYY-MM-DD)", "2025-01-01"),
                    FormField::select("Once Only", yes_no_options(), "No"),
                ],
                ModalAction::CREATE_EVENT,
            )
            .with_typed_context(ModalContext::Trigger(TriggerContext::Date))
            .start_editing(),
        )),
        "Age" => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "New Event - Age Trigger",
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::text("Age (years)", "65"),
                    FormField::select("Once Only", yes_no_options(), "Yes"),
                ],
                ModalAction::CREATE_EVENT,
            )
            .with_typed_context(ModalContext::Trigger(TriggerContext::Age))
            .start_editing(),
        )),
        "Repeating" => {
            // Show interval picker first
            let intervals = vec![
                "Weekly".to_string(),
                "Bi-Weekly".to_string(),
                "Monthly".to_string(),
                "Quarterly".to_string(),
                "Yearly".to_string(),
            ];
            ActionResult::modal(ModalState::Picker(PickerModal::new(
                "Select Repeat Interval",
                intervals,
                ModalAction::PICK_INTERVAL,
            )))
        }
        "Manual" => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "New Event - Manual Trigger",
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::select("Once Only", yes_no_options(), "No"),
                ],
                ModalAction::CREATE_EVENT,
            )
            .with_typed_context(ModalContext::Trigger(TriggerContext::Manual))
            .start_editing(),
        )),
        "Account Balance" => {
            // Get account list
            let accounts = EventsScreen::get_account_names(state);
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Picker(PickerModal::new(
                "Select Account for Balance Trigger",
                accounts,
                ModalAction::PICK_ACCOUNT_FOR_EFFECT,
            )))
        }
        "Net Worth" => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "New Event - Net Worth Trigger",
                vec![
                    FormField::text("Event Name", ""),
                    FormField::text("Description", ""),
                    FormField::currency("Threshold", 1000000.0),
                    FormField::select(
                        "Trigger When",
                        balance_comparison_options(),
                        "Balance rises to or above",
                    ),
                    FormField::select("Once Only", yes_no_options(), "Yes"),
                ],
                ModalAction::CREATE_EVENT,
            )
            .with_typed_context(ModalContext::Trigger(TriggerContext::NetWorth))
            .start_editing(),
        )),
        "Relative to Event" => {
            // Get event list
            let events = EventsScreen::get_event_names(state);
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            ActionResult::modal(ModalState::Picker(PickerModal::new(
                "Select Reference Event",
                events,
                ModalAction::PICK_EVENT_REFERENCE,
            )))
        }
        "Quick Events" => {
            // Show quick event template picker
            let templates = vec![
                "Social Security".to_string(),
                "RMD (Required Minimum Distributions)".to_string(),
                "Medicare Part B".to_string(),
            ];
            ActionResult::modal(ModalState::Picker(PickerModal::new(
                "Select Quick Event",
                templates,
                ModalAction::PICK_QUICK_EVENT,
            )))
        }
        _ => ActionResult::close(),
    }
}

/// Handle interval selection for repeating events
/// Creates a TriggerBuilderState and shows the start condition type picker
pub fn handle_interval_pick(interval: &str) -> ActionResult {
    let interval_data = match interval {
        "Weekly" => IntervalData::Weekly,
        "Bi-Weekly" => IntervalData::BiWeekly,
        "Monthly" => IntervalData::Monthly,
        "Quarterly" => IntervalData::Quarterly,
        "Yearly" => IntervalData::Yearly,
        _ => IntervalData::Monthly,
    };

    let builder = TriggerBuilderState::new_repeating(interval_data);

    // Show start condition type picker
    show_child_trigger_type_picker(builder, TriggerChildSlot::Start)
}

/// Show the picker for selecting a child trigger type (start or end condition)
fn show_child_trigger_type_picker(
    builder: TriggerBuilderState,
    slot: TriggerChildSlot,
) -> ActionResult {
    let title = match slot {
        TriggerChildSlot::Start => "Select Start Condition",
        TriggerChildSlot::End => "Select End Condition",
    };

    let none_option = match slot {
        TriggerChildSlot::Start => "None (Start Immediately)",
        TriggerChildSlot::End => "None (Run Forever)",
    };

    let options = vec![
        none_option.to_string(),
        "Date".to_string(),
        "Age".to_string(),
        "Account Balance".to_string(),
        "Net Worth".to_string(),
        "Relative to Event".to_string(),
    ];

    ActionResult::modal(ModalState::Picker(
        PickerModal::new(title, options, ModalAction::PICK_CHILD_TRIGGER_TYPE).with_typed_context(
            ModalContext::Trigger(TriggerContext::RepeatingBuilder(builder)),
        ),
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
                FormField::select(
                    "Trigger When",
                    balance_comparison_options(),
                    "Balance drops to or below",
                ),
                FormField::select("Once Only", yes_no_options(), "Yes"),
            ],
            ModalAction::CREATE_EVENT,
        )
        .with_typed_context(ModalContext::Trigger(TriggerContext::AccountBalance(
            account.to_string(),
        )))
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
                FormField::select("Once Only", yes_no_options(), "Yes"),
            ],
            ModalAction::CREATE_EVENT,
        )
        .with_typed_context(ModalContext::Trigger(TriggerContext::RelativeToEvent(
            event_ref.to_string(),
        )))
        .start_editing(),
    ))
}

/// Handle event creation
pub fn handle_create_event(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::close(),
    };

    // Get typed trigger context
    let trigger_ctx = ctx.typed_context().and_then(|c| c.as_trigger()).cloned();

    // Parse trigger type and create appropriate event
    let (trigger, name, description, once) = match trigger_ctx {
        Some(TriggerContext::Date) => parse_date_trigger(form),
        Some(TriggerContext::Age) => parse_age_trigger(form),
        Some(TriggerContext::Manual) => parse_manual_trigger(form),
        Some(TriggerContext::NetWorth) => parse_net_worth_trigger(form),
        Some(TriggerContext::AccountBalance(account)) => {
            parse_account_balance_trigger_typed(&account, form)
        }
        Some(TriggerContext::RelativeToEvent(event)) => parse_relative_trigger_typed(&event, form),
        Some(TriggerContext::Repeating(_)) | Some(TriggerContext::RepeatingBuilder(_)) => {
            // Repeating events use the separate finalize flow
            return ActionResult::close();
        }
        None => return ActionResult::close(),
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

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::close(),
    };

    if let Some(event) = state.data_mut().events.get_mut(idx) {
        // Fields: [name, description, once, enabled, trigger (ro), effects (ro)]
        if let Some(name) = form.get_str_non_empty(0) {
            event.name = EventTag(name.to_string());
        }
        event.description = form.get_optional_str(1);
        event.once = form.get_bool_or(2, false);
        event.enabled = form.get_bool_or(3, true);
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

fn parse_date_trigger(form: &FormModal) -> (TriggerData, String, Option<String>, bool) {
    let name = form.get_str(0).unwrap_or("").to_string();
    let desc = form.get_optional_str(1);
    let date = form.get_str(2).unwrap_or("2025-01-01").to_string();
    let once = form.get_bool_or(3, false);

    (TriggerData::Date { date }, name, desc, once)
}

fn parse_age_trigger(form: &FormModal) -> (TriggerData, String, Option<String>, bool) {
    let name = form.get_str(0).unwrap_or("").to_string();
    let desc = form.get_optional_str(1);
    let years: u8 = form.get_int_or(2, 65);
    let once = form.get_bool_or(3, true);

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

fn parse_manual_trigger(form: &FormModal) -> (TriggerData, String, Option<String>, bool) {
    let name = form.get_str(0).unwrap_or("").to_string();
    let desc = form.get_optional_str(1);
    let once = form.get_bool_or(2, false);

    (TriggerData::Manual, name, desc, once)
}

fn parse_net_worth_trigger(form: &FormModal) -> (TriggerData, String, Option<String>, bool) {
    let name = form.get_str(0).unwrap_or("").to_string();
    let desc = form.get_optional_str(1);
    let threshold_val = form.get_currency_or(2, 1000000.0);
    let comparison = form.get_str(3).unwrap_or("Balance rises to or above");
    let once = form.get_bool_or(4, true);

    // "Balance drops to or below" → LessThanOrEqual
    // "Balance rises to or above" → GreaterThanOrEqual
    let threshold = if comparison.contains("drops") || comparison.contains("<=") {
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

/// Parse account balance trigger with typed context
fn parse_account_balance_trigger_typed(
    account_name: &str,
    form: &FormModal,
) -> (TriggerData, String, Option<String>, bool) {
    let name = form.get_str(0).unwrap_or("").to_string();
    let desc = form.get_optional_str(1);
    // form field 2 is the read-only account field
    let threshold_val = form.get_currency_or(3, 100000.0);
    let comparison = form.get_str(4).unwrap_or("Balance drops to or below");
    let once = form.get_bool_or(5, true);

    let threshold = if comparison.contains("drops") || comparison.contains("<=") {
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

/// Parse relative trigger with typed context
fn parse_relative_trigger_typed(
    event_ref: &str,
    form: &FormModal,
) -> (TriggerData, String, Option<String>, bool) {
    let name = form.get_str(0).unwrap_or("").to_string();
    let desc = form.get_optional_str(1);
    // form field 2 is the read-only event ref field
    let offset_years: i32 = form.get_int_or(3, 0);
    let offset_months: i32 = form.get_int_or(4, 0);
    let once = form.get_bool_or(5, true);

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

// ========== Trigger Builder Handlers ==========

/// Handle child trigger type selection for repeating events
pub fn handle_pick_child_trigger_type(
    state: &AppState,
    trigger_type: &str,
    ctx: ActionContext,
) -> ActionResult {
    let builder = match ctx.trigger_builder() {
        Some(b) => b.clone(),
        None => return ActionResult::error("Missing trigger builder context"),
    };

    // Determine which slot we're building based on builder state
    let slot = builder.current_phase().unwrap_or(TriggerChildSlot::Start);

    match trigger_type {
        "None (Start Immediately)" | "None (Run Forever)" => {
            // Skip this condition, move to next phase
            handle_none_trigger(builder, slot)
        }
        "Date" => {
            show_child_trigger_form(state, builder, slot, PartialTrigger::Date { date: None })
        }
        "Age" => show_child_trigger_form(
            state,
            builder,
            slot,
            PartialTrigger::Age {
                years: None,
                months: None,
            },
        ),
        "Account Balance" => {
            // Need to pick account first
            let accounts = EventsScreen::get_account_names(state);
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            // Store the builder and slot info for the next step
            let mut new_builder = builder;
            new_builder.push_child(
                slot,
                PartialTrigger::AccountBalance {
                    account: String::new(),
                    threshold: None,
                    comparison: None,
                },
            );
            ActionResult::modal(ModalState::Picker(
                PickerModal::new("Select Account", accounts, ModalAction::BUILD_CHILD_TRIGGER)
                    .with_typed_context(ModalContext::Trigger(TriggerContext::RepeatingBuilder(
                        new_builder,
                    ))),
            ))
        }
        "Net Worth" => show_child_trigger_form(
            state,
            builder,
            slot,
            PartialTrigger::NetWorth {
                threshold: None,
                comparison: None,
            },
        ),
        "Relative to Event" => {
            // Need to pick event first
            let events = EventsScreen::get_event_names(state);
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            let mut new_builder = builder;
            new_builder.push_child(
                slot,
                PartialTrigger::RelativeToEvent {
                    event: String::new(),
                    offset_years: None,
                    offset_months: None,
                },
            );
            ActionResult::modal(ModalState::Picker(
                PickerModal::new("Select Event", events, ModalAction::BUILD_CHILD_TRIGGER)
                    .with_typed_context(ModalContext::Trigger(TriggerContext::RepeatingBuilder(
                        new_builder,
                    ))),
            ))
        }
        _ => ActionResult::close(),
    }
}

/// Handle "None" selection for start/end condition
fn handle_none_trigger(mut builder: TriggerBuilderState, slot: TriggerChildSlot) -> ActionResult {
    // Set the slot to None explicitly in the current trigger
    if let PartialTrigger::Repeating { start, end, .. } = &mut builder.current {
        match slot {
            TriggerChildSlot::Start => *start = Some(Box::new(PartialTrigger::None)),
            TriggerChildSlot::End => *end = Some(Box::new(PartialTrigger::None)),
        }
    }

    match slot {
        TriggerChildSlot::Start => {
            // Move to end condition picker
            show_child_trigger_type_picker(builder, TriggerChildSlot::End)
        }
        TriggerChildSlot::End => {
            // Move to finalize form
            show_finalize_form(builder)
        }
    }
}

/// Show the appropriate form for a child trigger type
fn show_child_trigger_form(
    _state: &AppState,
    mut builder: TriggerBuilderState,
    slot: TriggerChildSlot,
    partial: PartialTrigger,
) -> ActionResult {
    // Push the partial trigger as the current context
    builder.push_child(slot, partial.clone());

    let (title, fields) = match &partial {
        PartialTrigger::Date { .. } => {
            let title = match slot {
                TriggerChildSlot::Start => "Start Condition - Date",
                TriggerChildSlot::End => "End Condition - Date",
            };
            let fields = vec![FormField::text("Date (YYYY-MM-DD)", "2025-01-01")];
            (title, fields)
        }
        PartialTrigger::Age { .. } => {
            let title = match slot {
                TriggerChildSlot::Start => "Start Condition - Age",
                TriggerChildSlot::End => "End Condition - Age",
            };
            let fields = vec![
                FormField::text("Age (years)", "65"),
                FormField::text("Months (optional)", ""),
            ];
            (title, fields)
        }
        PartialTrigger::NetWorth { .. } => {
            let title = match slot {
                TriggerChildSlot::Start => "Start Condition - Net Worth",
                TriggerChildSlot::End => "End Condition - Net Worth",
            };
            let fields = vec![
                FormField::currency("Threshold", 1000000.0),
                FormField::select(
                    "Trigger When",
                    balance_comparison_options(),
                    "Balance rises to or above",
                ),
            ];
            (title, fields)
        }
        _ => return ActionResult::error("Unsupported trigger type for form"),
    };

    ActionResult::modal(ModalState::Form(
        FormModal::new(title, fields, ModalAction::COMPLETE_CHILD_TRIGGER)
            .with_typed_context(ModalContext::Trigger(TriggerContext::RepeatingBuilder(
                builder,
            )))
            .start_editing(),
    ))
}

/// Handle building child trigger after selecting account or event
pub fn handle_build_child_trigger(
    _state: &AppState,
    selected: &str,
    ctx: ActionContext,
) -> ActionResult {
    let mut builder = match ctx.trigger_builder() {
        Some(b) => b.clone(),
        None => return ActionResult::error("Missing trigger builder context"),
    };

    // Update the current partial trigger with the selected value
    match &mut builder.current {
        PartialTrigger::AccountBalance { account, .. } => {
            *account = selected.to_string();
            // Now show the threshold form
            let fields = vec![
                FormField::read_only("Account", selected),
                FormField::currency("Threshold", 100000.0),
                FormField::select(
                    "Trigger When",
                    balance_comparison_options(),
                    "Balance drops to or below",
                ),
            ];
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "Balance Trigger",
                    fields,
                    ModalAction::COMPLETE_CHILD_TRIGGER,
                )
                .with_typed_context(ModalContext::Trigger(TriggerContext::RepeatingBuilder(
                    builder,
                )))
                .start_editing(),
            ))
        }
        PartialTrigger::RelativeToEvent { event, .. } => {
            *event = selected.to_string();
            // Now show the offset form
            let fields = vec![
                FormField::read_only("Reference Event", selected),
                FormField::text("Offset Years", "0"),
                FormField::text("Offset Months", "0"),
            ];
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "Relative to Event",
                    fields,
                    ModalAction::COMPLETE_CHILD_TRIGGER,
                )
                .with_typed_context(ModalContext::Trigger(TriggerContext::RepeatingBuilder(
                    builder,
                )))
                .start_editing(),
            ))
        }
        _ => ActionResult::error("Unexpected trigger type in build_child_trigger"),
    }
}

/// Handle completing a child trigger (form submission)
pub fn handle_complete_child_trigger(_state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let mut builder = match ctx.trigger_builder() {
        Some(b) => b.clone(),
        None => return ActionResult::error("Missing trigger builder context"),
    };

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::error("Missing form data"),
    };

    // Determine which slot we were building
    let was_start = builder
        .parent_stack
        .last()
        .map(|(_, slot)| *slot == TriggerChildSlot::Start)
        .unwrap_or(false);

    // Update the current partial trigger with form values
    match &mut builder.current {
        PartialTrigger::Date { date } => {
            *date = form.get_str(0).map(|s| s.to_string());
        }
        PartialTrigger::Age { years, months } => {
            *years = form.get_int(0);
            *months = form.get_int(1);
        }
        PartialTrigger::NetWorth {
            threshold,
            comparison,
        } => {
            *threshold = form.get_currency(0);
            *comparison = form.get_str(1).map(|s| s.to_string());
        }
        PartialTrigger::AccountBalance {
            threshold,
            comparison,
            ..
        } => {
            // Skip field 0 (read-only account name)
            *threshold = form.get_currency(1);
            *comparison = form.get_str(2).map(|s| s.to_string());
        }
        PartialTrigger::RelativeToEvent {
            offset_years,
            offset_months,
            ..
        } => {
            // Skip field 0 (read-only event name)
            *offset_years = form.get_int(1);
            *offset_months = form.get_int(2);
        }
        _ => {}
    }

    // Pop back to parent
    builder.pop_to_parent();

    if was_start {
        // Move to end condition picker
        show_child_trigger_type_picker(builder, TriggerChildSlot::End)
    } else {
        // Move to finalize form
        show_finalize_form(builder)
    }
}

/// Show the final form for entering event name and description
fn show_finalize_form(builder: TriggerBuilderState) -> ActionResult {
    // Get interval display name
    let interval_name = if let PartialTrigger::Repeating { interval, .. } = &builder.current {
        match interval {
            IntervalData::Never => "Never",
            IntervalData::Weekly => "Weekly",
            IntervalData::BiWeekly => "Bi-Weekly",
            IntervalData::Monthly => "Monthly",
            IntervalData::Quarterly => "Quarterly",
            IntervalData::Yearly => "Yearly",
        }
    } else {
        "Repeating"
    };

    let title = format!("New {} Repeating Event", interval_name);
    let fields = vec![
        FormField::text("Event Name", ""),
        FormField::text("Description", ""),
    ];

    ActionResult::modal(ModalState::Form(
        FormModal::new(&title, fields, ModalAction::FINALIZE_REPEATING)
            .with_typed_context(ModalContext::Trigger(TriggerContext::RepeatingBuilder(
                builder,
            )))
            .start_editing(),
    ))
}

/// Handle finalizing a repeating event (create the actual event)
pub fn handle_finalize_repeating(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let builder = match ctx.trigger_builder() {
        Some(b) => b.clone(),
        None => return ActionResult::error("Missing trigger builder context"),
    };

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::error("Missing form data"),
    };

    let name = form.get_str(0).unwrap_or("").to_string();
    let description = form.get_optional_str(1);

    if name.is_empty() {
        return ActionResult::error("Event name cannot be empty");
    }

    // Convert the builder state to TriggerData
    let trigger = match convert_partial_to_trigger(&builder.current) {
        Some(t) => t,
        None => return ActionResult::error("Failed to build trigger"),
    };

    let event = EventData {
        name: EventTag(name),
        description,
        trigger,
        effects: vec![],
        once: false,
        enabled: true,
    };

    state.data_mut().events.push(event);
    state.events_state.selected_event_index = state.data().events.len() - 1;
    ActionResult::modified()
}

/// Convert a PartialTrigger to TriggerData
fn convert_partial_to_trigger(partial: &PartialTrigger) -> Option<TriggerData> {
    match partial {
        PartialTrigger::None => None,
        PartialTrigger::Date { date } => Some(TriggerData::Date {
            date: date.clone().unwrap_or_else(|| "2025-01-01".to_string()),
        }),
        PartialTrigger::Age { years, months } => Some(TriggerData::Age {
            years: years.unwrap_or(65),
            months: *months,
        }),
        PartialTrigger::Manual => Some(TriggerData::Manual),
        PartialTrigger::NetWorth {
            threshold,
            comparison,
        } => {
            let threshold_val = threshold.unwrap_or(1000000.0);
            let comp = comparison.as_deref().unwrap_or("");
            let threshold_data = if comp.contains("drops") || comp.contains("<=") {
                ThresholdData::LessThanOrEqual {
                    value: threshold_val,
                }
            } else {
                ThresholdData::GreaterThanOrEqual {
                    value: threshold_val,
                }
            };
            Some(TriggerData::NetWorth {
                threshold: threshold_data,
            })
        }
        PartialTrigger::AccountBalance {
            account,
            threshold,
            comparison,
        } => {
            let threshold_val = threshold.unwrap_or(100000.0);
            let comp = comparison.as_deref().unwrap_or("");
            let threshold_data = if comp.contains("drops") || comp.contains("<=") {
                ThresholdData::LessThanOrEqual {
                    value: threshold_val,
                }
            } else {
                ThresholdData::GreaterThanOrEqual {
                    value: threshold_val,
                }
            };
            Some(TriggerData::AccountBalance {
                account: AccountTag(account.clone()),
                threshold: threshold_data,
            })
        }
        PartialTrigger::RelativeToEvent {
            event,
            offset_years,
            offset_months,
        } => {
            let offset = if offset_years.unwrap_or(0) != 0 {
                OffsetData::Years {
                    value: offset_years.unwrap_or(0),
                }
            } else {
                OffsetData::Months {
                    value: offset_months.unwrap_or(0),
                }
            };
            Some(TriggerData::RelativeToEvent {
                event: EventTag(event.clone()),
                offset,
            })
        }
        PartialTrigger::Repeating {
            interval,
            start,
            end,
        } => {
            let start_trigger = start
                .as_ref()
                .and_then(|p| convert_partial_to_trigger(p))
                .map(Box::new);
            let end_trigger = end
                .as_ref()
                .and_then(|p| convert_partial_to_trigger(p))
                .map(Box::new);
            Some(TriggerData::Repeating {
                interval: *interval,
                start: start_trigger,
                end: end_trigger,
            })
        }
    }
}

// =============================================================================
// Quick Event Templates
// =============================================================================

use crate::data::events_data::{AmountData, EffectData, LotMethodData};
use crate::data::portfolio_data::AccountType;

/// Handle quick event template selection
pub fn handle_quick_event_pick(state: &mut AppState, template: &str) -> ActionResult {
    let event = match template {
        "Social Security" => create_social_security_template(state),
        "RMD (Required Minimum Distributions)" => create_rmd_template(state),
        "Medicare Part B" => create_medicare_template(state),
        _ => return ActionResult::close(),
    };

    // Add event and select it
    state.data_mut().events.push(event);
    state.events_state.selected_event_index = state.data().events.len() - 1;
    state.mark_modified();

    ActionResult::modified()
}

/// Find the first cash account (Checking or Savings) for use as default destination
fn first_cash_account_name(state: &AppState) -> String {
    state
        .data()
        .portfolios
        .accounts
        .iter()
        .find(|a| {
            matches!(
                a.account_type,
                AccountType::Checking(_) | AccountType::Savings(_)
            )
        })
        .map(|a| a.name.clone())
        .unwrap_or_else(|| "Checking".to_string())
}

/// Create a Social Security template event
fn create_social_security_template(state: &AppState) -> EventData {
    let dest = first_cash_account_name(state);
    EventData {
        name: EventTag("Social Security".to_string()),
        description: Some("Monthly Social Security benefits".to_string()),
        trigger: TriggerData::Repeating {
            interval: IntervalData::Monthly,
            start: Some(Box::new(TriggerData::Age {
                years: 67,
                months: None,
            })),
            end: None,
        },
        effects: vec![EffectData::Income {
            to: AccountTag(dest),
            amount: AmountData::fixed(2000.0), // Placeholder - user should customize
            gross: true,
            taxable: true, // SS is partially taxable at higher incomes
        }],
        once: false,
        enabled: true,
    }
}

/// Create an RMD (Required Minimum Distributions) template event
fn create_rmd_template(state: &AppState) -> EventData {
    let dest = first_cash_account_name(state);
    EventData {
        name: EventTag("RMD".to_string()),
        description: Some("Required Minimum Distributions from tax-deferred accounts".to_string()),
        trigger: TriggerData::Repeating {
            interval: IntervalData::Yearly,
            start: Some(Box::new(TriggerData::Age {
                years: 73,
                months: None,
            })),
            end: None,
        },
        effects: vec![EffectData::ApplyRmd {
            destination: AccountTag(dest),
            lot_method: LotMethodData::Fifo,
        }],
        once: false,
        enabled: true,
    }
}

/// Create a Medicare Part B template event
fn create_medicare_template(state: &AppState) -> EventData {
    let source = first_cash_account_name(state);
    EventData {
        name: EventTag("Medicare Part B".to_string()),
        description: Some("Medicare Part B monthly premiums".to_string()),
        trigger: TriggerData::Repeating {
            interval: IntervalData::Monthly,
            start: Some(Box::new(TriggerData::Age {
                years: 65,
                months: None,
            })),
            end: None,
        },
        effects: vec![EffectData::Expense {
            from: AccountTag(source),
            amount: AmountData::fixed(174.70), // 2024 standard Part B premium
        }],
        once: false,
        enabled: true,
    }
}
