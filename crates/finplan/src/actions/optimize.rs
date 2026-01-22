// Optimization actions - parameter management, objective selection, run optimization

use finplan_core::model::{AccountId, EventId};
use finplan_core::optimization::{
    OptimizableParameter, OptimizationAlgorithm, OptimizationConfig, OptimizationConstraints,
    OptimizationObjective,
};

use crate::state::context::{ModalContext, OptimizeContext};
use crate::state::{
    AppState, FieldType, FormField, FormModal, ModalState, OptimizationObjectiveSelection,
    OptimizationResultDisplay, OptimizeAction, ParameterType, PickerModal, SelectedParameter,
};

use super::ActionResult;

/// Handle optimization-related actions
pub fn handle_optimize_action(
    state: &mut AppState,
    action: OptimizeAction,
    value: &str,
) -> ActionResult {
    match action {
        OptimizeAction::AddParameter => handle_add_parameter(state, value),
        OptimizeAction::ConfigureParameter { index } => handle_configure_parameter(state, index),
        OptimizeAction::DeleteParameter { index } => handle_delete_parameter(state, index),
        OptimizeAction::SelectObjective => handle_select_objective(state, value),
        OptimizeAction::ConfigureSettings => handle_configure_settings(state, value),
        OptimizeAction::RunOptimization => handle_run_optimization(state),
    }
}

/// Show picker to add a new parameter, or handle selection if value is provided
fn handle_add_parameter(state: &mut AppState, value: &str) -> ActionResult {
    // If value is provided, we're handling a selection
    if !value.is_empty() {
        return handle_parameter_type_selected(state, value);
    }

    // Otherwise show the picker
    let options = vec![
        "Retirement Age".to_string(),
        "Contribution Rate".to_string(),
        "Withdrawal Amount".to_string(),
        "Asset Allocation".to_string(),
    ];

    let picker = PickerModal::new(
        "Select Parameter Type",
        options,
        crate::state::ModalAction::ADD_OPTIMIZE_PARAMETER,
    )
    .with_typed_context(ModalContext::Optimize(OptimizeContext::Parameter {
        index: state.optimize_state.selected_parameters.len(),
    }));

    ActionResult::modal(ModalState::Picker(picker))
}

/// Handle parameter type selection and show configuration form
pub fn handle_parameter_type_selected(state: &mut AppState, type_name: &str) -> ActionResult {
    let param_type = match type_name {
        "Retirement Age" => ParameterType::RetirementAge,
        "Contribution Rate" => ParameterType::ContributionRate,
        "Withdrawal Amount" => ParameterType::WithdrawalAmount,
        "Asset Allocation" => ParameterType::AssetAllocation,
        _ => return ActionResult::error("Unknown parameter type"),
    };

    // Build the configuration form based on parameter type
    let (fields, title) = match param_type {
        ParameterType::RetirementAge => {
            let event_options = get_event_options(state);
            let fields = vec![
                FormField::select("Event", event_options, ""),
                FormField::new("Min Age", FieldType::Text, "60"),
                FormField::new("Max Age", FieldType::Text, "70"),
            ];
            (fields, "Configure Retirement Age")
        }
        ParameterType::ContributionRate | ParameterType::WithdrawalAmount => {
            let event_options = get_event_options(state);
            let label = if param_type == ParameterType::ContributionRate {
                "Configure Contribution Rate"
            } else {
                "Configure Withdrawal Amount"
            };
            let fields = vec![
                FormField::select("Event", event_options, ""),
                FormField::currency("Min Amount", 0.0),
                FormField::currency("Max Amount", 50000.0),
            ];
            (fields, label)
        }
        ParameterType::AssetAllocation => {
            let account_options = get_investment_account_options(state);
            let fields = vec![
                FormField::select("Account", account_options, ""),
                FormField::percentage("Min Stock %", 0.2),
                FormField::percentage("Max Stock %", 0.8),
            ];
            (fields, "Configure Asset Allocation")
        }
    };

    let next_idx = state.optimize_state.selected_parameters.len();
    let action =
        crate::state::ModalAction::Optimize(OptimizeAction::ConfigureParameter { index: next_idx });
    let form = FormModal::new(title, fields, action)
        .with_typed_context(ModalContext::Optimize(OptimizeContext::Parameter {
            index: next_idx,
        }))
        .start_editing();

    // Store the param type temporarily in a new parameter
    state
        .optimize_state
        .selected_parameters
        .push(SelectedParameter {
            param_type,
            event_id: None,
            account_id: None,
            min_value: 0.0,
            max_value: 100.0,
        });

    ActionResult::modal(ModalState::Form(form))
}

/// Handle parameter configuration form submission
fn handle_configure_parameter(state: &mut AppState, index: usize) -> ActionResult {
    // This is called when the configuration form is submitted
    // Extract values from form before modifying state to avoid borrow issues

    // Get the form data from the modal
    let updates = if let ModalState::Form(ref form) = state.modal {
        let values = form.values();

        if let Some(param) = state.optimize_state.selected_parameters.get(index) {
            match param.param_type {
                ParameterType::RetirementAge => {
                    let event_name = values.str(0).to_string();
                    let event_id = find_event_id_by_name(state.data(), &event_name);
                    let min = values.int(1, 60) as f64;
                    let max = values.int(2, 70) as f64;
                    Some((param.param_type, event_id, None, min, max))
                }
                ParameterType::ContributionRate | ParameterType::WithdrawalAmount => {
                    let event_name = values.str(0).to_string();
                    let event_id = find_event_id_by_name(state.data(), &event_name);
                    let min = values.currency(1, 0.0);
                    let max = values.currency(2, 50000.0);
                    Some((param.param_type, event_id, None, min, max))
                }
                ParameterType::AssetAllocation => {
                    let account_name = values.str(0).to_string();
                    let account_id = find_account_id_by_name(state.data(), &account_name);
                    let min = values.percentage(1, 0.2);
                    let max = values.percentage(2, 0.8);
                    Some((param.param_type, None, account_id, min, max))
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Apply updates
    if let Some((_param_type, event_id, account_id, min_value, max_value)) = updates
        && let Some(param) = state.optimize_state.selected_parameters.get_mut(index)
    {
        param.event_id = event_id;
        param.account_id = account_id;
        param.min_value = min_value;
        param.max_value = max_value;
    }

    ActionResult::Modified(None)
}

/// Find event ID by name from simulation data
fn find_event_id_by_name(
    data: &crate::data::app_data::SimulationData,
    name: &str,
) -> Option<EventId> {
    data.events
        .iter()
        .position(|e| e.name.0 == name)
        .map(|idx| EventId((idx + 1) as u16))
}

/// Find account ID by name from simulation data
fn find_account_id_by_name(
    data: &crate::data::app_data::SimulationData,
    name: &str,
) -> Option<AccountId> {
    data.portfolios
        .accounts
        .iter()
        .position(|a| a.name == name)
        .map(|idx| AccountId((idx + 1) as u16))
}

fn handle_delete_parameter(state: &mut AppState, index: usize) -> ActionResult {
    if index < state.optimize_state.selected_parameters.len() {
        state.optimize_state.selected_parameters.remove(index);
        if state.optimize_state.selected_param_index > 0
            && state.optimize_state.selected_param_index
                >= state.optimize_state.selected_parameters.len()
        {
            state.optimize_state.selected_param_index = state
                .optimize_state
                .selected_parameters
                .len()
                .saturating_sub(1);
        }
        ActionResult::Modified(None)
    } else {
        ActionResult::close()
    }
}

/// Show objective selection picker
pub fn show_objective_picker(_state: &mut AppState) -> ActionResult {
    let options = vec![
        "Maximize Wealth at Death".to_string(),
        "Maximize Wealth at Retirement".to_string(),
        "Maximize Sustainable Withdrawal".to_string(),
        "Minimize Lifetime Taxes".to_string(),
    ];

    let picker = PickerModal::new(
        "Select Objective",
        options,
        crate::state::ModalAction::SELECT_OBJECTIVE,
    )
    .with_typed_context(ModalContext::Optimize(OptimizeContext::Objective));

    ActionResult::modal(ModalState::Picker(picker))
}

fn handle_select_objective(state: &mut AppState, value: &str) -> ActionResult {
    match value {
        "Maximize Wealth at Death" => {
            state.optimize_state.objective = OptimizationObjectiveSelection::MaxWealthAtDeath;
            ActionResult::Modified(None)
        }
        "Maximize Wealth at Retirement" => {
            // Show event picker for retirement event
            let event_options = get_event_options(state);
            let fields = vec![FormField::select("Retirement Event", event_options, "")];
            let form = FormModal::new(
                "Configure Retirement Objective",
                fields,
                crate::state::ModalAction::SELECT_OBJECTIVE,
            )
            .with_typed_context(ModalContext::Optimize(OptimizeContext::Objective))
            .start_editing();
            ActionResult::modal(ModalState::Form(form))
        }
        "Maximize Sustainable Withdrawal" => {
            // Show form for withdrawal event and target success rate
            let event_options = get_event_options(state);
            let fields = vec![
                FormField::select("Withdrawal Event", event_options, ""),
                FormField::percentage("Target Success Rate", 0.95),
            ];
            let form = FormModal::new(
                "Configure Sustainable Withdrawal",
                fields,
                crate::state::ModalAction::SELECT_OBJECTIVE,
            )
            .with_typed_context(ModalContext::Optimize(OptimizeContext::Objective))
            .start_editing();
            ActionResult::modal(ModalState::Form(form))
        }
        "Minimize Lifetime Taxes" => {
            state.optimize_state.objective = OptimizationObjectiveSelection::MinLifetimeTax;
            ActionResult::Modified(None)
        }
        _ => {
            // This is a form submission - parse the form
            if let ModalState::Form(ref form) = state.modal {
                let title = &form.title;
                let values = form.values();

                if title.contains("Retirement") {
                    let event_name = values.str(0);
                    state.optimize_state.objective =
                        OptimizationObjectiveSelection::MaxWealthAtRetirement {
                            event_id: find_event_id(state, event_name),
                        };
                } else if title.contains("Withdrawal") {
                    let event_name = values.str(0);
                    let success_rate = values.percentage(1, 0.95);
                    state.optimize_state.objective =
                        OptimizationObjectiveSelection::MaxSustainableWithdrawal {
                            event_id: find_event_id(state, event_name),
                            success_rate,
                        };
                }
            }
            ActionResult::Modified(None)
        }
    }
}

/// Show settings configuration form
pub fn show_settings_form(state: &mut AppState) -> ActionResult {
    let algorithm_options = vec![
        "Auto".to_string(),
        "Binary Search".to_string(),
        "Grid Search".to_string(),
        "Nelder-Mead".to_string(),
    ];

    let fields = vec![
        FormField::new(
            "Monte Carlo Iterations",
            FieldType::Text,
            &state.optimize_state.mc_iterations.to_string(),
        ),
        FormField::new(
            "Max Optimization Iterations",
            FieldType::Text,
            &state.optimize_state.max_iterations.to_string(),
        ),
        FormField::select("Algorithm", algorithm_options, "Auto"),
        FormField::percentage("Min Success Rate Constraint", 0.90),
    ];

    let form = FormModal::new(
        "Optimization Settings",
        fields,
        crate::state::ModalAction::CONFIGURE_OPTIMIZE_SETTINGS,
    )
    .with_typed_context(ModalContext::Optimize(OptimizeContext::Settings))
    .start_editing();

    ActionResult::modal(ModalState::Form(form))
}

fn handle_configure_settings(state: &mut AppState, _value: &str) -> ActionResult {
    // Parse form values
    if let ModalState::Form(ref form) = state.modal {
        let values = form.values();

        state.optimize_state.mc_iterations = values.int(0, 500);
        state.optimize_state.max_iterations = values.int(1, 100);
        // Algorithm is stored in field 2 but we use Auto for now
        // Min success rate constraint in field 3 - could be used for constraints
    }

    ActionResult::Modified(None)
}

fn handle_run_optimization(state: &mut AppState) -> ActionResult {
    // Validate we have parameters
    if state.optimize_state.selected_parameters.is_empty() {
        return ActionResult::error(
            "No parameters selected for optimization. Press 'a' to add parameters.",
        );
    }

    // Build the optimization config
    let opt_config = match build_optimization_config(state) {
        Ok(config) => config,
        Err(e) => return ActionResult::error(e),
    };

    // Build simulation config
    let sim_config = match state.to_simulation_config() {
        Ok(config) => config,
        Err(e) => return ActionResult::error(format!("Failed to build simulation config: {}", e)),
    };

    // Mark as running
    state.optimize_state.running = true;
    state.optimize_state.current_iteration = 0;
    state.optimize_state.convergence_data.clear();
    state.optimize_state.result = None;

    // Run the optimization (blocking - progress is tracked via result history)
    let result = finplan_core::optimization::optimize(&sim_config, &opt_config, None);

    // Update state with results
    state.optimize_state.running = false;

    match result {
        Ok(opt_result) => {
            // Convert to display format
            let optimal_values: Vec<(String, f64)> = opt_result
                .optimal_parameters
                .iter()
                .map(|(name, value)| (format_param_name(name), *value))
                .collect();

            // Build convergence data from history
            let convergence_data: Vec<(usize, f64)> = opt_result
                .history
                .best_values
                .iter()
                .enumerate()
                .map(|(i, v)| (i + 1, *v))
                .collect();

            state.optimize_state.result = Some(OptimizationResultDisplay {
                optimal_values,
                objective_value: opt_result.objective_value,
                success_rate: opt_result.optimal_stats.success_rate,
                converged: opt_result.converged,
                iterations: opt_result.iterations,
            });

            state.optimize_state.convergence_data = convergence_data;
            state.optimize_state.current_iteration = opt_result.iterations;

            ActionResult::Done(None)
        }
        Err(e) => ActionResult::error(format!("Optimization failed: {}", e)),
    }
}

/// Build OptimizationConfig from TUI state
fn build_optimization_config(state: &AppState) -> Result<OptimizationConfig, String> {
    // Convert parameters
    let parameters: Vec<OptimizableParameter> = state
        .optimize_state
        .selected_parameters
        .iter()
        .filter_map(|param| convert_parameter(state, param))
        .collect();

    if parameters.is_empty() {
        return Err(
            "No valid parameters configured. Make sure events/accounts are selected.".to_string(),
        );
    }

    // Convert objective
    let objective = convert_objective(state)?;

    // Build constraints
    let constraints = OptimizationConstraints {
        min_success_rate: Some(0.90),
        min_final_net_worth: None,
        max_withdrawal_rate: None,
    };

    // Determine algorithm
    let algorithm = OptimizationAlgorithm::Auto;

    Ok(OptimizationConfig {
        objective,
        parameters,
        constraints,
        algorithm,
        monte_carlo_iterations: state.optimize_state.mc_iterations,
        max_iterations: state.optimize_state.max_iterations,
        tolerance: 0.001,
    })
}

/// Convert TUI SelectedParameter to core OptimizableParameter
fn convert_parameter(state: &AppState, param: &SelectedParameter) -> Option<OptimizableParameter> {
    match param.param_type {
        ParameterType::RetirementAge => {
            let event_id = param.event_id.or_else(|| find_first_event_id(state))?;
            Some(OptimizableParameter::RetirementAge {
                event_id,
                min_age: param.min_value as u8,
                max_age: param.max_value as u8,
            })
        }
        ParameterType::ContributionRate => {
            let event_id = param.event_id.or_else(|| find_first_event_id(state))?;
            Some(OptimizableParameter::ContributionRate {
                event_id,
                min_amount: param.min_value,
                max_amount: param.max_value,
            })
        }
        ParameterType::WithdrawalAmount => {
            let event_id = param.event_id.or_else(|| find_first_event_id(state))?;
            Some(OptimizableParameter::WithdrawalAmount {
                event_id,
                min_amount: param.min_value,
                max_amount: param.max_value,
            })
        }
        ParameterType::AssetAllocation => {
            let account_id = param.account_id.or_else(|| find_first_account_id(state))?;
            Some(OptimizableParameter::AssetAllocation {
                account_id,
                min_stock_pct: param.min_value,
                max_stock_pct: param.max_value,
            })
        }
    }
}

/// Convert TUI objective selection to core OptimizationObjective
fn convert_objective(state: &AppState) -> Result<OptimizationObjective, String> {
    match &state.optimize_state.objective {
        OptimizationObjectiveSelection::MaxWealthAtDeath => {
            Ok(OptimizationObjective::MaximizeWealthAtDeath)
        }
        OptimizationObjectiveSelection::MaxWealthAtRetirement { event_id } => {
            let id = event_id
                .or_else(|| find_first_event_id(state))
                .ok_or_else(|| "No retirement event configured".to_string())?;
            Ok(OptimizationObjective::MaximizeWealthAtRetirement {
                retirement_event_id: id,
            })
        }
        OptimizationObjectiveSelection::MaxSustainableWithdrawal {
            event_id,
            success_rate,
        } => {
            let id = event_id
                .or_else(|| find_first_event_id(state))
                .ok_or_else(|| "No withdrawal event configured".to_string())?;
            Ok(OptimizationObjective::MaximizeSustainableWithdrawal {
                withdrawal_event_id: id,
                target_success_rate: *success_rate,
            })
        }
        OptimizationObjectiveSelection::MinLifetimeTax => {
            Ok(OptimizationObjective::MinimizeLifetimeTax)
        }
    }
}

// Helper functions

fn get_event_options(state: &AppState) -> Vec<String> {
    state
        .data()
        .events
        .iter()
        .map(|e| e.name.0.clone())
        .collect()
}

fn get_investment_account_options(state: &AppState) -> Vec<String> {
    state
        .data()
        .portfolios
        .accounts
        .iter()
        .filter(|a| a.account_type.can_hold_assets())
        .map(|a| a.name.clone())
        .collect()
}

fn find_event_id(state: &AppState, name: &str) -> Option<EventId> {
    state
        .data()
        .events
        .iter()
        .position(|e| e.name.0 == name)
        .map(|idx| EventId((idx + 1) as u16))
}

fn find_first_event_id(state: &AppState) -> Option<EventId> {
    if state.data().events.is_empty() {
        None
    } else {
        Some(EventId(1))
    }
}

fn find_first_account_id(state: &AppState) -> Option<AccountId> {
    if state.data().portfolios.accounts.is_empty() {
        None
    } else {
        Some(AccountId(1))
    }
}

fn format_param_name(name: &str) -> String {
    // Convert "RetirementAge(event_1)" to "Retirement Age"
    if name.starts_with("RetirementAge") {
        "Retirement Age".to_string()
    } else if name.starts_with("ContributionRate") {
        "Contribution Rate".to_string()
    } else if name.starts_with("WithdrawalAmount") {
        "Withdrawal Amount".to_string()
    } else if name.starts_with("AssetAllocation") {
        "Asset Allocation".to_string()
    } else {
        name.to_string()
    }
}
