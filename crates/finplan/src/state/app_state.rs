use std::collections::HashSet;
use std::path::PathBuf;

use finplan_core::config::SimulationConfig;
use rand::RngCore;

use crate::data::app_data::{AppData, SimulationData};
use crate::data::convert::{to_simulation_config, to_tui_result, ConvertError};

use super::errors::{LoadError, SaveError, SimulationError};
use super::modal::ModalState;
use super::screen_state::{EventsState, MonteCarloPreviewSummary, PercentileView, PortfolioProfilesState, ProjectionPreview, ResultsState, ScenarioState};
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
    pub withdrawals: f64,
    pub contributions: f64,
    pub taxes: f64,
}

// ========== Monte Carlo Results ==========

// Re-export MonteCarloStats from core
pub use finplan_core::model::MonteCarloStats;

/// Stored Monte Carlo result with percentile runs + mean
#[derive(Debug)]
pub struct MonteCarloStoredResult {
    /// Aggregate statistics from core
    pub stats: finplan_core::model::MonteCarloStats,
    /// TUI results for each percentile: (percentile, tui_result, core_result)
    pub percentile_results: Vec<(f64, SimulationResult, finplan_core::model::SimulationResult)>,
    /// Synthetic mean TUI result
    pub mean_tui_result: Option<SimulationResult>,
    /// Synthetic mean core result
    pub mean_core_result: Option<finplan_core::model::SimulationResult>,
}

impl MonteCarloStoredResult {
    /// Get the TUI result for a specific percentile
    pub fn get_percentile_tui(&self, percentile: f64) -> Option<&SimulationResult> {
        self.percentile_results
            .iter()
            .find(|(p, _, _)| (*p - percentile).abs() < 0.001)
            .map(|(_, tui, _)| tui)
    }

    /// Get the core result for a specific percentile
    pub fn get_percentile_core(&self, percentile: f64) -> Option<&finplan_core::model::SimulationResult> {
        self.percentile_results
            .iter()
            .find(|(p, _, _)| (*p - percentile).abs() < 0.001)
            .map(|(_, _, core)| core)
    }
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
    /// Path to the data directory (for per-scenario file storage)
    pub data_dir: Option<PathBuf>,
    /// Cached simulation config (rebuilt when running simulation)
    cached_config: Option<SimulationConfig>,
    pub simulation_result: Option<SimulationResult>,
    /// Core simulation result (needed for ledger and wealth snapshots)
    pub core_simulation_result: Option<finplan_core::model::SimulationResult>,
    /// Monte Carlo simulation result (4 representative runs + stats)
    pub monte_carlo_result: Option<MonteCarloStoredResult>,

    /// Set of scenario names with unsaved changes
    pub dirty_scenarios: HashSet<String>,

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
            data_dir: None,
            cached_config: None,
            simulation_result: None,
            core_simulation_result: None,
            monte_carlo_result: None,
            dirty_scenarios: HashSet::new(),
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
            self.monte_carlo_result = None;
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
        self.monte_carlo_result = None;
        self.invalidate_config_cache();
    }

    /// Load from the data directory (new per-scenario storage)
    pub fn load_from_data_dir(data_dir: PathBuf) -> Result<Self, LoadError> {
        use crate::data::storage::DataDirectory;

        let storage = DataDirectory::new(data_dir.clone());
        let result = storage.load().map_err(|e| LoadError::Io(e.to_string()))?;

        Ok(Self {
            app_data: result.app_data,
            current_scenario: result.current_scenario,
            data_dir: Some(data_dir),
            ..Default::default()
        })
    }

    /// Get the DataDirectory if configured
    fn get_storage(&self) -> Option<crate::data::storage::DataDirectory> {
        self.data_dir.as_ref().map(|p| crate::data::storage::DataDirectory::new(p.clone()))
    }

    /// Save the current scenario to its file
    pub fn save_current_scenario(&mut self) -> Result<(), SaveError> {
        let storage = self.get_storage().ok_or(SaveError::NoPath)?;
        let data = self.data().clone();

        storage
            .save_scenario(&self.current_scenario, &data)
            .map_err(|e| SaveError::Io(e.to_string()))?;

        // Also update the active scenario in config
        storage
            .save_active_scenario(&self.current_scenario)
            .map_err(|e| SaveError::Io(e.to_string()))?;

        self.mark_scenario_clean(&self.current_scenario.clone());
        Ok(())
    }

    /// Save a specific scenario to its file
    pub fn save_scenario(&mut self, name: &str) -> Result<(), SaveError> {
        let storage = self.get_storage().ok_or(SaveError::NoPath)?;
        let data = self.app_data.simulations.get(name)
            .ok_or_else(|| SaveError::Io(format!("Scenario '{}' not found", name)))?
            .clone();

        storage
            .save_scenario(name, &data)
            .map_err(|e| SaveError::Io(e.to_string()))?;

        self.mark_scenario_clean(name);
        Ok(())
    }

    /// Save all dirty scenarios
    pub fn save_all_dirty(&mut self) -> Result<usize, SaveError> {
        let dirty: Vec<String> = self.dirty_scenarios.iter().cloned().collect();
        let mut saved = 0;

        for name in dirty {
            self.save_scenario(&name)?;
            saved += 1;
        }

        // Also update the active scenario in config
        if let Some(storage) = self.get_storage() {
            storage
                .save_active_scenario(&self.current_scenario)
                .map_err(|e| SaveError::Io(e.to_string()))?;
        }

        Ok(saved)
    }

    /// Delete a scenario file from storage
    pub fn delete_scenario_file(&self, name: &str) -> Result<(), SaveError> {
        let storage = self.get_storage().ok_or(SaveError::NoPath)?;
        storage
            .delete_scenario(name)
            .map_err(|e| SaveError::Io(e.to_string()))
    }

    /// Export current scenario to an external file
    pub fn export_scenario(&self, dest: &std::path::Path) -> Result<(), SaveError> {
        let data = self.data();
        let yaml = data.to_yaml().map_err(|e| SaveError::Serialize(e.to_string()))?;
        std::fs::write(dest, yaml).map_err(|e| SaveError::Io(e.to_string()))
    }

    /// Import a scenario from an external file
    pub fn import_scenario(&mut self, source: &std::path::Path) -> Result<String, LoadError> {
        let content = std::fs::read_to_string(source)
            .map_err(|e| LoadError::Io(e.to_string()))?;

        let data = SimulationData::from_yaml(&content)
            .map_err(|e| LoadError::Parse(e.to_string()))?;

        // Use the filename (without extension) as the scenario name
        let mut name = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported")
            .to_string();

        // Ensure unique name
        let mut counter = 1;
        let base_name = name.clone();
        while self.app_data.simulations.contains_key(&name) {
            name = format!("{} ({})", base_name, counter);
            counter += 1;
        }

        self.app_data.simulations.insert(name.clone(), data);
        self.dirty_scenarios.insert(name.clone());

        Ok(name)
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

    /// Mark current scenario as modified (invalidates cache and marks dirty)
    pub fn mark_modified(&mut self) {
        self.invalidate_config_cache();
        self.dirty_scenarios.insert(self.current_scenario.clone());
    }

    /// Mark a specific scenario as clean (after saving)
    pub fn mark_scenario_clean(&mut self, name: &str) {
        self.dirty_scenarios.remove(name);
    }

    /// Mark all scenarios as clean
    pub fn mark_all_clean(&mut self) {
        self.dirty_scenarios.clear();
    }

    /// Check if the current scenario has unsaved changes
    pub fn is_current_dirty(&self) -> bool {
        self.dirty_scenarios.contains(&self.current_scenario)
    }

    /// Check if any scenario has unsaved changes
    pub fn has_unsaved_changes(&self) -> bool {
        !self.dirty_scenarios.is_empty()
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

        // Convert to TUI result format (uses pre-computed cash flow summaries from core)
        let tui_result = to_tui_result(
            &core_result,
            &self.data().parameters.birth_date,
            &self.data().parameters.start_date,
        )
        .map_err(|e| SimulationError::Conversion(e.to_string()))?;

        self.simulation_result = Some(tui_result);
        self.core_simulation_result = Some(core_result);
        // Clear Monte Carlo results when running single simulation
        self.monte_carlo_result = None;

        // Reset results state when new simulation runs
        self.results_state = ResultsState::default();
        self.results_state.viewing_monte_carlo = false;

        Ok(())
    }

    /// Run Monte Carlo simulation with specified number of iterations
    pub fn run_monte_carlo(&mut self, num_iterations: usize) -> Result<(), SimulationError> {
        // Convert TUI data to simulation config
        let sim_config = self
            .to_simulation_config()
            .map_err(|e| SimulationError::Config(e.to_string()))?;

        // Configure Monte Carlo simulation
        let mc_config = finplan_core::model::MonteCarloConfig {
            iterations: num_iterations,
            percentiles: vec![0.05, 0.50, 0.95],
            compute_mean: true,
        };

        // Run the Monte Carlo simulation using memory-efficient API
        let mc_summary = finplan_core::simulation::monte_carlo_simulate_with_config(&sim_config, &mc_config);

        // Convert percentile runs to TUI format
        let birth_date = &self.data().parameters.birth_date;
        let start_date = &self.data().parameters.start_date;

        let mut percentile_results = Vec::new();
        for (p, core_result) in &mc_summary.percentile_runs {
            let tui_result = to_tui_result(core_result, birth_date, start_date)
                .map_err(|e| SimulationError::Conversion(e.to_string()))?;
            percentile_results.push((*p, tui_result, core_result.clone()));
        }

        // Build mean results from accumulators
        let (mean_tui_result, mean_core_result) = if let Some(mean_core) = mc_summary.get_mean_result() {
            let mean_tui = to_tui_result(&mean_core, birth_date, start_date)
                .map_err(|e| SimulationError::Conversion(e.to_string()))?;
            (Some(mean_tui), Some(mean_core))
        } else {
            (None, None)
        };

        // Store the P50 run as the default simulation result
        if let Some((_, tui, core)) = percentile_results.iter().find(|(p, _, _)| (*p - 0.50).abs() < 0.001) {
            self.simulation_result = Some(tui.clone());
            self.core_simulation_result = Some(core.clone());
        }

        // Update scenario preview with MC summary
        if let Some(preview) = &mut self.scenario_state.projection_preview {
            let p5_final = mc_summary.stats.percentile_values.iter()
                .find(|(p, _)| (*p - 0.05).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);
            let p50_final = mc_summary.stats.percentile_values.iter()
                .find(|(p, _)| (*p - 0.50).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);
            let p95_final = mc_summary.stats.percentile_values.iter()
                .find(|(p, _)| (*p - 0.95).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);

            preview.mc_summary = Some(MonteCarloPreviewSummary {
                num_iterations: mc_summary.stats.num_iterations,
                success_rate: mc_summary.stats.success_rate,
                p5_final,
                p50_final,
                p95_final,
            });
        }

        // Store the full MC result
        let stored_result = MonteCarloStoredResult {
            stats: mc_summary.stats,
            percentile_results,
            mean_tui_result,
            mean_core_result,
        };
        self.monte_carlo_result = Some(stored_result);

        // Reset results state for MC viewing
        self.results_state = ResultsState::default();
        self.results_state.viewing_monte_carlo = true;
        self.results_state.percentile_view = PercentileView::P50;

        Ok(())
    }

    /// Run a quick projection simulation for the scenario preview
    pub fn run_projection_preview(&mut self) -> Result<(), SimulationError> {
        // Convert TUI data to simulation config
        let config = self
            .to_simulation_config()
            .map_err(|e| SimulationError::Config(e.to_string()))?;

        // Generate a random seed
        let seed = rand::rng().next_u64();

        // Run the simulation
        let core_result = finplan_core::simulation::simulate(&config, seed);

        // Convert to TUI result format (uses pre-computed cash flow summaries from core)
        let tui_result = to_tui_result(
            &core_result,
            &self.data().parameters.birth_date,
            &self.data().parameters.start_date,
        )
        .map_err(|e| SimulationError::Conversion(e.to_string()))?;

        // Calculate totals and milestones
        let total_income: f64 = tui_result.years.iter().map(|y| y.income).sum();
        let total_expenses: f64 = tui_result.years.iter().map(|y| y.expenses).sum();
        let total_taxes: f64 = tui_result.years.iter().map(|y| y.taxes).sum();

        // Generate milestones
        let mut milestones = Vec::new();
        let mut hit_1m = false;
        let mut hit_2m = false;

        for year in &tui_result.years {
            // First year hitting $1M net worth
            if !hit_1m && year.net_worth >= 1_000_000.0 {
                milestones.push((year.year, "$1M net worth".to_string()));
                hit_1m = true;
            }
            // First year hitting $2M net worth
            if !hit_2m && year.net_worth >= 2_000_000.0 {
                milestones.push((year.year, "$2M net worth".to_string()));
                hit_2m = true;
            }
        }

        // Add final year
        if let Some(last) = tui_result.years.last() {
            milestones.push((last.year, format!("Simulation ends (age {})", last.age)));
        }

        // Build yearly net worth data for bar chart
        let yearly_net_worth: Vec<(i32, f64)> = tui_result
            .years
            .iter()
            .map(|y| (y.year, y.net_worth))
            .collect();

        self.scenario_state.projection_preview = Some(ProjectionPreview {
            final_net_worth: tui_result.final_net_worth,
            total_income,
            total_expenses,
            total_taxes,
            milestones,
            yearly_net_worth,
            mc_summary: None,
        });
        self.scenario_state.projection_running = false;

        Ok(())
    }

    /// Invalidate the projection preview cache
    pub fn invalidate_projection_preview(&mut self) {
        self.scenario_state.projection_preview = None;
    }
}
