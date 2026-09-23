// Analysis actions - parameter sweep configuration and execution

use crate::data::analysis_data::{
    AnalysisMetricData, ChartConfigData, ChartType, ColorScheme, SweepParameterData,
};
use crate::modals::context::AnalysisContext;
use crate::modals::{
    AnalysisAction, FieldType, FormField, FormKind, FormModal, ModalAction, ModalContext,
    ModalState, PickerModal,
};
use crate::state::{AnalysisPanel, AppState};
use finplan_core::model::{CalendarAge, ParameterValue};

use super::ActionResult;

/// Handle analysis-related actions
pub fn handle_analysis_action(
    state: &mut AppState,
    action: AnalysisAction,
    value: &str,
) -> ActionResult {
    match action {
        AnalysisAction::AddParameter => handle_add_parameter(state, value),
        AnalysisAction::CreateParameter => handle_create_parameter(state),
        AnalysisAction::ConfigureParameter { index } => handle_configure_parameter(state, index),
        AnalysisAction::DeleteParameter { index } => handle_delete_parameter(state, index),
        AnalysisAction::ToggleMetric => handle_toggle_metric(state, value),
        AnalysisAction::ConfigureSettings => handle_configure_settings(state, value),
        AnalysisAction::RunAnalysis => handle_run_analysis(state),
        AnalysisAction::ConfigureChart { index } => handle_configure_chart(state, index, value),
    }
}

/// Select only variables declared in the scenario's Parameters panel.
fn handle_add_parameter(state: &mut AppState, value: &str) -> ActionResult {
    if state.analysis_state.sweep_parameters.len() >= 6 {
        return ActionResult::error(
            "Maximum of 6 sweep parameters supported. Remove a parameter to add a new one.",
        );
    }
    if !value.is_empty() {
        if state
            .analysis_state
            .sweep_parameters
            .iter()
            .any(|p| p.parameter_name == value)
        {
            return ActionResult::error("This parameter is already selected for analysis");
        }
        let Some(parameter) = state
            .data()
            .named_parameters
            .iter()
            .find(|p| p.name == value)
        else {
            return ActionResult::error(format!("Parameter '{value}' not found"));
        };
        let (min_value, max_value) = default_bounds(parameter.value);
        let sweep = SweepParameterData {
            parameter_name: parameter.name.clone(),
            min_value,
            max_value,
            step_count: state.analysis_state.default_steps.max(2),
        };
        return parameter_form(&sweep, None);
    }
    let options = state
        .data()
        .named_parameters
        .iter()
        .filter(|parameter| {
            !state
                .analysis_state
                .sweep_parameters
                .iter()
                .any(|sweep| sweep.parameter_name == parameter.name)
        })
        .map(|p| p.name.clone())
        .collect::<Vec<_>>();
    if options.is_empty() {
        return ActionResult::error(if state.data().named_parameters.is_empty() {
            "No named parameters defined. Add variables in the Parameters panel on the Events tab first."
        } else {
            "All named parameters are already selected for analysis."
        });
    }
    ActionResult::modal(ModalState::Picker(
        PickerModal::new(
            "Select Parameter to Sweep",
            options,
            ModalAction::ADD_ANALYSIS_PARAMETER,
        )
        .with_typed_context(ModalContext::Analysis(AnalysisContext::SelectParameter)),
    ))
}

fn default_bounds(value: ParameterValue) -> (ParameterValue, ParameterValue) {
    match value {
        ParameterValue::Money(value) => {
            let spread = (value.abs() * 0.5).max(100.0);
            (
                ParameterValue::Money(value - spread),
                ParameterValue::Money(value + spread),
            )
        }
        ParameterValue::Rate(value) => {
            let spread = (value.abs() * 0.5).max(0.01);
            (
                ParameterValue::Rate(value - spread),
                ParameterValue::Rate(value + spread),
            )
        }
        ParameterValue::Date(date) => (
            ParameterValue::Date(date),
            ParameterValue::Date(date.checked_add(jiff::Span::new().years(1)).unwrap_or(date)),
        ),
        ParameterValue::Age(age) => {
            let months = u16::from(age.years) * 12 + u16::from(age.months);
            let to_age = |months: u16| {
                ParameterValue::Age(CalendarAge::new((months / 12) as u8, (months % 12) as u8))
            };
            (
                to_age(months.saturating_sub(60)),
                to_age((months + 60).min(3071)),
            )
        }
    }
}

fn parameter_form(sweep: &SweepParameterData, index: Option<usize>) -> ActionResult {
    let mut fields = match (sweep.min_value, sweep.max_value) {
        (ParameterValue::Money(min), ParameterValue::Money(max)) => vec![
            FormField::new("Min Amount", FieldType::Currency, &min.to_string()),
            FormField::new("Max Amount", FieldType::Currency, &max.to_string()),
        ],
        (ParameterValue::Rate(min), ParameterValue::Rate(max)) => vec![
            FormField::new(
                "Min Rate (%)",
                FieldType::Percentage,
                &(min * 100.0).to_string(),
            ),
            FormField::new(
                "Max Rate (%)",
                FieldType::Percentage,
                &(max * 100.0).to_string(),
            ),
        ],
        (ParameterValue::Date(min), ParameterValue::Date(max)) => vec![
            FormField::text("Min Date (YYYY-MM-DD)", &min.to_string()),
            FormField::text("Max Date (YYYY-MM-DD)", &max.to_string()),
        ],
        (ParameterValue::Age(min), ParameterValue::Age(max)) => vec![
            FormField::text("Min Age (years)", &min.years.to_string()),
            FormField::text("Min Age (months 0-11)", &min.months.to_string()),
            FormField::text("Max Age (years)", &max.years.to_string()),
            FormField::text("Max Age (months 0-11)", &max.months.to_string()),
        ],
        _ => return ActionResult::error("Sweep bounds must have the same parameter type"),
    };
    fields.push(FormField::text("Steps", &sweep.step_count.to_string()));
    let (action, context) = match index {
        Some(index) => (
            AnalysisAction::ConfigureParameter { index },
            AnalysisContext::Parameter { index },
        ),
        None => (
            AnalysisAction::CreateParameter,
            AnalysisContext::NewParameter {
                parameter_name: sweep.parameter_name.clone(),
            },
        ),
    };
    ActionResult::modal(ModalState::Form(
        FormModal::new(
            &format!(
                "Sweep: {} ({})",
                sweep.parameter_name,
                super::parameter::kind_name(sweep.min_value)
            ),
            fields,
            ModalAction::Analysis(action),
        )
        .with_typed_context(ModalContext::Analysis(context))
        .start_editing(),
    ))
}

fn read_parameter_form(state: &AppState, name: &str) -> Result<SweepParameterData, String> {
    let current = state
        .data()
        .named_parameters
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("Parameter '{name}' not found"))?
        .value;
    let ModalState::Form(form) = &state.modal else {
        return Err("Expected form modal".into());
    };
    let age = |offset| {
        form.get_int::<u8>(offset)
            .zip(form.get_int::<u8>(offset + 1))
            .map(|(years, months)| ParameterValue::Age(CalendarAge::new(years, months)))
    };
    let (min, max, steps_index) = match current {
        ParameterValue::Money(_) => (
            form.get_currency(0).map(ParameterValue::Money),
            form.get_currency(1).map(ParameterValue::Money),
            2,
        ),
        ParameterValue::Rate(_) => (
            form.get_percentage(0).map(ParameterValue::Rate),
            form.get_percentage(1).map(ParameterValue::Rate),
            2,
        ),
        ParameterValue::Date(_) => (
            form.get_str(0)
                .and_then(|s| s.parse().ok())
                .map(ParameterValue::Date),
            form.get_str(1)
                .and_then(|s| s.parse().ok())
                .map(ParameterValue::Date),
            2,
        ),
        ParameterValue::Age(_) => (age(0), age(2), 4),
    };
    let min_value = min.ok_or("Enter a valid minimum value")?;
    let max_value = max.ok_or("Enter a valid maximum value")?;
    let step_count = form
        .get_int::<usize>(steps_index)
        .ok_or("Enter a whole number of steps")?;
    let sweep = SweepParameterData {
        parameter_name: name.into(),
        min_value,
        max_value,
        step_count,
    };
    sweep.validate(current)?;
    Ok(sweep)
}

fn handle_create_parameter(state: &mut AppState) -> ActionResult {
    let name = match &state.modal {
        ModalState::Form(form) => match &form.context {
            Some(ModalContext::Analysis(AnalysisContext::NewParameter { parameter_name })) => {
                parameter_name.clone()
            }
            _ => return ActionResult::error("Missing parameter context"),
        },
        _ => return ActionResult::error("Expected form modal"),
    };
    if state.analysis_state.sweep_parameters.len() >= 6
        || state
            .analysis_state
            .sweep_parameters
            .iter()
            .any(|p| p.parameter_name == name)
    {
        return ActionResult::error("Choose a different parameter (maximum 6 dimensions)");
    }
    let sweep = match read_parameter_form(state, &name) {
        Ok(sweep) => sweep,
        Err(error) => return ActionResult::error(error),
    };
    state.analysis_state.sweep_parameters.push(sweep);
    state.analysis_state.selected_param_index = state.analysis_state.sweep_parameters.len() - 1;
    state.analysis_state.invalidate_stale_results();
    ActionResult::Modified(None)
}

fn handle_configure_parameter(state: &mut AppState, index: usize) -> ActionResult {
    let Some(sweep) = state.analysis_state.sweep_parameters.get(index) else {
        return ActionResult::error("Parameter not found");
    };
    if !matches!(state.modal, ModalState::Form(_)) {
        return parameter_form(sweep, Some(index));
    }
    let updated = match read_parameter_form(state, &sweep.parameter_name) {
        Ok(sweep) => sweep,
        Err(error) => return ActionResult::error(error),
    };
    state.analysis_state.sweep_parameters[index] = updated;
    state.analysis_state.invalidate_stale_results();
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

        // Invalidate stale results since config changed
        state.analysis_state.invalidate_stale_results();

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
            // Note: We don't invalidate results here because metrics are computed at sweep time
            // and changing display metrics doesn't require re-running the sweep
            return ActionResult::Modified(None);
        }
    }

    // Show picker with current selections
    let metrics = [
        ("Success Rate", AnalysisMetricData::SuccessRate),
        ("P5 Final Net Worth", AnalysisMetricData::P5FinalNetWorth),
        ("P50 Final Net Worth", AnalysisMetricData::P50FinalNetWorth),
        ("P95 Final Net Worth", AnalysisMetricData::P95FinalNetWorth),
        ("Lifetime Taxes", AnalysisMetricData::LifetimeTaxes),
        ("Max Drawdown", AnalysisMetricData::MaxDrawdown),
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
fn parse_metric(label: &str) -> Option<AnalysisMetricData> {
    // Strip checkbox prefix if present
    let label = label.trim_start_matches("[x] ").trim_start_matches("[ ] ");

    match label {
        "Success Rate" => Some(AnalysisMetricData::SuccessRate),
        "P5 Final Net Worth" => Some(AnalysisMetricData::P5FinalNetWorth),
        "P50 Final Net Worth" => Some(AnalysisMetricData::P50FinalNetWorth),
        "P95 Final Net Worth" => Some(AnalysisMetricData::P95FinalNetWorth),
        "Lifetime Taxes" => Some(AnalysisMetricData::LifetimeTaxes),
        "Max Drawdown" => Some(AnalysisMetricData::MaxDrawdown),
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

/// Handle settings configuration - show form or process submission
fn handle_configure_settings(state: &mut AppState, _value: &str) -> ActionResult {
    // If no form modal exists, show the settings form
    if !matches!(state.modal, ModalState::Form(_)) {
        return show_settings_form(state);
    }

    // Capture old MC iterations to detect changes
    let old_mc_iterations = state.analysis_state.mc_iterations;

    // Otherwise, handle form submission - parse form values
    if let ModalState::Form(ref form) = state.modal {
        let values = form.values();

        state.analysis_state.mc_iterations = values.int(0, 500);
        state.analysis_state.default_steps = values.int(1, 6);
    }

    // Invalidate results if MC iterations changed (affects result validity)
    if state.analysis_state.mc_iterations != old_mc_iterations {
        state.analysis_state.invalidate_stale_results();
    }

    ActionResult::Modified(None)
}

/// Start the analysis run
fn handle_run_analysis(state: &mut AppState) -> ActionResult {
    use crate::state::PendingSimulation;
    use finplan_core::analysis::{SweepConfig, SweepParameter};
    if state.analysis_state.sweep_parameters.is_empty() {
        return ActionResult::error(
            "No sweep parameters configured. Press 'a' to choose a named variable.",
        );
    }
    if state.analysis_state.mc_iterations == 0 {
        return ActionResult::error("Use at least one Monte Carlo iteration in analysis settings");
    }
    if let Err(error) = state.data().validate_named_parameters() {
        return ActionResult::error(error);
    }
    let (metadata, _) = crate::data::convert::amount_expression_context(state.data());
    let mut parameters = Vec::new();
    let mut names = std::collections::HashSet::new();
    let mut total_points = 1usize;
    for sweep in &state.analysis_state.sweep_parameters {
        let Some(parameter) = state
            .data()
            .named_parameters
            .iter()
            .find(|p| p.name == sweep.parameter_name)
        else {
            return ActionResult::error(format!(
                "Parameter '{}' no longer exists",
                sweep.parameter_name
            ));
        };
        if !names.insert(&sweep.parameter_name) {
            return ActionResult::error("Each parameter can only be swept once");
        }
        if let Err(error) = sweep.validate(parameter.value) {
            return ActionResult::error(format!("{}: {error}", sweep.parameter_name));
        }
        total_points = match total_points.checked_mul(sweep.step_count) {
            Some(points) => points,
            None => return ActionResult::error("Too many sweep points"),
        };
        let id = metadata
            .parameter_id(&sweep.parameter_name)
            .expect("validated named parameter");
        parameters.push(SweepParameter::parameter(
            sweep.parameter(id),
            sweep.step_count,
        ));
    }

    // Compute ALL metrics during sweep analysis (they all come from the same P50 run)
    // User selection controls which metrics are displayed, not which are computed
    let metrics: Vec<finplan_core::analysis::AnalysisMetric> = vec![
        finplan_core::analysis::AnalysisMetric::SuccessRate,
        finplan_core::analysis::AnalysisMetric::Percentile { percentile: 5 },
        finplan_core::analysis::AnalysisMetric::Percentile { percentile: 25 },
        finplan_core::analysis::AnalysisMetric::Percentile { percentile: 50 },
        finplan_core::analysis::AnalysisMetric::Percentile { percentile: 75 },
        finplan_core::analysis::AnalysisMetric::Percentile { percentile: 95 },
        finplan_core::analysis::AnalysisMetric::LifetimeTaxes,
        finplan_core::analysis::AnalysisMetric::MaxDrawdown,
    ];

    // Use the scenario's configured seed for reproducibility
    let scenario_seed = state.data().parameters.seed;

    let sweep_config = SweepConfig {
        parameters,
        metrics,
        mc_iterations: state.analysis_state.mc_iterations,
        seed: scenario_seed,
        ..Default::default()
    };

    // Mark as running and set up progress tracking
    state.analysis_state.running = true;
    state.analysis_state.current_point = 0;
    state.analysis_state.total_points = total_points;
    state.analysis_state.results = None;

    // Switch to results panel to show progress
    state.analysis_state.focused_panel = AnalysisPanel::Results;

    // Set pending simulation request
    state.pending_simulation = Some(PendingSimulation::SweepAnalysis { sweep_config });

    ActionResult::Done(None)
}

/// Handle chart configuration - show form or process submission
fn handle_configure_chart(state: &mut AppState, index: usize, _value: &str) -> ActionResult {
    // Get results to determine available parameters
    let results = match &state.analysis_state.results {
        Some(r) => r,
        None => return ActionResult::error("No analysis results available"),
    };

    let ndim = results.ndim();

    // If no form modal exists, show the configuration form
    if !matches!(state.modal, ModalState::Form(_)) {
        return show_chart_config_form(state, index);
    }

    // Otherwise handle form submission
    if let ModalState::Form(ref form) = state.modal {
        let values = form.values();

        // Parse chart type from field 0
        let chart_type_str = values.str(0);
        let chart_type = if chart_type_str.contains("2D") || chart_type_str.contains("Heatmap") {
            ChartType::Heatmap2D
        } else {
            ChartType::Scatter1D
        };

        // Parse X parameter from field 1
        let x_param_str = values.str(1);
        let x_param_index = parse_param_index(x_param_str, ndim);

        // Parse Y parameter from field 2 (for 2D charts)
        let y_param_index = if chart_type == ChartType::Heatmap2D && ndim >= 2 {
            let y_param_str = values.str(2);
            Some(parse_param_index(y_param_str, ndim))
        } else {
            None
        };

        // Parse metric from field 3 (when Y param field exists) or 2 (when ndim < 2)
        let metric_field_idx = if ndim >= 2 { 3 } else { 2 };
        let metric_str = values.str(metric_field_idx);
        let metric = parse_metric(metric_str).unwrap_or(AnalysisMetricData::SuccessRate);

        // Parse color scheme from the next field
        let color_scheme_field_idx = metric_field_idx + 1;
        let color_scheme_str = values.str(color_scheme_field_idx);
        let color_scheme = parse_color_scheme(color_scheme_str);

        // Create or update the chart config
        let chart_config = ChartConfigData {
            chart_type,
            x_param_index,
            y_param_index,
            metric,
            color_scheme,
            fixed_values: std::collections::HashMap::new(),
        };

        // Ensure we have enough slots
        while state.analysis_state.chart_configs.len() <= index {
            state
                .analysis_state
                .chart_configs
                .push(ChartConfigData::new_1d(0, AnalysisMetricData::SuccessRate));
        }

        state.analysis_state.chart_configs[index] = chart_config;
        state.analysis_state.selected_chart_index = index;
    }

    ActionResult::Modified(None)
}

/// Show the chart configuration form
fn show_chart_config_form(state: &mut AppState, chart_index: usize) -> ActionResult {
    let results = match &state.analysis_state.results {
        Some(r) => r,
        None => return ActionResult::error("No analysis results available"),
    };

    let ndim = results.ndim();

    // Get existing chart config if any
    let existing = state.analysis_state.chart_configs.get(chart_index);

    // Determine current values
    let current_type = existing
        .map(|c| c.chart_type)
        .unwrap_or(ChartType::Scatter1D);
    let current_x = existing.map(|c| c.x_param_index).unwrap_or(0);
    let current_y = existing.and_then(|c| c.y_param_index).unwrap_or(1);
    let current_metric = existing
        .map(|c| c.metric)
        .unwrap_or(AnalysisMetricData::SuccessRate);

    // Build parameter options for picker fields
    let param_options: Vec<String> = (0..ndim)
        .map(|i| format!("{}: {}", i, results.param_label(i)))
        .collect();

    // Chart type options
    let type_options = vec!["1D Scatter".to_string(), "2D Heatmap".to_string()];
    let current_type_str = match current_type {
        ChartType::Scatter1D => "1D Scatter",
        ChartType::Heatmap2D => "2D Heatmap",
    };

    // Metric options
    let metric_options = vec![
        "Success Rate".to_string(),
        "P5 Final Net Worth".to_string(),
        "P50 Final Net Worth".to_string(),
        "P95 Final Net Worth".to_string(),
        "Lifetime Taxes".to_string(),
        "Max Drawdown".to_string(),
    ];
    let current_metric_str = match current_metric {
        AnalysisMetricData::SuccessRate => "Success Rate",
        AnalysisMetricData::P5FinalNetWorth => "P5 Final Net Worth",
        AnalysisMetricData::P50FinalNetWorth => "P50 Final Net Worth",
        AnalysisMetricData::P95FinalNetWorth => "P95 Final Net Worth",
        AnalysisMetricData::LifetimeTaxes => "Lifetime Taxes",
        AnalysisMetricData::MaxDrawdown => "Max Drawdown",
        _ => "Success Rate",
    };

    let mut fields = vec![
        FormField::select("Chart Type", type_options, current_type_str),
        FormField::select(
            "X Parameter",
            param_options.clone(),
            &format!("{}: {}", current_x, results.param_label(current_x)),
        ),
    ];

    // Add Y parameter field - always a Select, but options change based on chart type
    if ndim >= 2 {
        let (y_options, y_value) = if current_type == ChartType::Scatter1D {
            // For 1D, show only "N/A" option
            (vec!["N/A".to_string()], "N/A".to_string())
        } else {
            // For 2D, show full parameter list
            (
                param_options,
                format!(
                    "{}: {}",
                    current_y,
                    results.param_label(current_y.min(ndim - 1))
                ),
            )
        };
        fields.push(FormField::select("Y Parameter", y_options, &y_value));
    }

    fields.push(FormField::select(
        "Metric",
        metric_options,
        current_metric_str,
    ));

    // Color scheme options (for heatmaps)
    let color_scheme_options: Vec<String> = ColorScheme::all()
        .iter()
        .map(|s| s.display_name().to_string())
        .collect();
    let current_color_scheme = existing.map(|c| c.color_scheme).unwrap_or_default();
    fields.push(FormField::select(
        "Color Scheme",
        color_scheme_options,
        current_color_scheme.display_name(),
    ));

    let title = format!("Configure Chart {}", chart_index + 1);
    let action = ModalAction::Analysis(AnalysisAction::ConfigureChart { index: chart_index });

    let form = FormModal::new(&title, fields, action)
        .with_kind(FormKind::ChartConfig)
        .with_typed_context(ModalContext::Analysis(AnalysisContext::ChartConfig {
            chart_index,
        }))
        .start_editing();

    ActionResult::modal(ModalState::Form(form))
}

/// Parse parameter index from form value like "0: Retirement Age"
fn parse_param_index(value: &str, ndim: usize) -> usize {
    value
        .split(':')
        .next()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(0)
        .min(ndim.saturating_sub(1))
}

/// Parse color scheme from form value
fn parse_color_scheme(value: &str) -> ColorScheme {
    match value.to_lowercase().as_str() {
        "viridis" => ColorScheme::Viridis,
        "magma" => ColorScheme::Magma,
        "inferno" => ColorScheme::Inferno,
        "plasma" => ColorScheme::Plasma,
        "cividis" => ColorScheme::Cividis,
        "rocket" => ColorScheme::Rocket,
        "mako" => ColorScheme::Mako,
        "turbo" => ColorScheme::Turbo,
        _ => ColorScheme::default(),
    }
}

/// Get a readable range for a named sweep parameter.
pub fn format_parameter_label(param: &SweepParameterData) -> String {
    use crate::data::analysis_data::format_sweep_value;
    format!(
        "{}: {} - {} ({} steps)",
        param.parameter_name,
        format_sweep_value(param.min_value),
        format_sweep_value(param.max_value),
        param.step_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{
        app_data::SimulationData,
        events_data::{AccountTag, AmountData, EffectData, EventData, EventTag, TriggerData},
        named_parameters::NamedParameterData,
    };
    use crate::state::{AnalysisResults, PendingSimulation};
    use finplan_core::{
        analysis::{SweepResults, SweepTarget},
        model::ParameterId,
    };
    use jiff::civil::date;

    fn state() -> AppState {
        let mut state = AppState::new();
        state.data_mut().events = vec![EventData {
            name: EventTag("Event only".into()),
            description: None,
            trigger: TriggerData::Age {
                years: 65,
                months: None,
            },
            effects: vec![EffectData::Income {
                to: AccountTag("Cash".into()),
                amount: AmountData::fixed(100.0),
                gross: true,
                taxable: false,
            }],
            once: true,
            enabled: true,
        }];
        state.data_mut().named_parameters = vec![
            NamedParameterData {
                name: "Spending".into(),
                value: ParameterValue::Money(1000.0),
            },
            NamedParameterData {
                name: "Rate".into(),
                value: ParameterValue::Rate(0.04),
            },
            NamedParameterData {
                name: "Start".into(),
                value: ParameterValue::Date(date(2025, 1, 1)),
            },
            NamedParameterData {
                name: "Retire".into(),
                value: ParameterValue::Age(CalendarAge::new(65, 0)),
            },
        ];
        state
    }
    fn choose(state: &mut AppState, name: &str, values: &[&str]) {
        let ActionResult::Done(Some(ModalState::Form(mut form))) =
            handle_add_parameter(state, name)
        else {
            panic!("expected typed form")
        };
        for (field, value) in form.fields.iter_mut().zip(values) {
            field.value = (*value).into();
        }
        state.modal = ModalState::Form(form);
    }

    #[test]
    fn picker_uses_only_declared_variables_and_excludes_selected_ones() {
        let mut state = state();
        let ActionResult::Done(Some(ModalState::Picker(picker))) =
            handle_add_parameter(&mut state, "")
        else {
            panic!()
        };
        assert_eq!(picker.options, ["Spending", "Rate", "Start", "Retire"]);
        assert!(matches!(
            handle_add_parameter(&mut state, "Event only"),
            ActionResult::Error(_)
        ));
        choose(&mut state, "Spending", &["500", "1500", "3"]);
        assert!(matches!(
            handle_create_parameter(&mut state),
            ActionResult::Modified(_)
        ));
        let ActionResult::Done(Some(ModalState::Picker(picker))) =
            handle_add_parameter(&mut state, "")
        else {
            panic!()
        };
        assert_eq!(picker.options, ["Rate", "Start", "Retire"]);
        assert!(matches!(
            handle_add_parameter(&mut state, "Spending"),
            ActionResult::Error(_)
        ));
        state.data_mut().named_parameters.clear();
        assert!(
            matches!(handle_add_parameter(&mut state, ""), ActionResult::Error(message) if message.contains("Parameters panel"))
        );
    }

    #[test]
    fn all_parameter_types_save_load_and_target_the_registry() {
        let mut state = state();
        for (name, fields) in [
            ("Spending", vec!["500", "1500", "3"]),
            ("Rate", vec!["2", "6", "3"]),
            ("Start", vec!["2024-02-28", "2024-03-01", "3"]),
            ("Retire", vec!["65", "11", "66", "1", "3"]),
        ] {
            choose(&mut state, name, &fields);
            assert!(
                matches!(
                    handle_create_parameter(&mut state),
                    ActionResult::Modified(_)
                ),
                "{name}"
            );
        }
        assert_eq!(
            state.analysis_state.sweep_parameters[1].min_value,
            ParameterValue::Rate(0.02)
        );
        state.save_analysis_to_current_scenario();
        let loaded = SimulationData::from_yaml(&state.data().to_yaml().unwrap()).unwrap();
        assert_eq!(
            loaded.analysis.sweep_parameters,
            state.analysis_state.sweep_parameters
        );
        state
            .analysis_state
            .load_from_config(&loaded.analysis, &loaded.named_parameters);
        assert!(matches!(
            handle_run_analysis(&mut state),
            ActionResult::Done(_)
        ));
        let Some(PendingSimulation::SweepAnalysis { sweep_config }) = &state.pending_simulation
        else {
            panic!()
        };
        assert_eq!(sweep_config.total_points(), 81);
        for (index, sweep) in sweep_config.parameters.iter().enumerate() {
            assert!(
                matches!(&sweep.target, SweepTarget::Parameter(p) if p.parameter_id == ParameterId(index as u16))
            );
        }
        let result = AnalysisResults::new(SweepResults::new(
            sweep_config.all_sweep_values(),
            sweep_config.labels(),
            1960,
        ))
        .with_parameters(&state.analysis_state.sweep_parameters);
        assert_eq!(result.param_label(1), "Rate");
        assert_eq!(result.format_param_value(1, 0.04), "4.00%");
        assert_eq!(result.format_param_value(2, 1.0), "2024-02-29");
        assert_eq!(result.format_param_value(3, 792.0), "66y 0m");
    }

    #[test]
    fn invalid_ranges_do_not_create_or_modify_a_sweep() {
        let mut state = state();
        for (name, fields) in [
            ("Spending", vec!["NaN", "1500", "3"]),
            ("Spending", vec!["500", "1500", "1"]),
            ("Rate", vec!["6", "2", "3"]),
            ("Start", vec!["2024-02-30", "2024-03-01", "3"]),
            ("Start", vec!["2024-02-28", "2024-03-01", "4"]),
            ("Retire", vec!["65", "12", "66", "1", "3"]),
        ] {
            choose(&mut state, name, &fields);
            assert!(
                matches!(handle_create_parameter(&mut state), ActionResult::Error(_)),
                "{name}"
            );
            assert!(state.analysis_state.sweep_parameters.is_empty());
        }
        choose(&mut state, "Rate", &["2", "6", "3"]);
        handle_create_parameter(&mut state);
        let before = state.analysis_state.sweep_parameters.clone();
        if let ModalState::Form(form) = &mut state.modal {
            form.fields[1].value = "bad".into();
        }
        assert!(matches!(
            handle_configure_parameter(&mut state, 0),
            ActionResult::Error(_)
        ));
        assert_eq!(state.analysis_state.sweep_parameters, before);
        state.data_mut().named_parameters[1].value = ParameterValue::Money(1.0);
        assert!(matches!(
            handle_run_analysis(&mut state),
            ActionResult::Error(_)
        ));
        assert!(state.pending_simulation.is_none());
    }
}
