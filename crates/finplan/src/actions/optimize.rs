// Optimization actions - parameter management, objective selection, run optimization

use crate::state::{AppState, OptimizeAction, ParameterType, SelectedParameter};

use super::ActionResult;

/// Handle optimization-related actions
pub fn handle_optimize_action(
    state: &mut AppState,
    action: OptimizeAction,
    value: &str,
) -> ActionResult {
    match action {
        OptimizeAction::AddParameter => handle_add_parameter(state, value),
        OptimizeAction::ConfigureParameter { index } => {
            handle_configure_parameter(state, index, value)
        }
        OptimizeAction::DeleteParameter { index } => handle_delete_parameter(state, index),
        OptimizeAction::SelectObjective => handle_select_objective(state, value),
        OptimizeAction::ConfigureSettings => handle_configure_settings(state, value),
        OptimizeAction::RunOptimization => handle_run_optimization(state),
    }
}

fn handle_add_parameter(state: &mut AppState, _value: &str) -> ActionResult {
    // For now, add a default parameter
    state
        .optimize_state
        .selected_parameters
        .push(SelectedParameter {
            param_type: ParameterType::RetirementAge,
            event_id: None,
            account_id: None,
            min_value: 60.0,
            max_value: 70.0,
        });

    ActionResult::Modified(None)
}

fn handle_configure_parameter(state: &mut AppState, index: usize, _value: &str) -> ActionResult {
    // TODO: Show configuration modal
    let _ = (state, index);
    ActionResult::close()
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

fn handle_select_objective(state: &mut AppState, _value: &str) -> ActionResult {
    // TODO: Show objective selection modal
    let _ = state;
    ActionResult::close()
}

fn handle_configure_settings(state: &mut AppState, _value: &str) -> ActionResult {
    // TODO: Show settings modal
    let _ = state;
    ActionResult::close()
}

fn handle_run_optimization(state: &mut AppState) -> ActionResult {
    // TODO: Actually run optimization
    // For now just show a message
    state.set_error("Optimization execution not yet connected".into());
    ActionResult::close()
}
