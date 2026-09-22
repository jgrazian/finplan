//! Typed value sources shared by amount and calendar editors.
use crate::{
    modals::{FormField, FormModal, ParameterKind},
    state::AppState,
};
use finplan_core::model::{CalendarAge, ParameterValue};

pub enum ValueInput {
    Entered(ParameterValue),
    Parameter(String),
}

pub fn fields(state: &AppState, value: ParameterValue, reference: Option<&str>) -> Vec<FormField> {
    let kind = super::parameter::kind(value);
    let mut options = vec!["Enter value".to_string()];
    options.extend(
        super::parameter::names(state, kind)
            .iter()
            .map(|name| format!("Parameter: {name}")),
    );
    let selected =
        reference.map_or_else(|| "Enter value".into(), |name| format!("Parameter: {name}"));
    let mut fields = vec![FormField::select("Value source", options, &selected)];
    let resolved = reference
        .and_then(|name| {
            state
                .data()
                .named_parameters
                .iter()
                .find(|p| p.name == name)
        })
        .map(|p| p.value)
        .filter(|value| super::parameter::kind(*value) == kind)
        .unwrap_or(value);
    fields.extend(literal_fields(resolved, reference.is_some()));
    fields
}

fn literal_fields(value: ParameterValue, parameter: bool) -> Vec<FormField> {
    let mut fields = match value {
        ParameterValue::Money(value) => vec![FormField::currency("Amount", value)],
        ParameterValue::Date(value) => {
            vec![FormField::text("Date (YYYY-MM-DD)", &value.to_string())]
        }
        ParameterValue::Age(age) => vec![
            FormField::text("Years", &age.years.to_string()),
            FormField::text("Months (0–11)", &age.months.to_string()),
        ],
        ParameterValue::Rate(value) => vec![FormField::percentage("Rate (%)", value)],
    };
    if parameter {
        for field in &mut fields {
            field.label.push_str(" (parameter value)");
            field.field_type = crate::modals::FieldType::ReadOnly;
        }
    }
    fields
}

/// Keep parameter values visible but read-only; switching to an entered value
/// lets the user edit a copy without changing the shared parameter.
pub(crate) fn refresh(state: &mut AppState, source: usize, kind: ParameterKind) {
    use crate::modals::{FieldType, ModalState};
    let name = match &state.modal {
        ModalState::Form(form) => form
            .get_str(source)
            .and_then(|s| s.strip_prefix("Parameter: "))
            .map(str::to_owned),
        _ => return,
    };
    let value = name
        .as_ref()
        .and_then(|name| {
            state
                .data()
                .named_parameters
                .iter()
                .find(|p| &p.name == name)
        })
        .map(|p| p.value)
        .filter(|value| super::parameter::kind(*value) == kind);
    let ModalState::Form(form) = &mut state.modal else {
        return;
    };
    if let Some(value) = value {
        for (index, field) in literal_fields(value, true).into_iter().enumerate() {
            form.fields[source + 1 + index] = field;
        }
    } else {
        let count = if kind == ParameterKind::Age { 2 } else { 1 };
        for index in 0..count {
            let field = &mut form.fields[source + 1 + index];
            field.label = field
                .label
                .trim_end_matches(" (parameter value)")
                .to_owned();
            field.field_type = match kind {
                ParameterKind::Money => FieldType::Currency,
                ParameterKind::Rate => FieldType::Percentage,
                _ => FieldType::Text,
            };
        }
    }
}

pub fn read(
    state: &AppState,
    form: &FormModal,
    offset: usize,
    kind: ParameterKind,
) -> Result<ValueInput, String> {
    let source = form.get_str(offset).unwrap_or("");
    if let Some(name) = source.strip_prefix("Parameter: ") {
        if !super::parameter::names(state, kind)
            .iter()
            .any(|option| option == name)
        {
            return Err("Select an existing parameter of the matching type".into());
        }
        return Ok(ValueInput::Parameter(name.into()));
    }
    if source != "Enter value" {
        return Err("Select a value source".into());
    }
    let index = offset + 1;
    let value = match kind {
        ParameterKind::Money => form
            .get_currency(index)
            .filter(|v| v.is_finite())
            .map(ParameterValue::Money),
        ParameterKind::Rate => form
            .get_percentage(index)
            .filter(|v| v.is_finite())
            .map(ParameterValue::Rate),
        ParameterKind::Date => form
            .get_str(index)
            .and_then(|v| v.parse().ok())
            .map(ParameterValue::Date),
        ParameterKind::Age => form
            .get_int::<u8>(index)
            .zip(form.get_int::<u8>(index + 1))
            .filter(|(_, months)| *months < 12)
            .map(|(years, months)| ParameterValue::Age(CalendarAge::new(years, months))),
    };
    value.map(ValueInput::Entered).ok_or_else(|| {
        "Enter a valid value (finite amount, YYYY-MM-DD date, or age with 0–11 months)".into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        data::named_parameters::NamedParameterData,
        modals::{FieldType, FormKind, ModalAction, ModalState, handle_modal_key},
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    #[test]
    fn switching_source_updates_preview_and_editability_with_real_keys() {
        for editing in [false, true] {
            let mut state = AppState::new();
            state.data_mut().named_parameters.push(NamedParameterData {
                name: "Pay".into(),
                value: ParameterValue::Money(1234.0),
            });
            let mut form = FormModal::new(
                "Amount",
                fields(&state, ParameterValue::Money(10.0), None),
                ModalAction::AMOUNT_FIXED_FORM,
            )
            .with_kind(FormKind::ValueInput {
                source: 0,
                kind: ParameterKind::Money,
            });
            form.editing = editing;
            state.modal = ModalState::Form(form);
            handle_modal_key(
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                &mut state,
            );
            let ModalState::Form(form) = &state.modal else {
                panic!()
            };
            assert_eq!(form.get_str(0), Some("Parameter: Pay"));
            assert_eq!(form.get_currency(1), Some(1234.0));
            assert_eq!(form.fields[1].field_type, FieldType::ReadOnly);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
            terminal
                .draw(|frame| crate::modals::render_modal(frame, &mut state))
                .unwrap();
            assert!(terminal.backend().buffer().content.windows(7).any(|cells| {
                cells.iter().map(|cell| cell.symbol()).collect::<String>() == "1234.00"
                    && cells
                        .iter()
                        .all(|cell| cell.fg == ratatui::style::Color::DarkGray)
            }));

            handle_modal_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), &mut state);
            let ModalState::Form(form) = &state.modal else {
                panic!()
            };
            assert_eq!(form.get_str(0), Some("Enter value"));
            assert_eq!(form.fields[1].field_type, FieldType::Currency);
            assert_eq!(
                state.data().named_parameters[0].value,
                ParameterValue::Money(1234.0)
            );
        }
    }
}
