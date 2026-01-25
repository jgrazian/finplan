//! Storage abstraction for platform-independent persistence.
//!
//! This module defines the [`Storage`] trait that abstracts file system operations
//! for both native (std::fs) and web (LocalStorage/IndexedDB) platforms.

use std::collections::HashMap;
use std::path::Path;

use crate::data::app_data::{AppData, SimulationData};
use crate::state::ScenarioSummary;

/// Error types for storage operations
#[derive(Debug)]
pub enum StorageError {
    /// I/O error (file not found, permission denied, etc.)
    Io(String),
    /// Parse error (invalid YAML, corrupted data)
    Parse(String),
    /// Serialization error
    Serialize(String),
    /// Storage not available (e.g., LocalStorage full)
    NotAvailable(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io(msg) => write!(f, "IO error: {}", msg),
            StorageError::Parse(msg) => write!(f, "Parse error: {}", msg),
            StorageError::Serialize(msg) => write!(f, "Serialization error: {}", msg),
            StorageError::NotAvailable(msg) => write!(f, "Storage not available: {}", msg),
        }
    }
}

impl std::error::Error for StorageError {}

/// Result of loading data from storage
pub struct LoadResult {
    /// All loaded application data
    pub app_data: AppData,
    /// The current/active scenario name
    pub current_scenario: String,
    /// Cached scenario summaries (Monte Carlo results)
    pub scenario_summaries: HashMap<String, ScenarioSummary>,
}

/// Platform-independent storage interface.
///
/// This trait abstracts persistence operations so they work on both
/// native (filesystem) and web (browser storage) platforms.
pub trait Storage {
    /// Check if storage has been initialized
    fn exists(&self) -> bool;

    /// Initialize the storage (create directories, etc.)
    fn init(&self) -> Result<(), StorageError>;

    /// Load all scenarios and configuration
    fn load(&self) -> Result<LoadResult, StorageError>;

    /// Save a single scenario
    fn save_scenario(&self, name: &str, data: &SimulationData) -> Result<(), StorageError>;

    /// Save the active scenario name
    fn save_active_scenario(&self, name: &str) -> Result<(), StorageError>;

    /// Delete a scenario
    fn delete_scenario(&self, name: &str) -> Result<(), StorageError>;

    /// Rename a scenario
    fn rename_scenario(&self, old_name: &str, new_name: &str) -> Result<(), StorageError>;

    /// Load scenario summaries (cached Monte Carlo results)
    fn load_summaries(&self) -> Result<HashMap<String, ScenarioSummary>, StorageError>;

    /// Save scenario summaries
    fn save_summaries(
        &self,
        summaries: &HashMap<String, ScenarioSummary>,
    ) -> Result<(), StorageError>;

    /// Export a scenario to an external location
    ///
    /// On native: writes to a file path
    /// On web: triggers a download
    fn export_scenario(
        &self,
        name: &str,
        data: &SimulationData,
        dest: &Path,
    ) -> Result<(), StorageError>;

    /// Import a scenario from an external location
    ///
    /// On native: reads from a file path
    /// On web: reads from uploaded file data
    fn import_scenario(&self, source: &Path) -> Result<(String, SimulationData), StorageError>;

    /// Migrate from old single-file format (native only)
    ///
    /// Returns Ok(true) if migration was performed, Ok(false) if not needed
    fn migrate_from_single_file(&self, old_path: &Path) -> Result<bool, StorageError>;
}
