// Analysis actions - parameter sweep configuration and execution

use finplan_core::model::EventId;

use crate::data::events_data::{AmountData, EffectData, TriggerData};
use crate::modals::context::AnalysisContext;
use crate::modals::{
    AnalysisAction, FieldType, FormField, FormModal, ModalAction, ModalContext, ModalState,
    PickerModal,
};
use crate::state::{
    AnalysisMetricType, AnalysisPanel, AnalysisSweepParameter, AnalysisSweepType, AppState,
};

use super::ActionResult;

/// Handle analysis-related actions
pub fn handle_analysis_action(
    state: &mut AppState,
    action: AnalysisAction,
    value: &str,
) -> ActionResult {
    match action {
        AnalysisAction::AddParameter => handle_add_parameter(state, value),
        AnalysisAction::ConfigureParameter { index } => handle_configure_parameter(state, index),
        AnalysisAction::DeleteParameter { index } => handle_delete_parameter(state, index),
        AnalysisAction::ToggleMetric => handle_toggle_metric(state, value),
        AnalysisAction::ConfigureSettings => handle_configure_settings(state, value),
        AnalysisAction::RunAnalysis => handle_run_analysis(state),
        AnalysisAction::SelectParameterTarget { event_index } => {
            handle_select_parameter_target(state, event_index)
        }
    }
}

/// Show picker to select an event for a new sweep parameter
fn handle_add_parameter(state: &mut AppState, value: &str) -> ActionResult {
    // If value is provided, we're handling a selection
    if !value.is_empty() {
        return handle_event_selected(state, value);
    }

    // Get events that have sweepable parameters
    let sweepable = get_sweepable_events(state);

    if sweepable.is_empty() {
        return ActionResult::error(
            "No sweepable events found. Add events with Age triggers or fixed Income/Expense effects.",
        );
    }

    // Check if we already have 2 parameters (max for 2D sweep)
    if state.analysis_state.sweep_parameters.len() >= 2 {
        return ActionResult::error("Maximum of 2 sweep parameters supported for 2D analysis.");
    }

    let options: Vec<String> = sweepable.iter().map(|(name, _, _)| name.clone()).collect();

    let picker = PickerModal::new(
        "Select Event to Sweep",
        options,
        ModalAction::ADD_ANALYSIS_PARAMETER,
    )
    .with_typed_context(ModalContext::Analysis(AnalysisContext::SelectEvent));

    ActionResult::modal(ModalState::Picker(picker))
}

/// Handle event selection - show target picker (trigger vs effect)
fn handle_event_selected(state: &mut AppState, event_name: &str) -> ActionResult {
    let sweepable = get_sweepable_events(state);

    // Find the event index
    let event_index = sweepable.iter().position(|(name, _, _)| name == event_name);

    let Some(event_index) = event_index else {
        return ActionResult::error(format!("Event '{}' not found", event_name));
    };

    let (_, targets, _) = &sweepable[event_index];

    // If there's only one target, go straight to configuration
    if targets.len() == 1 {
        return show_parameter_config_form(state, event_index, &targets[0]);
    }

    // Otherwise show target picker
    let options: Vec<String> = targets
        .iter()
        .map(|t| format!("{} ({})", t.display_name(), event_name))
        .collect();

    let picker = PickerModal::new(
        "Select Parameter to Sweep",
        options,
        ModalAction::Analysis(AnalysisAction::SelectParameterTarget { event_index }),
    )
    .with_typed_context(ModalContext::Analysis(AnalysisContext::SelectTarget {
        event_index,
    }));

    ActionResult::modal(ModalState::Picker(picker))
}

/// Handle target selection after event is picked
fn handle_select_parameter_target(state: &mut AppState, event_index: usize) -> ActionResult {
    // Get the selected target from the modal
    let selected = if let ModalState::Picker(ref picker) = state.modal {
        picker
            .options
            .get(picker.selected_index)
            .cloned()
            .unwrap_or_default()
    } else {
        return ActionResult::error("Expected picker modal");
    };

    let sweepable = get_sweepable_events(state);

    if event_index >= sweepable.len() {
        return ActionResult::error("Event index out of bounds");
    }

    let (_, targets, _) = &sweepable[event_index];

    // Parse the target type from the selection
    let target = targets
        .iter()
        .find(|t| selected.starts_with(t.display_name()))
        .cloned();

    let Some(target) = target else {
        return ActionResult::error("Could not determine sweep target");
    };

    show_parameter_config_form(state, event_index, &target)
}

/// Show configuration form for a sweep parameter
fn show_parameter_config_form(
    state: &mut AppState,
    event_index: usize,
    sweep_type: &AnalysisSweepType,
) -> ActionResult {
    let sweepable = get_sweepable_events(state);

    if event_index >= sweepable.len() {
        return ActionResult::error("Event index out of bounds");
    }

    let (event_name, _, defaults) = &sweepable[event_index];

    // Get default values based on sweep type
    let (default_min, default_max) = match sweep_type {
        AnalysisSweepType::TriggerAge
        | AnalysisSweepType::RepeatingStartAge
        | AnalysisSweepType::RepeatingEndAge => {
            // Look for age defaults
            defaults
                .iter()
                .find(|(t, _, _)| t == sweep_type)
                .map(|(_, min, max)| (*min, *max))
                .unwrap_or((60.0, 70.0))
        }
        AnalysisSweepType::TriggerDate => defaults
            .iter()
            .find(|(t, _, _)| t == sweep_type)
            .map(|(_, min, max)| (*min, *max))
            .unwrap_or((2030.0, 2040.0)),
        AnalysisSweepType::EffectValue => defaults
            .iter()
            .find(|(t, _, _)| t == sweep_type)
            .map(|(_, min, max)| (*min, *max))
            .unwrap_or((0.0, 100000.0)),
    };

    let default_steps = state.analysis_state.default_steps;

    let (label, fields) = match sweep_type {
        AnalysisSweepType::TriggerAge
        | AnalysisSweepType::RepeatingStartAge
        | AnalysisSweepType::RepeatingEndAge => {
            let type_label = sweep_type.display_name();
            (
                format!("Configure {} Sweep: {}", type_label, event_name),
                vec![
                    FormField::new(
                        "Min Age",
                        FieldType::Text,
                        &format!("{}", default_min as u8),
                    ),
                    FormField::new(
                        "Max Age",
                        FieldType::Text,
                        &format!("{}", default_max as u8),
                    ),
                    FormField::new("Steps", FieldType::Text, &default_steps.to_string()),
                ],
            )
        }
        AnalysisSweepType::TriggerDate => (
            format!("Configure Year Sweep: {}", event_name),
            vec![
                FormField::new(
                    "Min Year",
                    FieldType::Text,
                    &format!("{}", default_min as i32),
                ),
                FormField::new(
                    "Max Year",
                    FieldType::Text,
                    &format!("{}", default_max as i32),
                ),
                FormField::new("Steps", FieldType::Text, &default_steps.to_string()),
            ],
        ),
        AnalysisSweepType::EffectValue => (
            format!("Configure Amount Sweep: {}", event_name),
            vec![
                FormField::currency("Min Amount", default_min),
                FormField::currency("Max Amount", default_max),
                FormField::new("Steps", FieldType::Text, &default_steps.to_string()),
            ],
        ),
    };

    // Find the actual event ID
    let event_id = state
        .data()
        .events
        .iter()
        .position(|e| e.name.0 == *event_name)
        .map(|idx| EventId((idx + 1) as u16));

    let Some(event_id) = event_id else {
        return ActionResult::error(format!("Event '{}' not found", event_name));
    };

    // Store the parameter info temporarily (will be completed in configure handler)
    let next_idx = state.analysis_state.sweep_parameters.len();
    state
        .analysis_state
        .sweep_parameters
        .push(AnalysisSweepParameter {
            event_id,
            name: event_name.clone(),
            sweep_type: *sweep_type,
            min_value: default_min,
            max_value: default_max,
            step_count: default_steps,
            current_value: default_min,
        });

    let action = ModalAction::Analysis(AnalysisAction::ConfigureParameter { index: next_idx });
    let form = FormModal::new(&label, fields, action)
        .with_typed_context(ModalContext::Analysis(AnalysisContext::Parameter {
            index: next_idx,
        }))
        .start_editing();

    ActionResult::modal(ModalState::Form(form))
}

/// Handle parameter configuration form submission
fn handle_configure_parameter(state: &mut AppState, index: usize) -> ActionResult {
    // Extract form values
    let updates = if let ModalState::Form(ref form) = state.modal {
        let values = form.values();

        if let Some(param) = state.analysis_state.sweep_parameters.get(index) {
            let (min, max, steps) = match param.sweep_type {
                AnalysisSweepType::TriggerAge
                | AnalysisSweepType::RepeatingStartAge
                | AnalysisSweepType::RepeatingEndAge => {
                    let min = values.int(0, 60) as f64;
                    let max = values.int(1, 70) as f64;
                    let steps = values.int(2, 6);
                    (min, max, steps)
                }
                AnalysisSweepType::TriggerDate => {
                    let min = values.int(0, 2030) as f64;
                    let max = values.int(1, 2040) as f64;
                    let steps = values.int(2, 6);
                    (min, max, steps)
                }
                AnalysisSweepType::EffectValue => {
                    let min = values.currency(0, 0.0);
                    let max = values.currency(1, 100000.0);
                    let steps = values.int(2, 6);
                    (min, max, steps)
                }
            };
            Some((min, max, steps))
        } else {
            None
        }
    } else {
        None
    };

    // Apply updates
    if let Some((min_value, max_value, step_count)) = updates
        && let Some(param) = state.analysis_state.sweep_parameters.get_mut(index)
    {
        param.min_value = min_value;
        param.max_value = max_value;
        param.step_count = step_count;
        param.current_value = min_value;
    }

    ActionResult::Modified(None)
}

/// Delete a sweep parameter
fn handle_delete_parameter(state: &mut AppState, index: usize) -> ActionResult {
    if index < state.analysis_state.sweep_parameters.len() {
        state.analysis_state.sweep_parameters.remove(index);

        // Adjust selected index if needed
        if state.analysis_state.selected_param_index > 0
            && state.analysis_state.selected_param_index
                >= state.analysis_state.sweep_parameters.len()
        {
            state.analysis_state.selected_param_index = state
                .analysis_state
                .sweep_parameters
                .len()
                .saturating_sub(1);
        }
        ActionResult::Modified(None)
    } else {
        ActionResult::close()
    }
}

/// Show metric toggle multi-select
fn handle_toggle_metric(state: &mut AppState, value: &str) -> ActionResult {
    // If value is provided, toggle that metric
    if !value.is_empty() {
        let metric = parse_metric(value);
        if let Some(metric) = metric {
            if state.analysis_state.selected_metrics.contains(&metric) {
                state.analysis_state.selected_metrics.remove(&metric);
            } else {
                state.analysis_state.selected_metrics.insert(metric);
            }
            return ActionResult::Modified(None);
        }
    }

    // Show picker with current selections
    let metrics = [
        ("Success Rate", AnalysisMetricType::SuccessRate),
        ("P5 Final Net Worth", AnalysisMetricType::P5FinalNetWorth),
        ("P50 Final Net Worth", AnalysisMetricType::P50FinalNetWorth),
        ("P95 Final Net Worth", AnalysisMetricType::P95FinalNetWorth),
        ("Lifetime Taxes", AnalysisMetricType::LifetimeTaxes),
        ("Max Drawdown", AnalysisMetricType::MaxDrawdown),
    ];

    let options: Vec<String> = metrics
        .iter()
        .map(|(label, metric)| {
            let selected = if state.analysis_state.selected_metrics.contains(metric) {
                "[x]"
            } else {
                "[ ]"
            };
            format!("{} {}", selected, label)
        })
        .collect();

    let picker = PickerModal::new(
        "Toggle Metrics",
        options,
        ModalAction::TOGGLE_ANALYSIS_METRIC,
    )
    .with_typed_context(ModalContext::Analysis(AnalysisContext::Metrics));

    ActionResult::modal(ModalState::Picker(picker))
}

/// Parse a metric label back to the enum
fn parse_metric(label: &str) -> Option<AnalysisMetricType> {
    // Strip checkbox prefix if present
    let label = label.trim_start_matches("[x] ").trim_start_matches("[ ] ");

    match label {
        "Success Rate" => Some(AnalysisMetricType::SuccessRate),
        "P5 Final Net Worth" => Some(AnalysisMetricType::P5FinalNetWorth),
        "P50 Final Net Worth" => Some(AnalysisMetricType::P50FinalNetWorth),
        "P95 Final Net Worth" => Some(AnalysisMetricType::P95FinalNetWorth),
        "Lifetime Taxes" => Some(AnalysisMetricType::LifetimeTaxes),
        "Max Drawdown" => Some(AnalysisMetricType::MaxDrawdown),
        _ => None,
    }
}

/// Show analysis settings configuration form
pub fn show_settings_form(state: &mut AppState) -> ActionResult {
    let fields = vec![
        FormField::new(
            "Monte Carlo Iterations",
            FieldType::Text,
            &state.analysis_state.mc_iterations.to_string(),
        ),
        FormField::new(
            "Default Steps",
            FieldType::Text,
            &state.analysis_state.default_steps.to_string(),
        ),
    ];

    let form = FormModal::new(
        "Analysis Settings",
        fields,
        ModalAction::CONFIGURE_ANALYSIS_SETTINGS,
    )
    .with_typed_context(ModalContext::Analysis(AnalysisContext::Settings))
    .start_editing();

    ActionResult::modal(ModalState::Form(form))
}

/// Handle settings configuration form submission
fn handle_configure_settings(state: &mut AppState, _value: &str) -> ActionResult {
    // Parse form values
    if let ModalState::Form(ref form) = state.modal {
        let values = form.values();

        state.analysis_state.mc_iterations = values.int(0, 500);
        state.analysis_state.default_steps = values.int(1, 6);
    }

    ActionResult::Modified(None)
}

/// Start the analysis run
fn handle_run_analysis(state: &mut AppState) -> ActionResult {
    use crate::state::PendingSimulation;
    use finplan_core::analysis::{
        EffectParam, EffectTarget, SweepConfig, SweepParameter, SweepTarget, TriggerParam,
    };

    // Validate we have parameters
    if state.analysis_state.sweep_parameters.is_empty() {
        return ActionResult::error("No sweep parameters configured. Press 'a' to add parameters.");
    }

    // Validate we have metrics
    if state.analysis_state.selected_metrics.is_empty() {
        return ActionResult::error("No metrics selected. Press 'm' to select metrics.");
    }

    // Build core SweepConfig from TUI state
    let mut parameters = Vec::new();
    for param in &state.analysis_state.sweep_parameters {
        let target = match param.sweep_type {
            AnalysisSweepType::TriggerAge => SweepTarget::Trigger(TriggerParam::Age),
            AnalysisSweepType::TriggerDate => SweepTarget::Trigger(TriggerParam::Date),
            AnalysisSweepType::EffectValue => SweepTarget::Effect {
                param: EffectParam::Value,
                target: EffectTarget::FirstEligible,
            },
            AnalysisSweepType::RepeatingStartAge => {
                SweepTarget::Trigger(TriggerParam::RepeatingStart(Box::new(TriggerParam::Age)))
            }
            AnalysisSweepType::RepeatingEndAge => {
                SweepTarget::Trigger(TriggerParam::RepeatingEnd(Box::new(TriggerParam::Age)))
            }
        };

        parameters.push(SweepParameter {
            event_id: param.event_id,
            target,
            min_value: param.min_value,
            max_value: param.max_value,
            step_count: param.step_count,
        });
    }

    // Convert TUI metrics to core metrics
    let metrics: Vec<finplan_core::analysis::AnalysisMetric> = state
        .analysis_state
        .selected_metrics
        .iter()
        .map(|m| match m {
            AnalysisMetricType::SuccessRate => finplan_core::analysis::AnalysisMetric::SuccessRate,
            AnalysisMetricType::NetWorthAtAge { age } => {
                finplan_core::analysis::AnalysisMetric::NetWorthAtAge { age: *age }
            }
            AnalysisMetricType::P5FinalNetWorth => {
                finplan_core::analysis::AnalysisMetric::Percentile { percentile: 5 }
            }
            AnalysisMetricType::P50FinalNetWorth => {
                finplan_core::analysis::AnalysisMetric::Percentile { percentile: 50 }
            }
            AnalysisMetricType::P95FinalNetWorth => {
                finplan_core::analysis::AnalysisMetric::Percentile { percentile: 95 }
            }
            AnalysisMetricType::LifetimeTaxes => {
                finplan_core::analysis::AnalysisMetric::LifetimeTaxes
            }
            AnalysisMetricType::MaxDrawdown => finplan_core::analysis::AnalysisMetric::MaxDrawdown,
        })
        .collect();

    let sweep_config = SweepConfig {
        parameters,
        metrics,
        mc_iterations: state.analysis_state.mc_iterations,
        ..Default::default()
    };

    // Mark as running and set up progress tracking
    state.analysis_state.running = true;
    state.analysis_state.current_point = 0;
    state.analysis_state.total_points = state.analysis_state.total_sweep_points();
    state.analysis_state.results = None;

    // Switch to results panel to show progress
    state.analysis_state.focused_panel = AnalysisPanel::Results;

    // Set pending simulation request
    state.pending_simulation = Some(PendingSimulation::SweepAnalysis { sweep_config });

    ActionResult::Done(None)
}

// ========== Helper Functions ==========

/// Sweepable target information: (event_name, sweep_types, defaults)
/// Defaults are (sweep_type, default_min, default_max)
type SweepableEvent = (
    String,
    Vec<AnalysisSweepType>,
    Vec<(AnalysisSweepType, f64, f64)>,
);

/// Get events that have sweepable parameters
fn get_sweepable_events(state: &AppState) -> Vec<SweepableEvent> {
    let mut result = Vec::new();

    for event in &state.data().events {
        let mut targets = Vec::new();
        let mut defaults = Vec::new();

        // Check trigger for sweepable types
        analyze_trigger(&event.trigger, &mut targets, &mut defaults);

        // Check effects for sweepable types
        for effect in &event.effects {
            analyze_effect(effect, &mut targets, &mut defaults);
        }

        if !targets.is_empty() {
            result.push((event.name.0.clone(), targets, defaults));
        }
    }

    result
}

/// Analyze a trigger for sweepable parameters
fn analyze_trigger(
    trigger: &TriggerData,
    targets: &mut Vec<AnalysisSweepType>,
    defaults: &mut Vec<(AnalysisSweepType, f64, f64)>,
) {
    match trigger {
        TriggerData::Age { years, .. } => {
            if !targets.contains(&AnalysisSweepType::TriggerAge) {
                targets.push(AnalysisSweepType::TriggerAge);
                let age = *years as f64;
                defaults.push((AnalysisSweepType::TriggerAge, age - 5.0, age + 5.0));
            }
        }
        TriggerData::Date { date } => {
            if !targets.contains(&AnalysisSweepType::TriggerDate) {
                targets.push(AnalysisSweepType::TriggerDate);
                // Parse year from date string "YYYY-MM-DD"
                let year = date
                    .split('-')
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(2030.0);
                defaults.push((AnalysisSweepType::TriggerDate, year - 5.0, year + 5.0));
            }
        }
        TriggerData::Repeating { start, end, .. } => {
            // Check start trigger
            if let Some(start_trigger) = start
                && let TriggerData::Age { years, .. } = start_trigger.as_ref()
                && !targets.contains(&AnalysisSweepType::RepeatingStartAge)
            {
                targets.push(AnalysisSweepType::RepeatingStartAge);
                let age = *years as f64;
                defaults.push((AnalysisSweepType::RepeatingStartAge, age - 5.0, age + 5.0));
            }
            // Check end trigger
            if let Some(end_trigger) = end
                && let TriggerData::Age { years, .. } = end_trigger.as_ref()
                && !targets.contains(&AnalysisSweepType::RepeatingEndAge)
            {
                targets.push(AnalysisSweepType::RepeatingEndAge);
                let age = *years as f64;
                defaults.push((AnalysisSweepType::RepeatingEndAge, age - 5.0, age + 5.0));
            }
        }
        _ => {}
    }
}

/// Analyze an effect for sweepable parameters
fn analyze_effect(
    effect: &EffectData,
    targets: &mut Vec<AnalysisSweepType>,
    defaults: &mut Vec<(AnalysisSweepType, f64, f64)>,
) {
    // Only sweep fixed amounts (not dynamic/account-based)
    let amount = match effect {
        EffectData::Income { amount, .. } => Some(amount),
        EffectData::Expense { amount, .. } => Some(amount),
        EffectData::Sweep { amount, .. } => Some(amount),
        EffectData::AssetPurchase { amount, .. } => Some(amount),
        EffectData::AssetSale { amount, .. } => Some(amount),
        EffectData::AdjustBalance { amount, .. } => Some(amount),
        EffectData::CashTransfer { amount, .. } => Some(amount),
        _ => None,
    };

    if let Some(amount) = amount
        && let Some(fixed_value) = extract_fixed_amount(amount)
        && !targets.contains(&AnalysisSweepType::EffectValue)
    {
        targets.push(AnalysisSweepType::EffectValue);
        // Default range: 50% to 150% of current value
        let min = (fixed_value * 0.5).max(0.0);
        let max = fixed_value * 1.5;
        defaults.push((AnalysisSweepType::EffectValue, min, max));
    }
}

/// Extract fixed value from an amount (handling inflation-adjusted wrappers)
fn extract_fixed_amount(amount: &AmountData) -> Option<f64> {
    match amount {
        AmountData::Fixed { value } => Some(*value),
        AmountData::InflationAdjusted { inner } => extract_fixed_amount(inner),
        AmountData::Scale { inner, .. } => extract_fixed_amount(inner),
        _ => None,
    }
}

/// Get display label for a sweep parameter
pub fn format_parameter_label(param: &AnalysisSweepParameter) -> String {
    format!(
        "{}: {} ({:.0} - {:.0}, {} steps)",
        param.name,
        param.sweep_type.display_name(),
        param.min_value,
        param.max_value,
        param.step_count
    )
}
