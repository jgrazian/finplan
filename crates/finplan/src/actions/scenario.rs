// Scenario actions - save, load, switch scenarios

use crate::state::{AppState, MessageModal, ModalState};

use super::{ActionContext, ActionResult};

/// Handle saving a scenario with a new name
pub fn handle_save_as(state: &mut AppState, name: &str) -> ActionResult {
    state.save_scenario_as(name);
    ActionResult::Modified(Some(ModalState::Message(MessageModal::info(
        "Success",
        &format!("Scenario saved as '{}'", name),
    ))))
}

/// Handle loading/switching to a scenario
pub fn handle_load_scenario(state: &mut AppState, name: &str) -> ActionResult {
    if state.app_data.simulations.contains_key(name) {
        state.switch_scenario(name);
        ActionResult::Done(Some(ModalState::Message(MessageModal::info(
            "Success",
            &format!("Switched to scenario '{}'", name),
        ))))
    } else {
        ActionResult::Done(Some(ModalState::Message(MessageModal::error(
            "Error",
            &format!("Scenario '{}' not found", name),
        ))))
    }
}

/// Handle switching to a scenario (silent, no message)
pub fn handle_switch_to(state: &mut AppState, name: &str) -> ActionResult {
    if state.app_data.simulations.contains_key(name) {
        state.switch_scenario(name);
    }
    ActionResult::close()
}

/// Handle editing simulation parameters (start date, birth date, duration)
pub fn handle_edit_parameters(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    // Parse form values: "start_date|birth_date|duration"
    let values: Vec<&str> = ctx.value.split('|').collect();

    if values.len() < 3 {
        return ActionResult::Error("Invalid form data".to_string());
    }

    let start_date = values[0].trim();
    let birth_date = values[1].trim();
    let duration_str = values[2].trim();

    // Validate start_date format (YYYY-MM-DD)
    if !start_date.is_empty() && start_date.parse::<jiff::civil::Date>().is_err() {
        return ActionResult::Error(format!("Invalid start date format: '{}'. Use YYYY-MM-DD", start_date));
    }

    // Validate birth_date format (YYYY-MM-DD)
    if !birth_date.is_empty() && birth_date.parse::<jiff::civil::Date>().is_err() {
        return ActionResult::Error(format!("Invalid birth date format: '{}'. Use YYYY-MM-DD", birth_date));
    }

    // Validate duration is a positive integer
    let duration: usize = match duration_str.parse() {
        Ok(d) if d > 0 => d,
        _ => return ActionResult::Error(format!("Invalid duration: '{}'. Must be a positive number", duration_str)),
    };

    // Update the parameters
    let params = &mut state.data_mut().parameters;
    params.start_date = start_date.to_string();
    params.birth_date = birth_date.to_string();
    params.duration_years = duration;

    // Clear the projection preview since parameters changed
    state.scenario_state.projection_preview = None;

    ActionResult::Modified(Some(ModalState::Message(MessageModal::info(
        "Success",
        "Simulation parameters updated",
    ))))
}
