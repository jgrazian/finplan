//! Per-scenario file storage system (native only)
//!
//! Directory structure:
//! ~/.finplan/
//!   config.yaml          # Active scenario, preferences
//!   summaries.yaml       # Cached scenario summaries (MC results)
//!   scenarios/
//!     retirement.yaml
//!     aggressive.yaml
//!     conservative.yaml

#[cfg(feature = "native")]
use std::collections::HashMap;
#[cfg(feature = "native")]
use std::fs;
#[cfg(feature = "native")]
use std::path::{Path, PathBuf};

#[cfg(feature = "native")]
use crate::state::ScenarioSummary;

#[cfg(feature = "native")]
use super::app_data::{AppData, SimulationData};
use super::keybindings_data::KeybindingsConfig;

/// Configuration stored in config.yaml
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct DataConfig {
    /// The currently active scenario name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_scenario: Option<String>,
}

/// Error types for storage operations
#[derive(Debug)]
pub enum StorageError {
    Io(String),
    Parse(String),
    Serialize(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io(msg) => write!(f, "IO error: {}", msg),
            StorageError::Parse(msg) => write!(f, "Parse error: {}", msg),
            StorageError::Serialize(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

impl std::error::Error for StorageError {}

/// Result of loading data from the storage directory (native only)
#[cfg(feature = "native")]
pub struct LoadResult {
    pub app_data: AppData,
    pub current_scenario: String,
    pub scenario_summaries: HashMap<String, ScenarioSummary>,
    pub keybindings: KeybindingsConfig,
}

/// Manages the data directory for per-scenario file storage (native only)
#[cfg(feature = "native")]
pub struct DataDirectory {
    root: PathBuf,
}

#[cfg(feature = "native")]
impl DataDirectory {
    /// Create a new DataDirectory instance
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Get the default data directory path (~/.finplan/)
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".finplan")
    }

    /// Get the path to config.yaml
    fn config_path(&self) -> PathBuf {
        self.root.join("config.yaml")
    }

    /// Get the path to the scenarios directory
    fn scenarios_dir(&self) -> PathBuf {
        self.root.join("scenarios")
    }

    /// Get the path to summaries.yaml
    fn summaries_path(&self) -> PathBuf {
        self.root.join("summaries.yaml")
    }

    /// Get the path to a specific scenario file
    fn scenario_path(&self, name: &str) -> PathBuf {
        self.scenarios_dir()
            .join(format!("{}.yaml", sanitize_filename(name)))
    }

    /// Check if the data directory exists and has been initialized
    pub fn exists(&self) -> bool {
        self.root.exists() && self.scenarios_dir().exists()
    }

    /// Initialize the data directory structure
    pub fn init(&self) -> Result<(), StorageError> {
        fs::create_dir_all(&self.root)
            .map_err(|e| StorageError::Io(format!("Failed to create data directory: {}", e)))?;
        fs::create_dir_all(self.scenarios_dir()).map_err(|e| {
            StorageError::Io(format!("Failed to create scenarios directory: {}", e))
        })?;
        Ok(())
    }

    /// Load the config file
    fn load_config(&self) -> Result<DataConfig, StorageError> {
        let config_path = self.config_path();
        if !config_path.exists() {
            return Ok(DataConfig::default());
        }

        let content = fs::read_to_string(&config_path)
            .map_err(|e| StorageError::Io(format!("Failed to read config: {}", e)))?;

        serde_saphyr::from_str(&content)
            .map_err(|e| StorageError::Parse(format!("Failed to parse config: {}", e)))
    }

    /// Save the config file
    fn save_config(&self, config: &DataConfig) -> Result<(), StorageError> {
        let yaml = serde_saphyr::to_string(config)
            .map_err(|e| StorageError::Serialize(format!("Failed to serialize config: {}", e)))?;

        fs::write(self.config_path(), yaml)
            .map_err(|e| StorageError::Io(format!("Failed to write config: {}", e)))
    }

    /// Load scenario summaries from summaries.yaml
    pub fn load_summaries(&self) -> Result<HashMap<String, ScenarioSummary>, StorageError> {
        let summaries_path = self.summaries_path();
        if !summaries_path.exists() {
            return Ok(HashMap::new());
        }

        let content = fs::read_to_string(&summaries_path)
            .map_err(|e| StorageError::Io(format!("Failed to read summaries: {}", e)))?;

        serde_saphyr::from_str(&content)
            .map_err(|e| StorageError::Parse(format!("Failed to parse summaries: {}", e)))
    }

    /// Save scenario summaries to summaries.yaml
    pub fn save_summaries(
        &self,
        summaries: &HashMap<String, ScenarioSummary>,
    ) -> Result<(), StorageError> {
        if !self.exists() {
            self.init()?;
        }

        let yaml = serde_saphyr::to_string(summaries).map_err(|e| {
            StorageError::Serialize(format!("Failed to serialize summaries: {}", e))
        })?;

        fs::write(self.summaries_path(), yaml)
            .map_err(|e| StorageError::Io(format!("Failed to write summaries: {}", e)))
    }

    /// Load keybindings from keybindings.yaml
    pub fn load_keybindings(&self) -> KeybindingsConfig {
        KeybindingsConfig::load_or_default(&self.root)
    }

    /// Save keybindings to keybindings.yaml
    pub fn save_keybindings(&self, keybindings: &KeybindingsConfig) -> Result<(), StorageError> {
        keybindings.save(&self.root)
    }

    /// Load all scenarios from the scenarios directory
    pub fn load(&self) -> Result<LoadResult, StorageError> {
        if !self.exists() {
            self.init()?;
        }

        let config = self.load_config()?;
        let mut simulations = HashMap::new();

        // Read all .yaml files from the scenarios directory
        let scenarios_dir = self.scenarios_dir();
        if scenarios_dir.exists() {
            let entries = fs::read_dir(&scenarios_dir).map_err(|e| {
                StorageError::Io(format!("Failed to read scenarios directory: {}", e))
            })?;

            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|ext| ext == "yaml" || ext == "yml")
                    && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                {
                    match self.load_scenario_from_path(&path) {
                        Ok(data) => {
                            simulations.insert(name.to_string(), data);
                        }
                        Err(e) => {
                            tracing::warn!(scenario = name, error = %e, "Failed to load scenario");
                        }
                    }
                }
            }
        }

        // If no scenarios found, create a default one
        if simulations.is_empty() {
            simulations.insert("Default".to_string(), SimulationData::default());
        }

        // Determine the current scenario
        let current_scenario = config
            .active_scenario
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

        // Load keybindings
        let keybindings = self.load_keybindings();

        Ok(LoadResult {
            app_data,
            current_scenario,
            scenario_summaries,
            keybindings,
        })
    }

    /// Load a single scenario from a path
    fn load_scenario_from_path(&self, path: &Path) -> Result<SimulationData, StorageError> {
        let content = fs::read_to_string(path)
            .map_err(|e| StorageError::Io(format!("Failed to read file: {}", e)))?;

        SimulationData::from_yaml(&content)
            .map_err(|e| StorageError::Parse(format!("Failed to parse YAML: {}", e)))
    }

    /// Save a single scenario
    pub fn save_scenario(&self, name: &str, data: &SimulationData) -> Result<(), StorageError> {
        if !self.exists() {
            self.init()?;
        }

        let yaml = data
            .to_yaml()
            .map_err(|e| StorageError::Serialize(format!("Failed to serialize scenario: {}", e)))?;

        let path = self.scenario_path(name);
        fs::write(path, yaml)
            .map_err(|e| StorageError::Io(format!("Failed to write scenario: {}", e)))
    }

    /// Save the active scenario to the config file
    pub fn save_active_scenario(&self, name: &str) -> Result<(), StorageError> {
        let config = DataConfig {
            active_scenario: Some(name.to_string()),
        };
        self.save_config(&config)
    }

    /// Delete a scenario file
    pub fn delete_scenario(&self, name: &str) -> Result<(), StorageError> {
        let path = self.scenario_path(name);
        if path.exists() {
            fs::remove_file(path)
                .map_err(|e| StorageError::Io(format!("Failed to delete scenario: {}", e)))?;
        }
        Ok(())
    }

    /// Rename a scenario (move the file)
    pub fn rename_scenario(&self, old_name: &str, new_name: &str) -> Result<(), StorageError> {
        let old_path = self.scenario_path(old_name);
        let new_path = self.scenario_path(new_name);

        if old_path.exists() {
            fs::rename(old_path, new_path)
                .map_err(|e| StorageError::Io(format!("Failed to rename scenario: {}", e)))?;
        }
        Ok(())
    }

    /// Export a scenario to an external file path
    pub fn export_scenario(
        &self,
        _name: &str,
        data: &SimulationData,
        dest: &Path,
    ) -> Result<(), StorageError> {
        let yaml = data
            .to_yaml()
            .map_err(|e| StorageError::Serialize(format!("Failed to serialize scenario: {}", e)))?;

        fs::write(dest, yaml).map_err(|e| StorageError::Io(format!("Failed to write file: {}", e)))
    }

    /// Import a scenario from an external file path
    pub fn import_scenario(&self, source: &Path) -> Result<(String, SimulationData), StorageError> {
        let content = fs::read_to_string(source)
            .map_err(|e| StorageError::Io(format!("Failed to read file: {}", e)))?;

        let data = SimulationData::from_yaml(&content)
            .map_err(|e| StorageError::Parse(format!("Failed to parse YAML: {}", e)))?;

        // Use the filename (without extension) as the scenario name
        let name = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported")
            .to_string();

        Ok((name, data))
    }

    /// Migrate from old single-file format (~/.finplan.yaml)
    pub fn migrate_from_single_file(&self, old_path: &Path) -> Result<bool, StorageError> {
        if !old_path.exists() {
            return Ok(false);
        }

        // Read the old file
        let content = fs::read_to_string(old_path)
            .map_err(|e| StorageError::Io(format!("Failed to read old config: {}", e)))?;

        // Try to parse as AppData first (multi-scenario format)
        let (simulations, active_scenario) = if let Ok(app_data) = AppData::from_yaml(&content) {
            (app_data.simulations, app_data.active_scenario)
        } else {
            // Fall back to single SimulationData
            let data = SimulationData::from_yaml(&content)
                .map_err(|e| StorageError::Parse(format!("Failed to parse old config: {}", e)))?;

            let name = old_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Default")
                .to_string();

            let mut map = HashMap::new();
            map.insert(name.clone(), data);
            (map, Some(name))
        };

        // Initialize the new directory structure
        self.init()?;

        // Save each scenario to its own file
        for (name, data) in &simulations {
            self.save_scenario(name, data)?;
        }

        // Save the config with the active scenario
        if let Some(active) = active_scenario {
            self.save_active_scenario(&active)?;
        }

        // Create backup of old file
        let backup_path = old_path.with_extension("yaml.backup");
        fs::rename(old_path, backup_path)
            .map_err(|e| StorageError::Io(format!("Failed to create backup: {}", e)))?;

        Ok(true)
    }

    /// Get the root path of the data directory
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Sanitize a filename to be safe for the filesystem
#[cfg(feature = "native")]
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_data_directory_init() {
        let temp_dir = TempDir::new().unwrap();
        let data_dir = DataDirectory::new(temp_dir.path().join(".finplan"));

        assert!(!data_dir.exists());
        data_dir.init().unwrap();
        assert!(data_dir.exists());
    }

    #[test]
    fn test_save_and_load_scenario() {
        let temp_dir = TempDir::new().unwrap();
        let data_dir = DataDirectory::new(temp_dir.path().join(".finplan"));
        data_dir.init().unwrap();

        let mut data = SimulationData::default();
        data.portfolios.name = "Test Portfolio".to_string();

        data_dir.save_scenario("test", &data).unwrap();

        let result = data_dir.load().unwrap();
        assert!(result.app_data.simulations.contains_key("test"));
        assert_eq!(
            result.app_data.simulations["test"].portfolios.name,
            "Test Portfolio"
        );
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("simple"), "simple");
        assert_eq!(sanitize_filename("with spaces"), "with spaces");
        assert_eq!(sanitize_filename("with/slash"), "with_slash");
        assert_eq!(sanitize_filename("test:colon"), "test_colon");
    }
}
