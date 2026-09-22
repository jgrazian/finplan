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
            show_fixed_amount_form(state, event, effect, field_idx, effect_type, false)
        }
        AmountTypeOption::InflationAdjusted => {
            show_fixed_amount_form(state, event, effect, field_idx, effect_type, true)
        }
        AmountTypeOption::Scale => show_scale_form(state, event, effect, field_idx, effect_type),
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

/// Edit a money value or a reference, optionally adjusted for inflation.
fn show_fixed_amount_form(
    state: &AppState,
    event: usize,
    effect: usize,
    field_idx: usize,
    effect_type: EffectTypeContext,
    inflation: bool,
) -> ActionResult {
    let current = state
        .pending_effect_form
        .as_ref()
        .and_then(|form| form.get_amount(field_idx));
    let current = match current.as_ref() {
        Some(AmountData::InflationAdjusted { inner }) => Some(inner.as_ref()),
        amount => amount,
    };
    let (value, reference) = match current {
        Some(AmountData::Fixed { value }) => (*value, None),
        Some(AmountData::Parameter { name }) => (0.0, Some(name.as_str())),
        _ => (0.0, None),
    };
    ActionResult::modal(ModalState::Form(
        FormModal::new(
            if inflation {
                "Inflation-Adjusted Amount"
            } else {
                "Amount"
            },
            super::value_input::fields(
                state,
                finplan_core::model::ParameterValue::Money(value),
                reference,
            ),
            if inflation {
                ModalAction::AMOUNT_INFLATION_FORM
            } else {
                ModalAction::AMOUNT_FIXED_FORM
            },
        )
        .with_kind(crate::modals::FormKind::ValueInput {
            source: 0,
            kind: crate::modals::ParameterKind::Money,
        })
        .with_typed_context(ModalContext::Amount(AmountContext::EffectField {
            event,
            effect,
            field_idx,
            effect_type,
        })),
    ))
}

/// Show form for Scale (percentage) amount
fn show_scale_form(
    state: &AppState,
    event: usize,
    effect: usize,
    field_idx: usize,
    effect_type: EffectTypeContext,
) -> ActionResult {
    let accounts = EventsScreen::get_account_names(state);
    let current = state
        .pending_effect_form
        .as_ref()
        .and_then(|form| form.get_amount(field_idx));
    let (multiplier, reference, inner) = match current.as_ref() {
        Some(AmountData::Scale { multiplier, inner }) => (*multiplier, None, Some(inner.as_ref())),
        Some(AmountData::RateTimes { rate, inner }) => {
            (0.04, Some(rate.as_str()), Some(inner.as_ref()))
        }
        _ => (0.04, None, None),
    };
    let selected_account = match inner {
        Some(AmountData::AccountBalance { account }) => account.0.clone(),
        _ => accounts.first().cloned().unwrap_or_default(),
    };
    let mut fields = super::value_input::fields(
        state,
        finplan_core::model::ParameterValue::Rate(multiplier),
        reference,
    );
    fields.push(FormField::select("Of Account", accounts, &selected_account));

    ActionResult::modal(ModalState::Form(
        FormModal::new(
            "Percentage of Account Balance",
            fields,
            ModalAction::AMOUNT_SCALE_FORM,
        )
        .with_kind(crate::modals::FormKind::ValueInput {
            source: 0,
            kind: crate::modals::ParameterKind::Rate,
        })
        .with_typed_context(ModalContext::Amount(AmountContext::EffectField {
            event,
            effect,
            field_idx,
            effect_type,
        })),
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

/// Handle an entered money value or a Money parameter reference.
pub fn handle_fixed_amount_form(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    handle_money_value(state, ctx, false)
}

pub fn handle_inflation_form(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    handle_money_value(state, ctx, true)
}

fn handle_money_value(state: &mut AppState, ctx: ActionContext, inflation: bool) -> ActionResult {
    let Some(AmountContext::EffectField { field_idx, .. }) =
        ctx.typed_context().and_then(|c| c.as_amount())
    else {
        return ActionResult::close();
    };
    let Some(form) = ctx.form() else {
        return ActionResult::close();
    };
    let mut amount =
        match super::value_input::read(state, form, 0, crate::modals::ParameterKind::Money) {
            Ok(super::value_input::ValueInput::Entered(
                finplan_core::model::ParameterValue::Money(value),
            )) => AmountData::fixed(value),
            Ok(super::value_input::ValueInput::Parameter(name)) => AmountData::Parameter { name },
            Err(error) => return ActionResult::error(error),
            _ => unreachable!(),
        };
    if inflation {
        amount = AmountData::inflation_adjusted(amount);
    }
    create_amount_result(state, amount, *field_idx)
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

    let account = form.get_str(2).unwrap_or("").to_string();
    if !EventsScreen::get_account_names(state).contains(&account) {
        return ActionResult::error("Select an existing account");
    }
    let inner = AmountData::AccountBalance {
        account: AccountTag(account),
    };
    let amount = match super::value_input::read(state, form, 0, crate::modals::ParameterKind::Rate)
    {
        Ok(super::value_input::ValueInput::Entered(finplan_core::model::ParameterValue::Rate(
            value,
        ))) => AmountData::scale(value, inner),
        Ok(super::value_input::ValueInput::Parameter(rate)) => AmountData::RateTimes {
            rate,
            inner: Box::new(inner),
        },
        Err(error) => return ActionResult::error(error),
        _ => unreachable!(),
    };

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

#[cfg(test)]
mod value_tests {
    use super::*;
    use crate::{data::named_parameters::NamedParameterData, modals::ConfirmedValue};
    use finplan_core::model::ParameterValue;

    #[test]
    fn percentage_editor_preserves_rate_references_and_account() {
        use crate::data::portfolio_data::{AccountData, AccountType, Property};

        let mut state = AppState::new();
        state.data_mut().portfolios.accounts = ["Checking", "Savings"]
            .into_iter()
            .map(|name| AccountData {
                name: name.into(),
                description: None,
                account_type: AccountType::Savings(Property {
                    value: 10_000.0,
                    return_profile: None,
                }),
            })
            .collect();
        state.data_mut().named_parameters = vec![
            NamedParameterData {
                name: "Withdrawal rate".into(),
                value: ParameterValue::Rate(0.05),
            },
            NamedParameterData {
                name: "Salary".into(),
                value: ParameterValue::Money(1234.0),
            },
        ];
        let balance = AmountData::AccountBalance {
            account: AccountTag("Savings".into()),
        };
        for expected in [
            AmountData::scale(0.04, balance.clone()),
            AmountData::RateTimes {
                rate: "Withdrawal rate".into(),
                inner: Box::new(balance.clone()),
            },
        ] {
            state.pending_effect_form = Some(FormModal::new(
                "Expense",
                vec![FormField::amount("Amount", expected.clone())],
                ModalAction::ADD_EFFECT,
            ));
            let ActionResult::Done(Some(ModalState::Form(form))) =
                show_scale_form(&state, 0, 0, 0, EffectTypeContext::Expense)
            else {
                panic!()
            };
            assert_eq!(
                form.fields[0].options,
                vec!["Enter value", "Parameter: Withdrawal rate"]
            );
            assert_eq!(form.get_str(2), Some("Savings"));
            if matches!(expected, AmountData::RateTimes { .. }) {
                assert_eq!(form.get_str(0), Some("Parameter: Withdrawal rate"));
                assert_eq!(form.get_percentage(1), Some(0.05));
                assert_eq!(
                    form.fields[1].field_type,
                    crate::modals::FieldType::ReadOnly
                );
            } else {
                assert_eq!(form.get_percentage(1), Some(0.04));
            }

            // Invalid input must keep the pending effect form available for correction.
            for (source, percentage, account) in [
                ("Parameter: Salary", "5", "Savings"),
                ("Parameter: missing", "5", "Savings"),
                ("Enter value", "NaN", "Savings"),
                ("Enter value", "4", "missing"),
            ] {
                let mut invalid = form.clone();
                invalid.fields[0].value = source.into();
                invalid.fields[1].value = percentage.into();
                invalid.fields[2].value = account.into();
                let context = invalid.context.clone();
                let value = ConfirmedValue::Form(Box::new(invalid));
                assert!(matches!(
                    handle_scale_form(&mut state, ActionContext::new(context.as_ref(), &value)),
                    ActionResult::Error(_)
                ));
                assert!(state.pending_effect_form.is_some());
            }

            let context = form.context.clone();
            let value = ConfirmedValue::Form(Box::new(form.clone()));
            let ActionResult::Done(Some(ModalState::Form(saved))) =
                handle_scale_form(&mut state, ActionContext::new(context.as_ref(), &value))
            else {
                panic!()
            };
            assert_eq!(saved.get_amount(0), Some(expected));

            // Switching either source to an entered percentage saves a decimal multiplier.
            state.pending_effect_form = Some(saved);
            let mut entered = form;
            entered.fields[0].value = "Enter value".into();
            entered.fields[1].value = "6".into();
            let context = entered.context.clone();
            let value = ConfirmedValue::Form(Box::new(entered));
            let ActionResult::Done(Some(ModalState::Form(saved))) =
                handle_scale_form(&mut state, ActionContext::new(context.as_ref(), &value))
            else {
                panic!()
            };
            assert_eq!(
                saved.get_amount(0),
                Some(AmountData::scale(0.06, balance.clone()))
            );
            assert_eq!(
                state.data().named_parameters[0].value,
                ParameterValue::Rate(0.05)
            );
        }
    }

    #[test]
    fn amount_editor_preserves_references_and_allows_entered_values() {
        let mut state = AppState::new();
        state.data_mut().named_parameters = vec![
            NamedParameterData {
                name: "Pay".into(),
                value: ParameterValue::Money(1234.0),
            },
            NamedParameterData {
                name: "Rate".into(),
                value: ParameterValue::Rate(0.1),
            },
        ];
        for inflation in [false, true] {
            let expected = AmountData::Parameter { name: "Pay".into() };
            let expected = if inflation {
                AmountData::inflation_adjusted(expected)
            } else {
                expected
            };
            state.pending_effect_form = Some(FormModal::new(
                "Income",
                vec![FormField::amount("Amount", expected.clone())],
                ModalAction::ADD_EFFECT,
            ));
            let ActionResult::Done(Some(ModalState::Form(form))) =
                show_fixed_amount_form(&state, 0, 0, 0, EffectTypeContext::Income, inflation)
            else {
                panic!()
            };
            assert_eq!(
                form.fields[0].options,
                vec!["Enter value", "Parameter: Pay"]
            );
            assert_eq!(form.get_str(0), Some("Parameter: Pay"));
            assert_eq!(
                form.fields[1].field_type,
                crate::modals::FieldType::ReadOnly
            );
            let context = form.context.clone();
            let value = ConfirmedValue::Form(Box::new(form));
            let ActionResult::Done(Some(ModalState::Form(form))) = handle_money_value(
                &mut state,
                ActionContext::new(context.as_ref(), &value),
                inflation,
            ) else {
                panic!()
            };
            assert_eq!(form.get_amount(0), Some(expected));
            state.pending_effect_form = Some(form);
            let ActionResult::Done(Some(ModalState::Form(mut form))) =
                show_fixed_amount_form(&state, 0, 0, 0, EffectTypeContext::Income, inflation)
            else {
                panic!()
            };
            form.fields[0].value = "Enter value".into();
            form.fields[1].value = "2500".into();
            let context = form.context.clone();
            let value = ConfirmedValue::Form(Box::new(form));
            let ActionResult::Done(Some(ModalState::Form(form))) = handle_money_value(
                &mut state,
                ActionContext::new(context.as_ref(), &value),
                inflation,
            ) else {
                panic!()
            };
            let literal = AmountData::fixed(2500.0);
            assert_eq!(
                form.get_amount(0),
                Some(if inflation {
                    AmountData::inflation_adjusted(literal)
                } else {
                    literal
                })
            );
            assert_eq!(
                state.data().named_parameters[0].value,
                ParameterValue::Money(1234.0)
            );
        }
    }

    #[test]
    fn amount_without_parameters_stays_editable_and_rejects_invalid_input() {
        let mut state = AppState::new();
        state.pending_effect_form = Some(FormModal::new(
            "Income",
            vec![FormField::amount("Amount", AmountData::fixed(50.0))],
            ModalAction::ADD_EFFECT,
        ));
        let ActionResult::Done(Some(ModalState::Form(form))) =
            show_fixed_amount_form(&state, 0, 0, 0, EffectTypeContext::Income, false)
        else {
            panic!()
        };
        assert_eq!(form.get_currency(1), Some(50.0));
        assert_eq!(form.fields[0].options, vec!["Enter value"]);
        for (source, value) in [("Enter value", "NaN"), ("Parameter: missing", "50")] {
            let mut form = form.clone();
            form.fields[0].value = source.into();
            form.fields[1].value = value.into();
            let context = form.context.clone();
            let value = ConfirmedValue::Form(Box::new(form));
            assert!(matches!(
                handle_fixed_amount_form(&mut state, ActionContext::new(context.as_ref(), &value)),
                ActionResult::Error(_)
            ));
            assert!(state.pending_effect_form.is_some());
        }
    }
}
