// Scenario actions - save, load, switch scenarios, import, export

#[cfg(feature = "native")]
use std::path::Path;

use crate::modals::{MessageModal, ModalState};
use crate::state::AppState;

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
    // Extract form fields
    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::Error("Invalid form data".to_string()),
    };

    let start_date = form.get_str(0).unwrap_or("").trim();
    let birth_date = form.get_str(1).unwrap_or("").trim();
    let duration_str = form.get_str(2).unwrap_or("").trim();

    // Validate start_date format (YYYY-MM-DD)
    if !start_date.is_empty() && start_date.parse::<jiff::civil::Date>().is_err() {
        return ActionResult::Error(format!(
            "Invalid start date format: '{}'. Use YYYY-MM-DD",
            start_date
        ));
    }

    // Validate birth_date format (YYYY-MM-DD)
    if !birth_date.is_empty() && birth_date.parse::<jiff::civil::Date>().is_err() {
        return ActionResult::Error(format!(
            "Invalid birth date format: '{}'. Use YYYY-MM-DD",
            birth_date
        ));
    }

    // Validate duration is a positive integer
    let duration: usize = match duration_str.parse() {
        Ok(d) if d > 0 => d,
        _ => {
            return ActionResult::Error(format!(
                "Invalid duration: '{}'. Must be a positive number",
                duration_str
            ));
        }
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

/// Handle importing a scenario from an external file (native only - uses filesystem)
#[cfg(feature = "native")]
pub fn handle_import(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let path_str = ctx.selected().unwrap_or_default().trim();
    if path_str.is_empty() {
        return ActionResult::Error("File path cannot be empty".to_string());
    }

    let path = Path::new(path_str);
    if !path.exists() {
        return ActionResult::Error(format!("File not found: {}", path_str));
    }

    match state.import_scenario(path) {
        Ok(name) => {
            // Switch to the imported scenario
            state.switch_scenario(&name);
            ActionResult::Modified(Some(ModalState::Message(MessageModal::info(
                "Imported",
                &format!("Imported scenario as '{}'", name),
            ))))
        }
        Err(e) => ActionResult::Error(format!("Import failed: {}", e)),
    }
}

/// Handle exporting the current scenario to an external file (native only - uses filesystem)
#[cfg(feature = "native")]
pub fn handle_export(state: &AppState, ctx: ActionContext) -> ActionResult {
    let path_str = ctx.selected().unwrap_or_default().trim();
    if path_str.is_empty() {
        return ActionResult::Error("File path cannot be empty".to_string());
    }

    let path = Path::new(path_str);

    match state.export_scenario(path) {
        Ok(()) => ActionResult::Done(Some(ModalState::Message(MessageModal::info(
            "Exported",
            &format!("Scenario exported to {}", path_str),
        )))),
        Err(e) => ActionResult::Error(format!("Export failed: {}", e)),
    }
}

/// Handle creating a new empty scenario
pub fn handle_new_scenario(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let name = ctx.selected().unwrap_or_default().trim();
    if name.is_empty() {
        return ActionResult::Error("Scenario name cannot be empty".to_string());
    }

    // Check if name already exists
    if state.app_data.simulations.contains_key(name) {
        return ActionResult::Error(format!("Scenario '{}' already exists", name));
    }

    state.new_scenario(name);
    state.dirty_scenarios.insert(name.to_string());

    ActionResult::Modified(Some(ModalState::Message(MessageModal::info(
        "Created",
        &format!("Created new scenario '{}'", name),
    ))))
}

/// Handle duplicating an existing scenario
pub fn handle_duplicate_scenario(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    let new_name = ctx.selected().unwrap_or_default().trim();
    if new_name.is_empty() {
        return ActionResult::Error("Scenario name cannot be empty".to_string());
    }

    // Check if name already exists
    if state.app_data.simulations.contains_key(new_name) {
        return ActionResult::Error(format!("Scenario '{}' already exists", new_name));
    }

    // Get the currently selected scenario name from the sorted list
    let scenarios = state.get_scenario_list_with_summaries();
    let source_name = scenarios
        .get(state.scenario_state.selected_index)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| state.current_scenario.clone());

    if state.duplicate_scenario(&source_name, new_name) {
        // Switch to the new scenario
        state.switch_scenario(new_name);
        ActionResult::Modified(Some(ModalState::Message(MessageModal::info(
            "Duplicated",
            &format!("Duplicated '{}' as '{}'", source_name, new_name),
        ))))
    } else {
        ActionResult::Error(format!("Failed to duplicate scenario '{}'", source_name))
    }
}

/// Handle deleting a scenario (confirm dialog already shown)
/// On native, also deletes from disk. On web, only removes from memory.
pub fn handle_delete_scenario(state: &mut AppState) -> ActionResult {
    // Get the selected scenario name
    let scenarios = state.get_scenario_list_with_summaries();
    let selected_name = scenarios
        .get(state.scenario_state.selected_index)
        .map(|(name, _)| name.clone());

    if let Some(name) = selected_name {
        // Try to delete from disk first (native only)
        #[cfg(feature = "native")]
        if let Err(e) = state.delete_scenario_file(&name) {
            // Log but continue - file might not exist
            tracing::warn!(scenario = name, error = %e, "Could not delete scenario file");
        }

        if state.delete_scenario(&name) {
            ActionResult::Modified(Some(ModalState::Message(MessageModal::info(
                "Deleted",
                &format!("Deleted scenario '{}'", name),
            ))))
        } else {
            ActionResult::Error("Cannot delete the last scenario".to_string())
        }
    } else {
        ActionResult::Error("No scenario selected".to_string())
    }
}

/// Handle Monte Carlo with convergence-based stopping
pub fn handle_monte_carlo_convergence(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    use finplan_core::model::ConvergenceMetric;

    let form = match ctx.form() {
        Some(f) => f,
        None => return ActionResult::Error("Invalid form data".to_string()),
    };

    let metric_str = form.get_str(0).unwrap_or("").trim();
    let min_str = form.get_str(1).unwrap_or("").trim();
    let max_str = form.get_str(2).unwrap_or("").trim();
    let threshold_str = form.get_str(3).unwrap_or("").trim();

    // Parse convergence metric
    let metric = match metric_str {
        "Median" => ConvergenceMetric::Median,
        "Success Rate" => ConvergenceMetric::SuccessRate,
        "Percentiles" => ConvergenceMetric::Percentiles,
        "Mean" => ConvergenceMetric::Mean,
        _ => {
            return ActionResult::Error(format!("Unknown convergence metric: '{}'", metric_str));
        }
    };

    // Parse minimum iterations
    let min_iterations: usize = match min_str.parse() {
        Ok(n) if n > 0 => n,
        _ => {
            return ActionResult::Error(format!(
                "Invalid min iterations: '{}'. Must be a positive number",
                min_str
            ));
        }
    };

    // Parse maximum iterations
    let max_iterations: usize = match max_str.parse() {
        Ok(n) if n > min_iterations => n,
        _ => {
            return ActionResult::Error(format!(
                "Invalid max iterations: '{}'. Must be greater than min ({})",
                max_str, min_iterations
            ));
        }
    };

    // Parse convergence threshold (as percentage, convert to decimal)
    let threshold_pct: f64 = match threshold_str.parse() {
        Ok(n) if n > 0.0 && n <= 100.0 => n,
        _ => {
            return ActionResult::Error(format!(
                "Invalid threshold: '{}'. Must be between 0 and 100",
                threshold_str
            ));
        }
    };
    let relative_threshold = threshold_pct / 100.0;

    // Request the convergence-based Monte Carlo simulation
    state.request_monte_carlo_convergence(
        min_iterations,
        max_iterations,
        relative_threshold,
        metric,
    );

    ActionResult::close()
}
