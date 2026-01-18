use std::path::PathBuf;

use finplan_core::config::SimulationConfig;
use rand::RngCore;

use crate::data::app_data::{AppData, SimulationData};
use crate::data::convert::{to_simulation_config, to_tui_result, ConvertError};

use super::errors::{LoadError, SaveError, SimulationError};
use super::modal::ModalState;
use super::screen_state::{EventsState, PortfolioProfilesState, ResultsState, ScenarioState};
use super::tabs::TabId;

// ========== SimulationResult ==========
// Simplified result structure for TUI display

#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub final_net_worth: f64,
    pub years: Vec<YearResult>,
}

#[derive(Debug, Clone)]
pub struct YearResult {
    pub year: i32,
    pub age: u8,
    pub net_worth: f64,
    pub income: f64,
    pub expenses: f64,
    pub taxes: f64,
}

// ========== AppState ==========
// Main application state

#[derive(Debug)]
pub struct AppState {
    pub active_tab: TabId,
    /// All simulation scenarios
    pub app_data: AppData,
    /// Current scenario name being edited
    pub current_scenario: String,
    /// Path to the config file (if loaded from file)
    pub config_path: Option<PathBuf>,
    /// Cached simulation config (rebuilt when running simulation)
    cached_config: Option<SimulationConfig>,
    pub simulation_result: Option<SimulationResult>,
    /// Core simulation result (needed for ledger and wealth snapshots)
    pub core_simulation_result: Option<finplan_core::model::SimulationResult>,

    // Per-screen state
    pub portfolio_profiles_state: PortfolioProfilesState,
    pub events_state: EventsState,
    pub scenario_state: ScenarioState,
    pub results_state: ResultsState,

    pub modal: ModalState,
    pub error_message: Option<String>,
    pub exit: bool,
}

impl Default for AppState {
    fn default() -> Self {
        let default_name = "Default".to_string();
        let mut app_data = AppData::new();
        app_data
            .simulations
            .insert(default_name.clone(), SimulationData::default());

        Self {
            active_tab: TabId::PortfolioProfiles,
            app_data,
            current_scenario: default_name,
            config_path: None,
            cached_config: None,
            simulation_result: None,
            core_simulation_result: None,
            portfolio_profiles_state: PortfolioProfilesState::default(),
            events_state: EventsState::default(),
            scenario_state: ScenarioState::default(),
            results_state: ResultsState::default(),
            modal: ModalState::None,
            error_message: None,
            exit: false,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current simulation data (convenience accessor)
    pub fn data(&self) -> &SimulationData {
        self.app_data
            .simulations
            .get(&self.current_scenario)
            .expect("Current scenario should always exist")
    }

    /// Get mutable reference to current simulation data
    pub fn data_mut(&mut self) -> &mut SimulationData {
        self.app_data
            .simulations
            .get_mut(&self.current_scenario)
            .expect("Current scenario should always exist")
    }

    /// Get list of all scenario names
    pub fn scenario_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.app_data.simulations.keys().cloned().collect();
        names.sort();
        names
    }

    /// Switch to a different scenario
    pub fn switch_scenario(&mut self, name: &str) {
        if self.app_data.simulations.contains_key(name) {
            self.current_scenario = name.to_string();
            self.simulation_result = None;
            self.core_simulation_result = None;
            self.invalidate_config_cache();
        }
    }

    /// Save current scenario with a new name (copy)
    pub fn save_scenario_as(&mut self, name: &str) {
        let data = self.data().clone();
        self.app_data.simulations.insert(name.to_string(), data);
        self.current_scenario = name.to_string();
    }

    /// Create a new empty scenario
    pub fn new_scenario(&mut self, name: &str) {
        self.app_data
            .simulations
            .insert(name.to_string(), SimulationData::default());
        self.current_scenario = name.to_string();
        self.simulation_result = None;
        self.core_simulation_result = None;
        self.invalidate_config_cache();
    }

    /// Load AppData from a YAML file
    pub fn load_from_file(path: PathBuf) -> Result<Self, LoadError> {
        let content =
            std::fs::read_to_string(&path).map_err(|e| LoadError::Io(e.to_string()))?;

        // Try to parse as AppData first, fall back to SimulationData
        let (app_data, current_scenario) = if let Ok(app_data) = AppData::from_yaml(&content) {
            // Use active_scenario if set, otherwise fall back to "Default" or first available
            let scenario = app_data
                .active_scenario
                .clone()
                .filter(|s| app_data.simulations.contains_key(s))
                .or_else(|| {
                    app_data
                        .simulations
                        .get("Default")
                        .map(|_| "Default".to_string())
                })
                .or_else(|| app_data.simulations.keys().next().cloned())
                .unwrap_or_else(|| "Default".to_string());
            (app_data, scenario)
        } else {
            // Fall back to loading as single SimulationData
            let data = SimulationData::from_yaml(&content)
                .map_err(|e| LoadError::Parse(e.to_string()))?;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Imported")
                .to_string();
            let mut app_data = AppData::new();
            app_data.simulations.insert(name.clone(), data);
            (app_data, name)
        };

        Ok(Self {
            app_data,
            current_scenario,
            config_path: Some(path),
            ..Default::default()
        })
    }

    /// Save all scenarios to YAML file
    pub fn save_to_file(&mut self, path: &PathBuf) -> Result<(), SaveError> {
        // Update active_scenario before saving
        self.app_data.active_scenario = Some(self.current_scenario.clone());
        let yaml = self
            .app_data
            .to_yaml()
            .map_err(|e| SaveError::Serialize(e.to_string()))?;
        std::fs::write(path, yaml).map_err(|e| SaveError::Io(e.to_string()))?;
        Ok(())
    }

    /// Save to current config path (if set)
    pub fn save(&mut self) -> Result<(), SaveError> {
        match self.config_path.clone() {
            Some(path) => self.save_to_file(&path),
            None => Err(SaveError::NoPath),
        }
    }

    /// Convert current data to SimulationConfig for running simulation
    pub fn to_simulation_config(&self) -> Result<SimulationConfig, ConvertError> {
        to_simulation_config(self.data())
    }

    /// Get or build the cached simulation config
    pub fn get_or_build_config(&mut self) -> Result<&SimulationConfig, ConvertError> {
        if self.cached_config.is_none() {
            self.cached_config = Some(self.to_simulation_config()?);
        }
        Ok(self.cached_config.as_ref().unwrap())
    }

    /// Invalidate the cached config (call after modifying data)
    pub fn invalidate_config_cache(&mut self) {
        self.cached_config = None;
    }

    /// Mark data as modified (invalidates cache)
    pub fn mark_modified(&mut self) {
        self.invalidate_config_cache();
    }

    pub fn switch_tab(&mut self, tab: TabId) {
        self.active_tab = tab;
    }

    pub fn next_tab(&mut self) {
        let current_index = self.active_tab.index();
        let next_index = (current_index + 1) % TabId::ALL.len();
        self.active_tab = TabId::from_index(next_index).unwrap();
    }

    pub fn prev_tab(&mut self) {
        let current_index = self.active_tab.index();
        let next_index = if current_index == 0 {
            TabId::ALL.len() - 1
        } else {
            current_index - 1
        };
        self.active_tab = TabId::from_index(next_index).unwrap();
    }

    pub fn set_error(&mut self, message: String) {
        self.error_message = Some(message);
    }

    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    /// Run the simulation and store results
    pub fn run_simulation(&mut self) -> Result<(), SimulationError> {
        // Convert TUI data to simulation config
        let config = self
            .to_simulation_config()
            .map_err(|e| SimulationError::Config(e.to_string()))?;

        // Generate a random seed
        let seed = rand::rng().next_u64();

        // Run the simulation
        let core_result = finplan_core::simulation::simulate(&config, seed);

        // Convert to TUI result format
        let tui_result = to_tui_result(
            &core_result,
            &self.data().parameters.birth_date,
            &self.data().parameters.start_date,
        )
        .map_err(|e| SimulationError::Conversion(e.to_string()))?;

        self.simulation_result = Some(tui_result);
        self.core_simulation_result = Some(core_result);

        // Reset results state when new simulation runs
        self.results_state = ResultsState::default();

        Ok(())
    }
}
