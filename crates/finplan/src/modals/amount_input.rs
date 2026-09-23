//! Inline static/expression editing shared by every effect amount field.
use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use finplan_core::{
    config::SimulationMetadata,
    expression::compile_amount,
    model::{AmountMode, ParameterId, ParameterValue},
};

use super::{FormModal, ModalResult};
use crate::data::{
    app_data::SimulationData,
    convert::amount_expression_context,
    expressions::{parameter_source, parameter_tokens},
    keybindings_data::KeybindingsConfig,
};

#[derive(Debug, Clone)]
pub struct AmountInput {
    pub expression: bool,
    /// Retain the other mode's draft when switching back and forth.
    pub alternate: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Completion {
    start: usize,
    end: usize,
    candidates: Vec<String>,
    selected: usize,
}

pub struct Environment {
    metadata: SimulationMetadata,
    parameters: HashMap<ParameterId, ParameterValue>,
    names: Vec<String>,
    error: Option<String>,
}

impl Environment {
    pub fn new(data: &SimulationData) -> Self {
        let (metadata, parameters) = amount_expression_context(data);
        let mut names: Vec<_> = data
            .named_parameters
            .iter()
            .map(|p| p.name.clone())
            .collect();
        names.sort();
        Self {
            metadata,
            parameters,
            names,
            error: data.validate_named_parameters().err(),
        }
    }
}

pub fn handle_key(
    key: KeyEvent,
    modal: &mut FormModal,
    bindings: &KeybindingsConfig,
    environment: &Environment,
) -> ModalResult {
    if modal.fields.is_empty() {
        return ModalResult::Continue;
    }
    let index = modal.focused_field;
    let amount = modal.fields[index].amount_input.is_some();
    let expression = modal.fields[index]
        .amount_input
        .as_ref()
        .is_some_and(|input| input.expression);
    let submit = key.code == KeyCode::F(10)
        || (key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Enter | KeyCode::Char('s')));
    let completing = amount
        && expression
        && modal.editing
        && matches!(key.code, KeyCode::Tab | KeyCode::BackTab);
    if !completing {
        modal.completion = None;
    }

    if amount && modal.editing && modal.editing_original_amount.is_none() {
        modal.editing_original_amount = Some(Box::new(modal.fields[index].clone()));
    }
    // x is ordinary text while editing an expression (max, cash, etc.).
    if amount
        && key.code == KeyCode::Char('x')
        && key.modifiers.is_empty()
        && (!modal.editing || !expression)
    {
        modal
            .editing_original_amount
            .get_or_insert_with(|| Box::new(modal.fields[index].clone()));
        let field = &mut modal.fields[index];
        let input = field.amount_input.as_mut().unwrap();
        let next = input.alternate.take().unwrap_or_else(|| {
            if input.expression {
                "0".into()
            } else {
                field
                    .value
                    .replace(',', "")
                    .trim_start_matches('$')
                    .to_owned()
            }
        });
        input.alternate = Some(std::mem::replace(&mut field.value, next));
        input.expression = !input.expression;
        field.cursor_pos = field.value.len();
        modal.editing = true;
        modal.error = None;
        return ModalResult::Continue;
    }
    if completing {
        complete(modal, &environment.names, key.code == KeyCode::BackTab);
        return ModalResult::Continue;
    }
    if amount && modal.editing && key.code == KeyCode::Esc {
        if let Some(original) = modal.editing_original_amount.take() {
            modal.fields[index] = *original;
        }
        modal.editing = false;
        modal.editing_original_value = None;
        modal.error = None;
        return ModalResult::Continue;
    }

    let finish = amount
        && modal.editing
        && matches!(
            key.code,
            KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down
        );
    if submit || finish {
        let indices: Vec<_> = if submit {
            modal
                .fields
                .iter()
                .enumerate()
                .filter(|(_, f)| f.amount_input.is_some())
                .map(|(i, _)| i)
                .collect()
        } else {
            vec![index]
        };
        for i in indices {
            if let Err(error) = validate(modal, i, environment) {
                if modal.focused_field != i {
                    modal.editing_original_amount = None;
                }
                modal.focused_field = i;
                modal.editing = true;
                modal.fields[i].cursor_pos = modal.fields[i].value.len();
                modal.error = Some(error);
                return ModalResult::Continue;
            }
        }
        modal.error = None;
        modal.editing_original_amount = None;
    }

    // Typing a number on a focused amount starts a fresh edit.
    if amount
        && !modal.editing
        && matches!(key.code, KeyCode::Char(c) if c.is_ascii_digit() || c == '-' || c == '.')
    {
        modal.editing_original_amount = Some(Box::new(modal.fields[index].clone()));
        modal.fields[index].value.clear();
        modal.fields[index].cursor_pos = 0;
        modal.editing = true;
    }
    let result = super::form::handle_form_key(key, modal, bindings);
    if modal.focused_field != index {
        modal.editing_original_amount = None;
        if modal.fields[modal.focused_field].amount_input.is_some() {
            // Selecting an amount must not inherit the previous field's edit mode.
            modal.editing = false;
            modal.editing_original_value = None;
        }
    } else if amount && modal.editing && modal.editing_original_amount.is_none() {
        modal.editing_original_amount = Some(Box::new(modal.fields[index].clone()));
    }
    if matches!(
        key.code,
        KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete
    ) && !submit
    {
        modal.error = None;
    }
    result
}

fn validate(modal: &mut FormModal, index: usize, environment: &Environment) -> Result<(), String> {
    let field = &modal.fields[index];
    let amount = field.parsed_amount().ok_or_else(|| {
        format!(
            "{}: enter a finite amount, or press x for an expression",
            field.label
        )
    })?;
    if let Some(error) = &environment.error {
        return Err(error.clone());
    }
    let compiled = compile_amount(
        &amount.to_source(),
        &environment.metadata,
        &environment.parameters,
    )
    .map_err(|error| format!("{}: {error}", field.label))?;
    // The existing amount-type control is the source of truth after an annotation
    // is entered. Normalize it immediately so the form cannot show conflicting modes.
    if let Some(mode) = compiled.amount_mode {
        let mode_index = modal
            .fields
            .iter()
            .position(|f| f.label == "Amount Type")
            .ok_or_else(|| {
                "gross()/net() are only supported for income, asset sales, and sweeps".to_owned()
            })?;
        modal.fields[mode_index].value = match mode {
            AmountMode::Gross => "Gross",
            AmountMode::Net => "Net",
        }
        .into();
        modal.fields[index].value = compiled
            .amount
            .to_source(&environment.metadata)
            .map_err(|e| e.to_string())?;
        modal.fields[index].cursor_pos = modal.fields[index].value.len();
    }
    Ok(())
}

fn complete(modal: &mut FormModal, names: &[String], backwards: bool) {
    let field = &mut modal.fields[modal.focused_field];
    if let Some(completion) = &mut modal.completion {
        completion.selected = if backwards {
            (completion.selected + completion.candidates.len() - 1) % completion.candidates.len()
        } else {
            (completion.selected + 1) % completion.candidates.len()
        };
    } else {
        let Some((prefix_range, prefix)) = parameter_tokens(&field.value[..field.cursor_pos])
            .pop()
            .filter(|(range, _)| range.end == field.cursor_pos)
        else {
            modal.error = Some("Type $ and a parameter name, then press Tab to complete".into());
            return;
        };
        let candidates: Vec<_> = names
            .iter()
            .filter(|name| name.starts_with(&prefix))
            .map(|name| parameter_source(name))
            .collect();
        if candidates.is_empty() {
            modal.error = Some(format!("No parameters match ${prefix}"));
            return;
        }
        let end = parameter_tokens(&field.value)
            .into_iter()
            .find(|(range, _)| range.start == prefix_range.start)
            .map_or(field.cursor_pos, |(range, _)| range.end);
        let selected = if backwards { candidates.len() - 1 } else { 0 };
        modal.completion = Some(Completion {
            start: prefix_range.start,
            end,
            candidates,
            selected,
        });
    }
    let completion = modal.completion.as_mut().unwrap();
    let candidate = &completion.candidates[completion.selected];
    field
        .value
        .replace_range(completion.start..completion.end, candidate);
    completion.end = completion.start + candidate.len();
    field.cursor_pos = completion.end;
    modal.error = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        actions::{self, ActionContext, ActionResult},
        data::{
            events_data::{AmountData, EventData, EventTag, TriggerData},
            named_parameters::NamedParameterData,
            portfolio_data::{
                AccountData, AccountType, AssetAccount, AssetTag, AssetValue, Property,
            },
        },
        modals::{
            ConfirmedValue, FormField, ModalAction, ModalContext, ModalState, handle_modal_key,
            render_modal,
        },
        state::AppState,
    };

    fn setup() -> AppState {
        let mut state = AppState::new();
        state.data_mut().portfolios.accounts = vec![
            AccountData {
                name: "Checking".into(),
                description: None,
                account_type: AccountType::Checking(Property {
                    value: 1000.0,
                    return_profile: None,
                }),
            },
            AccountData {
                name: "Vanguard".into(),
                description: None,
                account_type: AccountType::Brokerage(AssetAccount {
                    assets: vec![AssetValue {
                        asset: AssetTag("VTI".into()),
                        value: 1000.0,
                    }],
                }),
            },
        ];
        state.data_mut().events = vec![EventData {
            name: EventTag("Test".into()),
            description: None,
            trigger: TriggerData::Manual,
            effects: vec![],
            enabled: true,
            once: false,
        }];
        state.data_mut().named_parameters = vec![
            NamedParameterData {
                name: "Spending".into(),
                value: ParameterValue::Money(500.0),
            },
            NamedParameterData {
                name: "Spending extra".into(),
                value: ParameterValue::Money(200.0),
            },
            NamedParameterData {
                name: "Épargne".into(),
                value: ParameterValue::Money(300.0),
            },
        ];
        state
    }

    fn form(state: &mut AppState) -> &mut FormModal {
        let ModalState::Form(form) = &mut state.modal else {
            panic!("expected form")
        };
        form
    }
    fn key(state: &mut AppState, code: KeyCode) -> ModalResult {
        handle_modal_key(KeyEvent::new(code, KeyModifiers::NONE), state)
    }
    fn text(state: &mut AppState, value: &str) {
        key(state, KeyCode::End);
        let count = {
            let f = form(state);
            f.fields[f.focused_field].value.chars().count()
        };
        for _ in 0..count {
            key(state, KeyCode::Backspace);
        }
        for ch in value.chars() {
            key(state, KeyCode::Char(ch));
        }
    }
    fn show(state: &mut AppState, result: ActionResult) {
        let ActionResult::Done(Some(ModalState::Form(modal))) = result else {
            panic!("expected effect form")
        };
        let amount_index = modal
            .fields
            .iter()
            .position(|f| f.amount_input.is_some())
            .unwrap();
        state.modal = ModalState::Form(modal);
        for _ in 0..amount_index {
            key(state, KeyCode::Tab);
        }
        assert_eq!(form(state).focused_field, amount_index);
        assert!(!form(state).editing);
        assert!(form(state).editing_original_value.is_none());
    }
    fn simple_form(state: &mut AppState, amount: AmountData) {
        state.modal = ModalState::Form(FormModal::new(
            "Amount",
            vec![FormField::amount("Amount", amount)],
            ModalAction::ADD_EFFECT,
        ));
    }

    #[test]
    fn every_effect_amount_supports_static_input_expression_edit_and_reload() {
        let mut state = setup();
        for name in [
            "Income",
            "Expense",
            "Asset Purchase",
            "Asset Sale",
            "Sweep",
            "Adjust Balance",
            "Cash Transfer",
        ] {
            let result = actions::handle_effect_type_for_add(&state, name);
            show(&mut state, result);
            key(&mut state, KeyCode::Char('?'));
            assert!(form(&mut state).amount_help.is_some(), "{name}");
            key(&mut state, KeyCode::Esc);
            assert!(form(&mut state).amount_help.is_none());
            assert!(!form(&mut state).editing);
            key(&mut state, KeyCode::Char('9'));
            let ModalResult::Confirmed(_, value) = key(&mut state, KeyCode::F(10)) else {
                panic!("static submission: {name}")
            };
            let submitted = value.as_form().unwrap();
            let ctx = ActionContext::new(submitted.context.as_ref(), &value);
            assert!(matches!(
                actions::handle_add_effect(&mut state, ctx),
                ActionResult::Modified(_)
            ));
            let index = state.data().events[0].effects.len() - 1;
            assert_eq!(
                state.data().events[0].effects[index].amount(),
                Some(&AmountData::fixed(9.0))
            );

            let context = ModalContext::effect_existing(0, index);
            let selected = ConfirmedValue::Picker("Edit Effect".into());
            let result = actions::handle_action_for_effect_pick(
                &state,
                "Edit Effect",
                ActionContext::new(Some(&context), &selected),
            );
            show(&mut state, result);
            key(&mut state, KeyCode::Char('x'));
            text(&mut state, "max(0, inflation($Spe");
            key(&mut state, KeyCode::Tab);
            for ch in "))".chars() {
                key(&mut state, KeyCode::Char(ch));
            }
            let ModalResult::Confirmed(_, value) = key(&mut state, KeyCode::F(10)) else {
                panic!(
                    "expression submission: {name}: {:?}",
                    form(&mut state).error
                )
            };
            let submitted = value.as_form().unwrap();
            assert!(matches!(
                actions::handle_edit_effect(
                    &mut state,
                    ActionContext::new(submitted.context.as_ref(), &value)
                ),
                ActionResult::Modified(_)
            ));
            assert_eq!(
                state.data().events[0].effects[index]
                    .amount()
                    .unwrap()
                    .to_source(),
                "max(0, inflation($Spending))"
            );
        }
        let loaded = SimulationData::from_yaml(&state.data().to_yaml().unwrap()).unwrap();
        assert_eq!(loaded.events[0].effects.len(), 7);
        assert!(crate::data::convert::to_simulation_config(&loaded).is_ok());
        for effect in &loaded.events[0].effects {
            let field = FormField::amount("Amount", effect.amount().unwrap().clone());
            assert!(field.amount_input.unwrap().expression);
            assert_eq!(field.value, "max(0, inflation($Spending))");
        }
    }

    #[test]
    fn modes_preserve_drafts_and_escape_restores_amount() {
        let mut state = setup();
        simple_form(&mut state, AmountData::fixed(42.0));
        key(&mut state, KeyCode::Char('x'));
        text(&mut state, "$Spending");
        key(&mut state, KeyCode::Enter);
        key(&mut state, KeyCode::Char('x'));
        assert_eq!(form(&mut state).fields[0].value, "42");
        text(&mut state, "125");
        key(&mut state, KeyCode::Enter);
        key(&mut state, KeyCode::Char('x'));
        assert_eq!(form(&mut state).fields[0].value, "$Spending");
        text(&mut state, "broken(");
        key(&mut state, KeyCode::Esc);
        assert_eq!(
            form(&mut state).get_amount(0),
            Some(AmountData::fixed(125.0))
        );
        key(&mut state, KeyCode::Enter);
        text(&mut state, "999");
        key(&mut state, KeyCode::Esc);
        assert_eq!(
            form(&mut state).get_amount(0),
            Some(AmountData::fixed(125.0))
        );
    }

    #[test]
    fn compiler_errors_retain_text_and_block_submit() {
        let mut state = setup();
        simple_form(&mut state, AmountData::fixed(0.0));
        key(&mut state, KeyCode::Char('x'));
        for source in ["$Missing", "inflation(", "$Spending * $Spending", "net(50)"] {
            text(&mut state, source);
            assert!(matches!(
                key(&mut state, KeyCode::F(10)),
                ModalResult::Continue
            ));
            assert!(form(&mut state).error.is_some());
            assert_eq!(form(&mut state).fields[0].value, source);
        }
        text(&mut state, "$Spending + 100");
        assert!(matches!(
            key(&mut state, KeyCode::F(10)),
            ModalResult::Confirmed(_, _)
        ));
        simple_form(&mut state, AmountData::fixed(0.0));
        key(&mut state, KeyCode::Enter);
        text(&mut state, "--1");
        assert!(matches!(
            key(&mut state, KeyCode::F(10)),
            ModalResult::Continue
        ));
    }

    #[test]
    fn completion_cycles_quotes_names_preserves_suffix_and_handles_unicode() {
        let mut state = setup();
        simple_form(
            &mut state,
            AmountData::Expression {
                source: "$Spe + 1".into(),
            },
        );
        key(&mut state, KeyCode::Enter);
        form(&mut state).fields[0].cursor_pos = 4;
        key(&mut state, KeyCode::Tab);
        assert_eq!(form(&mut state).fields[0].value, "$Spending + 1");
        key(&mut state, KeyCode::Tab);
        assert_eq!(form(&mut state).fields[0].value, "$\"Spending extra\" + 1");
        key(&mut state, KeyCode::BackTab);
        assert_eq!(form(&mut state).fields[0].value, "$Spending + 1");
        text(&mut state, "$É");
        key(&mut state, KeyCode::Tab);
        assert_eq!(form(&mut state).fields[0].value, "$Épargne");
        key(&mut state, KeyCode::Home);
        key(&mut state, KeyCode::Right);
        key(&mut state, KeyCode::Right);
        key(&mut state, KeyCode::Backspace);
        assert_eq!(form(&mut state).fields[0].value, "$pargne");
        text(&mut state, "balance(\"$Spe");
        key(&mut state, KeyCode::Tab);
        assert_eq!(form(&mut state).fields[0].value, "balance(\"$Spe");
    }

    #[test]
    fn expression_help_preserves_edit_completion_error_and_revert_state() {
        let mut state = setup();
        simple_form(&mut state, AmountData::fixed(42.0));
        key(&mut state, KeyCode::Char('x'));
        text(&mut state, "$Spe");
        key(&mut state, KeyCode::Tab);
        let original = format!("{:?}", form(&mut state));
        for close in [KeyCode::Esc, KeyCode::Enter, KeyCode::Char('?')] {
            handle_modal_key(
                KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT),
                &mut state,
            );
            assert!(form(&mut state).amount_help.is_some());
            for code in [KeyCode::F(10), KeyCode::Char('x'), KeyCode::Tab] {
                assert!(matches!(key(&mut state, code), ModalResult::Continue));
            }
            assert!(matches!(
                handle_modal_key(
                    KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
                    &mut state,
                ),
                ModalResult::Continue
            ));
            assert!(matches!(key(&mut state, close), ModalResult::Continue));
            assert_eq!(format!("{:?}", form(&mut state)), original);
        }
        key(&mut state, KeyCode::Tab);
        assert_eq!(form(&mut state).fields[0].value, "$\"Spending extra\"");
        text(&mut state, "inflation(");
        key(&mut state, KeyCode::F(10));
        assert!(form(&mut state).error.is_some());
        let invalid_draft = format!("{:?}", form(&mut state));
        key(&mut state, KeyCode::Char('?'));
        key(&mut state, KeyCode::Esc);
        assert_eq!(format!("{:?}", form(&mut state)), invalid_draft);
        key(&mut state, KeyCode::Esc);
        assert_eq!(
            form(&mut state).get_amount(0),
            Some(AmountData::fixed(42.0))
        );
    }

    #[test]
    fn help_is_available_while_navigating_amount_forms_but_keeps_other_text_editable() {
        let mut state = setup();
        state.modal = ModalState::Form(
            FormModal::new(
                "Amount form",
                vec![
                    FormField::text("Note", "Why"),
                    FormField::amount("Amount", AmountData::fixed(10.0)),
                ],
                ModalAction::ADD_EFFECT,
            )
            .start_editing(),
        );
        key(&mut state, KeyCode::Char('?'));
        assert_eq!(form(&mut state).fields[0].value, "Why?");
        assert!(form(&mut state).amount_help.is_none());
        key(&mut state, KeyCode::Enter);
        key(&mut state, KeyCode::Char('?'));
        assert!(form(&mut state).amount_help.is_some());
        key(&mut state, KeyCode::Esc);
        assert_eq!(form(&mut state).focused_field, 0);
        assert!(!form(&mut state).editing);
        form(&mut state).fields.pop();
        key(&mut state, KeyCode::Char('?'));
        assert!(form(&mut state).amount_help.is_none());
    }

    #[test]
    fn annotations_update_amount_type_and_rendering_shows_mode_help_and_errors() {
        let mut state = setup();
        let result = actions::handle_effect_type_for_add(&state, "Income");
        show(&mut state, result);
        key(&mut state, KeyCode::Char('x'));
        text(&mut state, "net($Spending)");
        key(&mut state, KeyCode::Enter);
        assert_eq!(form(&mut state).str("Amount Type"), Some("Net"));
        let index = form(&mut state).focused_field;
        assert_eq!(form(&mut state).fields[index].value, "$Spending");
        key(&mut state, KeyCode::Enter);
        text(&mut state, "$Épargne + $Missing");
        key(&mut state, KeyCode::F(10));
        for width in [70, 100] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 35)).unwrap();
            terminal
                .draw(|frame| render_modal(frame, &mut state))
                .unwrap();
            let rendered: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(rendered.contains("Expression: Tab complete"));
            assert!(rendered.contains("unknown parameter"));
            assert!(rendered.contains("Complete variable"));
            assert!(rendered.contains("[?] Expressions"));
        }
    }
}
