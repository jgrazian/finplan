// Scenario actions - save, load, switch scenarios

use crate::state::{AppState, MessageModal, ModalState};

use super::ActionResult;

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
