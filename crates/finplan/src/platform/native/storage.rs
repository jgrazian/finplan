//! Native storage implementation using the filesystem.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::data::app_data::SimulationData;
use crate::data::storage::DataDirectory;
use crate::platform::storage::{LoadResult, Storage, StorageError};
use crate::state::ScenarioSummary;

/// Native storage implementation that wraps DataDirectory.
pub struct NativeStorage {
    data_dir: DataDirectory,
}

impl NativeStorage {
    /// Create a new native storage with the given root path.
    pub fn new(root: PathBuf) -> Self {
        Self {
            data_dir: DataDirectory::new(root),
        }
    }

    /// Create native storage with the default path (~/.finplan/).
    pub fn with_default_path() -> Self {
        Self::new(DataDirectory::default_path())
    }

    /// Get the root path of the storage directory.
    pub fn root(&self) -> &Path {
        self.data_dir.root()
    }
}

impl Storage for NativeStorage {
    fn exists(&self) -> bool {
        self.data_dir.exists()
    }

    fn init(&self) -> Result<(), StorageError> {
        self.data_dir.init().map_err(convert_error)
    }

    fn load(&self) -> Result<LoadResult, StorageError> {
        let result = self.data_dir.load().map_err(convert_error)?;
        Ok(LoadResult {
            app_data: result.app_data,
            current_scenario: result.current_scenario,
            scenario_summaries: result.scenario_summaries,
        })
    }

    fn save_scenario(&self, name: &str, data: &SimulationData) -> Result<(), StorageError> {
        self.data_dir
            .save_scenario(name, data)
            .map_err(convert_error)
    }

    fn save_active_scenario(&self, name: &str) -> Result<(), StorageError> {
        self.data_dir
            .save_active_scenario(name)
            .map_err(convert_error)
    }

    fn delete_scenario(&self, name: &str) -> Result<(), StorageError> {
        self.data_dir.delete_scenario(name).map_err(convert_error)
    }

    fn rename_scenario(&self, old_name: &str, new_name: &str) -> Result<(), StorageError> {
        self.data_dir
            .rename_scenario(old_name, new_name)
            .map_err(convert_error)
    }

    fn load_summaries(&self) -> Result<HashMap<String, ScenarioSummary>, StorageError> {
        self.data_dir.load_summaries().map_err(convert_error)
    }

    fn save_summaries(
        &self,
        summaries: &HashMap<String, ScenarioSummary>,
    ) -> Result<(), StorageError> {
        self.data_dir
            .save_summaries(summaries)
            .map_err(convert_error)
    }

    fn export_scenario(
        &self,
        name: &str,
        data: &SimulationData,
        dest: &Path,
    ) -> Result<(), StorageError> {
        self.data_dir
            .export_scenario(name, data, dest)
            .map_err(convert_error)
    }

    fn import_scenario(&self, source: &Path) -> Result<(String, SimulationData), StorageError> {
        self.data_dir.import_scenario(source).map_err(convert_error)
    }

    fn migrate_from_single_file(&self, old_path: &Path) -> Result<bool, StorageError> {
        self.data_dir
            .migrate_from_single_file(old_path)
            .map_err(convert_error)
    }
}

/// Convert from data::storage::StorageError to platform::storage::StorageError
fn convert_error(e: crate::data::storage::StorageError) -> StorageError {
    match e {
        crate::data::storage::StorageError::Io(msg) => StorageError::Io(msg),
        crate::data::storage::StorageError::Parse(msg) => StorageError::Parse(msg),
        crate::data::storage::StorageError::Serialize(msg) => StorageError::Serialize(msg),
    }
}
