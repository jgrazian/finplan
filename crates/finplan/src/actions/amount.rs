//! Amount actions - handling amount editing within effect forms
//!
//! This module handles the editing of recursive AmountData structures within
//! effect forms. When a user activates an Amount field, it launches a type picker
//! and configuration form flow.

use crate::data::events_data::{AccountTag, AmountData};
use crate::modals::amount_builder::AmountTypeOption;
use crate::modals::context::{AmountContext, EffectTypeContext, ModalContext};
use crate::modals::{FormField, FormModal, ModalAction, ModalState, PickerModal};
use crate::screens::events::EventsScreen;
use crate::state::AppState;

use super::{ActionContext, ActionResult};

/// Launch the amount type picker for editing an amount field
pub fn launch_amount_picker(
    _state: &AppState,
    event_idx: usize,
    effect_idx: usize,
    field_idx: usize,
    effect_type: EffectTypeContext,
    _current_amount: &AmountData,
) -> ActionResult {
    ActionResult::modal(ModalState::Picker(
        PickerModal::new(
            "Select Amount Type",
            AmountTypeOption::option_strings(),
            ModalAction::PICK_AMOUNT_TYPE,
        )
        .with_typed_context(ModalContext::Amount(AmountContext::TypePicker {
            event: event_idx,
            effect: effect_idx,
            field_idx,
            effect_type,
        })),
    ))
}

/// Handle amount type selection from picker
pub fn handle_amount_type_pick(
    state: &mut AppState,
    selected: &str,
    ctx: ActionContext,
) -> ActionResult {
    let amount_ctx = ctx.typed_context().and_then(|c| c.as_amount()).cloned();

    let Some(AmountContext::TypePicker {
        event,
        effect,
        field_idx,
        effect_type,
    }) = amount_ctx
    else {
        return ActionResult::close();
    };

    let Some(amount_type) = AmountTypeOption::from_display_name(selected) else {
        return ActionResult::close();
    };

    // Create form based on selected type
    match amount_type {
        AmountTypeOption::Fixed => {
            show_fixed_amount_form(event, effect, field_idx, effect_type, 0.0)
        }
        AmountTypeOption::InflationAdjusted => {
            show_inflation_adjusted_form(state, event, effect, field_idx, effect_type, 0.0)
        }
        AmountTypeOption::Scale => show_scale_form(
            state,
            event,
            effect,
            field_idx,
            effect_type,
            0.04,
            "".to_string(),
        ),
        AmountTypeOption::SourceBalance => {
            // Simple - no form needed, just return the amount
            create_amount_result(state, AmountData::SourceBalance, field_idx)
        }
        AmountTypeOption::ZeroTargetBalance => {
            create_amount_result(state, AmountData::ZeroTargetBalance, field_idx)
        }
        AmountTypeOption::TargetToBalance => {
            show_target_to_balance_form(event, effect, field_idx, effect_type, 0.0)
        }
        AmountTypeOption::AccountBalance => {
            show_account_balance_form(state, event, effect, field_idx, effect_type, "")
        }
        AmountTypeOption::AccountCashBalance => {
            show_account_cash_balance_form(state, event, effect, field_idx, effect_type, "")
        }
    }
}

/// Show form for Fixed amount
fn show_fixed_amount_form(
    event: usize,
    effect: usize,
    field_idx: usize,
    effect_type: EffectTypeContext,
    initial_value: f64,
) -> ActionResult {
    ActionResult::modal(ModalState::Form(
        FormModal::new(
            "Fixed Amount",
            vec![FormField::currency("Amount", initial_value)],
            ModalAction::AMOUNT_FIXED_FORM,
        )
        .with_typed_context(ModalContext::Amount(AmountContext::EffectField {
            event,
            effect,
            field_idx,
            effect_type,
        }))
        .start_editing(),
    ))
}

/// Show form for InflationAdjusted amount
fn show_inflation_adjusted_form(
    _state: &AppState,
    event: usize,
    effect: usize,
    field_idx: usize,
    effect_type: EffectTypeContext,
    initial_value: f64,
) -> ActionResult {
    // For simplicity, inflation-adjusted wraps a fixed amount
    // Users can edit the base amount
    ActionResult::modal(ModalState::Form(
        FormModal::new(
            "Inflation-Adjusted Amount",
            vec![
                FormField::currency("Base Amount (today's $)", initial_value),
                FormField::read_only("Note", "Amount will adjust for inflation over time"),
            ],
            ModalAction::AMOUNT_INFLATION_FORM,
        )
        .with_typed_context(ModalContext::Amount(AmountContext::EffectField {
            event,
            effect,
            field_idx,
            effect_type,
        }))
        .start_editing(),
    ))
}

/// Show form for Scale (percentage) amount
fn show_scale_form(
    state: &AppState,
    event: usize,
    effect: usize,
    field_idx: usize,
    effect_type: EffectTypeContext,
    initial_multiplier: f64,
    initial_account: String,
) -> ActionResult {
    let accounts = EventsScreen::get_account_names(state);

    // Default to first account if none specified
    let selected_account = if initial_account.is_empty() {
        accounts.first().cloned().unwrap_or_default()
    } else {
        initial_account
    };

    ActionResult::modal(ModalState::Form(
        FormModal::new(
            "Percentage of Account Balance",
            vec![
                FormField::percentage("Percentage", initial_multiplier),
                FormField::select("Of Account", accounts, &selected_account),
            ],
            ModalAction::AMOUNT_SCALE_FORM,
        )
        .with_typed_context(ModalContext::Amount(AmountContext::EffectField {
            event,
            effect,
            field_idx,
            effect_type,
        }))
        .start_editing(),
    ))
}

/// Show form for TargetToBalance amount
fn show_target_to_balance_form(
    event: usize,
    effect: usize,
    field_idx: usize,
    effect_type: EffectTypeContext,
    initial_target: f64,
) -> ActionResult {
    ActionResult::modal(ModalState::Form(
        FormModal::new(
            "Target Balance",
            vec![FormField::currency("Target Balance", initial_target)],
            ModalAction::AMOUNT_TARGET_FORM,
        )
        .with_typed_context(ModalContext::Amount(AmountContext::EffectField {
            event,
            effect,
            field_idx,
            effect_type,
        }))
        .start_editing(),
    ))
}

/// Show form for AccountBalance amount
fn show_account_balance_form(
    state: &AppState,
    event: usize,
    effect: usize,
    field_idx: usize,
    effect_type: EffectTypeContext,
    initial_account: &str,
) -> ActionResult {
    let accounts = EventsScreen::get_account_names(state);
    let selected = if initial_account.is_empty() {
        accounts.first().cloned().unwrap_or_default()
    } else {
        initial_account.to_string()
    };

    ActionResult::modal(ModalState::Form(
        FormModal::new(
            "Account Balance Reference",
            vec![FormField::select("Account", accounts, &selected)],
            ModalAction::AMOUNT_ACCOUNT_BALANCE_FORM,
        )
        .with_typed_context(ModalContext::Amount(AmountContext::EffectField {
            event,
            effect,
            field_idx,
            effect_type,
        }))
        .start_editing(),
    ))
}

/// Show form for AccountCashBalance amount
fn show_account_cash_balance_form(
    state: &AppState,
    event: usize,
    effect: usize,
    field_idx: usize,
    effect_type: EffectTypeContext,
    initial_account: &str,
) -> ActionResult {
    let accounts = EventsScreen::get_account_names(state);
    let selected = if initial_account.is_empty() {
        accounts.first().cloned().unwrap_or_default()
    } else {
        initial_account.to_string()
    };

    ActionResult::modal(ModalState::Form(
        FormModal::new(
            "Account Cash Balance Reference",
            vec![FormField::select("Account", accounts, &selected)],
            ModalAction::AMOUNT_CASH_BALANCE_FORM,
        )
        .with_typed_context(ModalContext::Amount(AmountContext::EffectField {
            event,
            effect,
            field_idx,
            effect_type,
        }))
        .start_editing(),
    ))
}

/// Handle Fixed amount form submission
pub fn handle_fixed_amount_form(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let amount_ctx = ctx.typed_context().and_then(|c| c.as_amount()).cloned();

    let Some(AmountContext::EffectField { field_idx, .. }) = amount_ctx else {
        return ActionResult::close();
    };

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::close(),
    };

    let value = form.get_currency(0).unwrap_or(0.0);
    let amount = AmountData::fixed(value);

    create_amount_result(state, amount, field_idx)
}

/// Handle InflationAdjusted amount form submission
pub fn handle_inflation_form(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let amount_ctx = ctx.typed_context().and_then(|c| c.as_amount()).cloned();

    let Some(AmountContext::EffectField { field_idx, .. }) = amount_ctx else {
        return ActionResult::close();
    };

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::close(),
    };

    let base_value = form.get_currency(0).unwrap_or(0.0);
    let amount = AmountData::inflation_adjusted(AmountData::fixed(base_value));

    create_amount_result(state, amount, field_idx)
}

/// Handle Scale (percentage) amount form submission
pub fn handle_scale_form(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let amount_ctx = ctx.typed_context().and_then(|c| c.as_amount()).cloned();

    let Some(AmountContext::EffectField { field_idx, .. }) = amount_ctx else {
        return ActionResult::close();
    };

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::close(),
    };

    let multiplier = form.get_percentage(0).unwrap_or(0.04);
    let account = form.get_str(1).unwrap_or("").to_string();

    let amount = AmountData::scale(
        multiplier,
        AmountData::AccountBalance {
            account: AccountTag(account),
        },
    );

    create_amount_result(state, amount, field_idx)
}

/// Handle TargetToBalance form submission
pub fn handle_target_form(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let amount_ctx = ctx.typed_context().and_then(|c| c.as_amount()).cloned();

    let Some(AmountContext::EffectField { field_idx, .. }) = amount_ctx else {
        return ActionResult::close();
    };

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::close(),
    };

    let target = form.get_currency(0).unwrap_or(0.0);
    let amount = AmountData::TargetToBalance { target };

    create_amount_result(state, amount, field_idx)
}

/// Handle AccountBalance form submission
pub fn handle_account_balance_form(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let amount_ctx = ctx.typed_context().and_then(|c| c.as_amount()).cloned();

    let Some(AmountContext::EffectField { field_idx, .. }) = amount_ctx else {
        return ActionResult::close();
    };

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::close(),
    };

    let account = form.get_str(0).unwrap_or("").to_string();
    let amount = AmountData::AccountBalance {
        account: AccountTag(account),
    };

    create_amount_result(state, amount, field_idx)
}

/// Handle AccountCashBalance form submission
pub fn handle_cash_balance_form(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let amount_ctx = ctx.typed_context().and_then(|c| c.as_amount()).cloned();

    let Some(AmountContext::EffectField { field_idx, .. }) = amount_ctx else {
        return ActionResult::close();
    };

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::close(),
    };

    let account = form.get_str(0).unwrap_or("").to_string();
    let amount = AmountData::AccountCashBalance {
        account: AccountTag(account),
    };

    create_amount_result(state, amount, field_idx)
}

/// Create the result that returns the amount to the effect form.
/// Takes the pending effect form from state, updates the amount field, and returns it.
fn create_amount_result(
    state: &mut AppState,
    amount: AmountData,
    field_idx: usize,
) -> ActionResult {
    // Take the pending form from state
    let Some(mut form) = state.pending_effect_form.take() else {
        // No pending form - just close
        return ActionResult::close();
    };

    // Update the amount field at the specified index
    if let Some(field) = form.fields.get_mut(field_idx) {
        field.set_amount(amount);
    }

    // Return to the effect form with the updated amount
    ActionResult::modal(ModalState::Form(form))
}
