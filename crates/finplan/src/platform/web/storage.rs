//! Web storage implementation using browser LocalStorage.

use std::collections::HashMap;
use std::path::Path;

use gloo_storage::{LocalStorage, Storage as GlooStorage};

use crate::data::app_data::{AppData, SimulationData};
use crate::platform::storage::{LoadResult, Storage, StorageError};
use crate::state::ScenarioSummary;

/// Key prefix for scenario data in LocalStorage
const SCENARIO_PREFIX: &str = "finplan_scenario_";
/// Key for the list of scenario names
const SCENARIO_INDEX_KEY: &str = "finplan_scenarios";
/// Key for the active scenario name
const ACTIVE_SCENARIO_KEY: &str = "finplan_active";
/// Key for cached scenario summaries
const SUMMARIES_KEY: &str = "finplan_summaries";

/// Web storage implementation using browser LocalStorage.
///
/// Data is stored as JSON in the browser's LocalStorage API.
/// Each scenario is stored separately with a prefixed key.
pub struct WebStorage {
    /// Whether storage has been initialized
    initialized: bool,
}

impl WebStorage {
    /// Create a new web storage instance.
    pub fn new() -> Self {
        Self { initialized: false }
    }

    /// Get the list of scenario names from the index.
    fn get_scenario_index() -> Vec<String> {
        LocalStorage::get(SCENARIO_INDEX_KEY).unwrap_or_default()
    }

    /// Save the scenario index.
    fn save_scenario_index(names: &[String]) -> Result<(), StorageError> {
        LocalStorage::set(SCENARIO_INDEX_KEY, names)
            .map_err(|e| StorageError::Io(format!("Failed to save scenario index: {}", e)))
    }

    /// Get the storage key for a scenario.
    fn scenario_key(name: &str) -> String {
        format!("{}{}", SCENARIO_PREFIX, name)
    }
}

impl Default for WebStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Storage for WebStorage {
    fn exists(&self) -> bool {
        // Check if we have any scenarios stored
        !Self::get_scenario_index().is_empty() || self.initialized
    }

    fn init(&self) -> Result<(), StorageError> {
        // LocalStorage doesn't need initialization, but we can set up defaults
        if Self::get_scenario_index().is_empty() {
            // Create default scenario
            let default_data = SimulationData::default();
            let key = Self::scenario_key("Default");
            LocalStorage::set(&key, &default_data).map_err(|e| {
                StorageError::Io(format!("Failed to create default scenario: {}", e))
            })?;
            Self::save_scenario_index(&["Default".to_string()])?;
        }
        Ok(())
    }

    fn load(&self) -> Result<LoadResult, StorageError> {
        let scenario_names = Self::get_scenario_index();
        let mut simulations = HashMap::new();

        // Load each scenario
        for name in &scenario_names {
            let key = Self::scenario_key(name);
            match LocalStorage::get::<SimulationData>(&key) {
                Ok(data) => {
                    simulations.insert(name.clone(), data);
                }
                Err(e) => {
                    tracing::warn!(scenario = name, error = %e, "Failed to load scenario from LocalStorage");
                }
            }
        }

        // If no scenarios found, create a default one
        if simulations.is_empty() {
            simulations.insert("Default".to_string(), SimulationData::default());
            // Persist the default
            let _ = self.save_scenario("Default", &SimulationData::default());
        }

        // Get the active scenario
        let active_scenario: Option<String> = LocalStorage::get(ACTIVE_SCENARIO_KEY).ok();
        let current_scenario = active_scenario
            .filter(|s| simulations.contains_key(s))
            .or_else(|| simulations.get("Default").map(|_| "Default".to_string()))
            .or_else(|| simulations.keys().next().cloned())
            .unwrap_or_else(|| "Default".to_string());

        let app_data = AppData {
            active_scenario: Some(current_scenario.clone()),
            simulations,
        };

        // Load cached summaries
        let scenario_summaries = self.load_summaries().unwrap_or_default();

        Ok(LoadResult {
            app_data,
            current_scenario,
            scenario_summaries,
        })
    }

    fn save_scenario(&self, name: &str, data: &SimulationData) -> Result<(), StorageError> {
        let key = Self::scenario_key(name);
        LocalStorage::set(&key, data)
            .map_err(|e| StorageError::Io(format!("Failed to save scenario: {}", e)))?;

        // Update the index if this is a new scenario
        let mut names = Self::get_scenario_index();
        if !names.contains(&name.to_string()) {
            names.push(name.to_string());
            Self::save_scenario_index(&names)?;
        }

        Ok(())
    }

    fn save_active_scenario(&self, name: &str) -> Result<(), StorageError> {
        LocalStorage::set(ACTIVE_SCENARIO_KEY, name)
            .map_err(|e| StorageError::Io(format!("Failed to save active scenario: {}", e)))
    }

    fn delete_scenario(&self, name: &str) -> Result<(), StorageError> {
        let key = Self::scenario_key(name);
        LocalStorage::delete(&key);

        // Update the index
        let mut names = Self::get_scenario_index();
        names.retain(|n| n != name);
        Self::save_scenario_index(&names)?;

        Ok(())
    }

    fn rename_scenario(&self, old_name: &str, new_name: &str) -> Result<(), StorageError> {
        // Load the old scenario
        let old_key = Self::scenario_key(old_name);
        let data: SimulationData = LocalStorage::get(&old_key)
            .map_err(|e| StorageError::Io(format!("Failed to load scenario for rename: {}", e)))?;

        // Save under new name
        let new_key = Self::scenario_key(new_name);
        LocalStorage::set(&new_key, &data)
            .map_err(|e| StorageError::Io(format!("Failed to save renamed scenario: {}", e)))?;

        // Delete old key
        LocalStorage::delete(&old_key);

        // Update the index
        let mut names = Self::get_scenario_index();
        if let Some(pos) = names.iter().position(|n| n == old_name) {
            names[pos] = new_name.to_string();
        }
        Self::save_scenario_index(&names)?;

        Ok(())
    }

    fn load_summaries(&self) -> Result<HashMap<String, ScenarioSummary>, StorageError> {
        LocalStorage::get(SUMMARIES_KEY)
            .map_err(|e| StorageError::Io(format!("Failed to load summaries: {}", e)))
    }

    fn save_summaries(
        &self,
        summaries: &HashMap<String, ScenarioSummary>,
    ) -> Result<(), StorageError> {
        LocalStorage::set(SUMMARIES_KEY, summaries)
            .map_err(|e| StorageError::Io(format!("Failed to save summaries: {}", e)))
    }

    fn export_scenario(
        &self,
        _name: &str,
        data: &SimulationData,
        _dest: &Path,
    ) -> Result<(), StorageError> {
        // In web, we trigger a download instead of writing to a file path
        // For now, we'll serialize to YAML and use the web-sys download API
        let yaml = data
            .to_yaml()
            .map_err(|e| StorageError::Serialize(format!("Failed to serialize scenario: {}", e)))?;

        // TODO: Implement browser download using web-sys
        // For now, log the YAML (user can copy from console)
        tracing::info!("Export scenario YAML:\n{}", yaml);

        Ok(())
    }

    fn import_scenario(&self, _source: &Path) -> Result<(String, SimulationData), StorageError> {
        // In web, import would come from a file input element, not a path
        // This would be handled by JavaScript/wasm-bindgen integration
        Err(StorageError::NotAvailable(
            "Web import requires file input element - use import_from_yaml() instead".to_string(),
        ))
    }

    fn migrate_from_single_file(&self, _old_path: &Path) -> Result<bool, StorageError> {
        // No migration needed on web - there's no old file format
        Ok(false)
    }
}

impl WebStorage {
    /// Import a scenario from YAML string (for web file input handling).
    pub fn import_from_yaml(&self, yaml: &str, name: &str) -> Result<SimulationData, StorageError> {
        let data = SimulationData::from_yaml(yaml)
            .map_err(|e| StorageError::Parse(format!("Failed to parse YAML: {}", e)))?;

        self.save_scenario(name, &data)?;

        Ok(data)
    }
}
