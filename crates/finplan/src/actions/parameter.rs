//! Named parameter CRUD and kind-filtered selection shared by event editors.
use super::ActionResult;
use crate::data::named_parameters::NamedParameterData;
use crate::modals::{
    ConfirmedValue, FormField, FormModal, ModalAction, ModalState, ParameterAction, ParameterKind,
    PickerModal,
};
use crate::state::AppState;
use finplan_core::model::{CalendarAge, ParameterValue};

pub fn kind(value: ParameterValue) -> ParameterKind {
    match value {
        ParameterValue::Money(_) => ParameterKind::Money,
        ParameterValue::Rate(_) => ParameterKind::Rate,
        ParameterValue::Date(_) => ParameterKind::Date,
        ParameterValue::Age(_) => ParameterKind::Age,
    }
}
pub fn kind_name(value: ParameterValue) -> &'static str {
    match kind(value) {
        ParameterKind::Money => "Money",
        ParameterKind::Rate => "Rate",
        ParameterKind::Date => "Date",
        ParameterKind::Age => "Age",
    }
}
pub fn display_value(value: ParameterValue) -> String {
    match value {
        ParameterValue::Money(v) => format!("${v:.2}"),
        ParameterValue::Rate(v) => format!("{}%", v * 100.0),
        ParameterValue::Date(v) => v.to_string(),
        ParameterValue::Age(v) => format!("{}y {}m", v.years, v.months),
    }
}
pub fn names(state: &AppState, wanted: ParameterKind) -> Vec<String> {
    state
        .data()
        .named_parameters
        .iter()
        .filter(|p| kind(p.value) == wanted)
        .map(|p| p.name.clone())
        .collect()
}
pub fn type_picker(state: &AppState, index: Option<usize>) -> ModalState {
    let mut picker = PickerModal::new(
        "Parameter Type",
        ["Money", "Rate", "Date", "Age"]
            .map(str::to_string)
            .to_vec(),
        ModalAction::Parameter(ParameterAction::PickType { index }),
    );
    if let Some(parameter) = index.and_then(|i| state.data().named_parameters.get(i)) {
        picker.selected_index = picker
            .options
            .iter()
            .position(|n| n == kind_name(parameter.value))
            .unwrap_or(0);
    }
    ModalState::Picker(picker)
}
pub fn handle(
    state: &mut AppState,
    action: ParameterAction,
    value: &ConfirmedValue,
) -> ActionResult {
    match action {
        ParameterAction::PickType { index } => {
            let kind = match value.as_str() {
                Some("Money") => ParameterKind::Money,
                Some("Rate") => ParameterKind::Rate,
                Some("Date") => ParameterKind::Date,
                Some("Age") => ParameterKind::Age,
                _ => return ActionResult::close(),
            };
            show_form(state, index, kind)
        }
        ParameterAction::Save {
            index,
            kind: selected_kind,
        } => {
            let Some(form) = value.as_form() else {
                return ActionResult::close();
            };
            let name = form.get_str(0).unwrap_or("").trim().to_string();
            if name.is_empty() {
                return ActionResult::error("Parameter name cannot be empty");
            }
            if state
                .data()
                .named_parameters
                .iter()
                .enumerate()
                .any(|(i, p)| Some(i) != index && p.name == name)
            {
                return ActionResult::error("A parameter with this name already exists");
            }
            let parsed = match selected_kind {
                ParameterKind::Money => form
                    .get_currency(1)
                    .filter(|v| v.is_finite())
                    .map(ParameterValue::Money),
                ParameterKind::Rate => form
                    .get_percentage(1)
                    .filter(|v| v.is_finite())
                    .map(ParameterValue::Rate),
                ParameterKind::Date => form
                    .get_str(1)
                    .and_then(|s| s.parse().ok())
                    .map(ParameterValue::Date),
                ParameterKind::Age => form
                    .get_int::<u8>(1)
                    .zip(form.get_int::<u8>(2))
                    .filter(|(_, m)| *m < 12)
                    .map(|(y, m)| ParameterValue::Age(CalendarAge::new(y, m))),
            };
            let Some(parsed) = parsed else {
                return ActionResult::error(
                    "Invalid value. Use a finite amount/rate, YYYY-MM-DD date, or age 0–255 years and 0–11 months.",
                );
            };
            if let Some(index) = index {
                let Some(old) = state.data().named_parameters.get(index).cloned() else {
                    return ActionResult::error("Parameter no longer exists");
                };
                if kind(old.value) != selected_kind
                    && (!state.data().parameter_references(&old.name).is_empty()
                        || used_by_analysis(state, &old.name))
                {
                    return ActionResult::error(
                        "This parameter is used by events or analysis. Remove its references before changing its type.",
                    );
                }
                state.data_mut().rename_parameter(&old.name, &name);
                for sweep in &mut state.analysis_state.sweep_parameters {
                    if sweep.parameter_name == old.name {
                        sweep.parameter_name = name.clone();
                    }
                }
                state.data_mut().named_parameters[index] = NamedParameterData {
                    name,
                    value: parsed,
                };
            } else {
                state.data_mut().named_parameters.push(NamedParameterData {
                    name,
                    value: parsed,
                });
                state.events_state.selected_parameter_index =
                    state.data().named_parameters.len() - 1;
            }
            state.analysis_state.results = None;
            state.analysis_state.results_fingerprint = None;
            ActionResult::modified()
        }
        ParameterAction::Delete { index } => {
            let Some(parameter) = state.data().named_parameters.get(index) else {
                return ActionResult::close();
            };
            if used_by_analysis(state, &parameter.name) {
                return ActionResult::error(
                    "This parameter is selected in Analysis. Remove its sweep before deleting it.",
                );
            }
            let refs = state.data().parameter_references(&parameter.name);
            if !refs.is_empty() {
                return ActionResult::error(format!(
                    "Used by events: {}. Remove these references before deleting.",
                    refs.join(", ")
                ));
            }
            state.data_mut().named_parameters.remove(index);
            state.events_state.selected_parameter_index =
                index.min(state.data().named_parameters.len().saturating_sub(1));
            ActionResult::modified()
        }
    }
}
fn used_by_analysis(state: &AppState, name: &str) -> bool {
    // Runtime analysis is authoritative for the current scenario; sync to disk on save.
    state
        .analysis_state
        .sweep_parameters
        .iter()
        .any(|sweep| sweep.parameter_name == name)
}

fn show_form(state: &AppState, index: Option<usize>, wanted: ParameterKind) -> ActionResult {
    let current = index.and_then(|i| state.data().named_parameters.get(i));
    let value = current.filter(|p| kind(p.value) == wanted).map(|p| p.value);
    let mut fields = vec![FormField::text(
        "Name",
        current.map_or("", |p| p.name.as_str()),
    )];
    fields.extend(match wanted {
        ParameterKind::Money => vec![FormField::currency(
            "Amount ($)",
            if let Some(ParameterValue::Money(v)) = value {
                v
            } else {
                0.0
            },
        )],
        ParameterKind::Rate => vec![FormField::percentage(
            "Rate (%)",
            if let Some(ParameterValue::Rate(v)) = value {
                v
            } else {
                0.04
            },
        )],
        ParameterKind::Date => vec![FormField::text(
            "Date (YYYY-MM-DD)",
            &if let Some(ParameterValue::Date(v)) = value {
                v.to_string()
            } else {
                state.data().parameters.start_date.clone()
            },
        )],
        ParameterKind::Age => {
            let v = if let Some(ParameterValue::Age(v)) = value {
                v
            } else {
                CalendarAge::years(65)
            };
            vec![
                FormField::text("Years (0–255)", &v.years.to_string()),
                FormField::text("Months (0–11)", &v.months.to_string()),
            ]
        }
    });
    // Preserve numeric precision when an existing value is opened and saved unchanged.
    if let Some(ParameterValue::Money(v)) = value {
        fields[1] = FormField::new(
            "Amount ($)",
            crate::modals::FieldType::Currency,
            &v.to_string(),
        );
    }
    if let Some(ParameterValue::Rate(v)) = value {
        fields[1] = FormField::new(
            "Rate (%)",
            crate::modals::FieldType::Percentage,
            &(v * 100.0).to_string(),
        );
    }
    ActionResult::modal(ModalState::Form(
        FormModal::new(
            if index.is_some() {
                "Edit Parameter"
            } else {
                "New Parameter"
            },
            fields,
            ModalAction::Parameter(ParameterAction::Save {
                index,
                kind: wanted,
            }),
        )
        .start_editing(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::events_data::{
        AccountTag, AmountData, EffectData, EventData, EventTag, TriggerData,
    };

    fn submit(
        state: &mut AppState,
        index: Option<usize>,
        kind: ParameterKind,
        values: &[&str],
    ) -> ActionResult {
        let form = FormModal::new(
            "test",
            values.iter().map(|v| FormField::text("", v)).collect(),
            ModalAction::Parameter(ParameterAction::Save { index, kind }),
        );
        handle(
            state,
            ParameterAction::Save { index, kind },
            &ConfirmedValue::Form(Box::new(form)),
        )
    }
    #[test]
    fn typed_parameter_crud_validates_values_and_protects_references() {
        let mut state = AppState::new();
        for (kind, values) in [
            (ParameterKind::Money, vec!["Salary", "$1,000"]),
            (ParameterKind::Rate, vec!["Match", "5"]),
            (ParameterKind::Date, vec!["Start", "2030-01-01"]),
            (ParameterKind::Age, vec!["Retire", "65", "6"]),
        ] {
            assert!(matches!(
                submit(&mut state, None, kind, &values),
                ActionResult::Modified(_)
            ));
        }
        assert_eq!(
            state.data().named_parameters[1].value,
            ParameterValue::Rate(0.05)
        );
        for (kind, values) in [
            (ParameterKind::Money, vec!["Bad", "NaN"]),
            (ParameterKind::Rate, vec!["Bad", "inf"]),
            (ParameterKind::Date, vec!["Bad", "2030-02-30"]),
            (ParameterKind::Age, vec!["Bad", "65", "12"]),
            (ParameterKind::Age, vec!["Bad", "256", "0"]),
            (ParameterKind::Money, vec![" Salary ", "10"]),
        ] {
            assert!(matches!(
                submit(&mut state, None, kind, &values),
                ActionResult::Error(_)
            ));
        }
        state.data_mut().events.push(EventData {
            name: EventTag("Paycheck".into()),
            description: None,
            trigger: TriggerData::Manual,
            effects: vec![EffectData::Income {
                to: AccountTag("Checking".into()),
                amount: AmountData::inflation_adjusted(AmountData::Parameter {
                    name: "Salary".into(),
                }),
                gross: false,
                taxable: false,
            }],
            once: false,
            enabled: false,
        });
        assert!(matches!(
            handle(
                &mut state,
                ParameterAction::Delete { index: 0 },
                &ConfirmedValue::Confirm
            ),
            ActionResult::Error(_)
        ));
        assert!(matches!(
            submit(&mut state, Some(0), ParameterKind::Rate, &["Salary", "5"]),
            ActionResult::Error(_)
        ));
        assert!(matches!(
            submit(
                &mut state,
                Some(0),
                ParameterKind::Money,
                &["Annual pay", "2000"]
            ),
            ActionResult::Modified(_)
        ));
        assert_eq!(
            state.data().parameter_references("Annual pay"),
            vec!["Paycheck"]
        );
        state.data_mut().events.clear();
        assert!(matches!(
            handle(
                &mut state,
                ParameterAction::Delete { index: 0 },
                &ConfirmedValue::Confirm
            ),
            ActionResult::Modified(_)
        ));
        assert_eq!(state.data().named_parameters.len(), 3);
    }
    #[test]
    fn analysis_references_follow_renames_and_protect_parameter_type_and_deletion() {
        use crate::data::analysis_data::SweepParameterData;
        let mut state = AppState::new();
        submit(&mut state, None, ParameterKind::Money, &["Pay", "100"]);
        state
            .analysis_state
            .sweep_parameters
            .push(SweepParameterData {
                parameter_name: "Pay".into(),
                min_value: ParameterValue::Money(50.0),
                max_value: ParameterValue::Money(150.0),
                step_count: 3,
            });
        state.save_analysis_to_current_scenario();
        assert!(matches!(
            handle(
                &mut state,
                ParameterAction::Delete { index: 0 },
                &ConfirmedValue::Confirm
            ),
            ActionResult::Error(_)
        ));
        assert!(matches!(
            submit(&mut state, Some(0), ParameterKind::Rate, &["Pay", "4"]),
            ActionResult::Error(_)
        ));
        assert!(matches!(
            submit(
                &mut state,
                Some(0),
                ParameterKind::Money,
                &["Monthly Pay", "200"]
            ),
            ActionResult::Modified(_)
        ));
        assert_eq!(
            state.analysis_state.sweep_parameters[0].parameter_name,
            "Monthly Pay"
        );
        assert_eq!(
            state.data().analysis.sweep_parameters[0].parameter_name,
            "Monthly Pay"
        );
        state
            .data_mut()
            .rename_event("Monthly Pay", "Renamed event");
        assert_eq!(
            state.data().analysis.sweep_parameters[0].parameter_name,
            "Monthly Pay"
        );
        state.analysis_state.sweep_parameters.clear();
        assert!(matches!(
            handle(
                &mut state,
                ParameterAction::Delete { index: 0 },
                &ConfirmedValue::Confirm
            ),
            ActionResult::Modified(_)
        ));
        state.save_analysis_to_current_scenario();
        assert!(state.data().analysis.sweep_parameters.is_empty());
    }
}
