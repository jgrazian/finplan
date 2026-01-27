use std::collections::HashSet;
use std::path::PathBuf;

use finplan_core::config::SimulationConfig;
use rand::RngCore;

use crate::data::app_data::{AppData, SimulationData};
use crate::data::convert::{ConvertError, to_simulation_config, to_tui_result};

use super::cache::CachedValue;
use super::errors::{LoadError, SaveError, SimulationError};
use super::modal::ModalState;
use super::screen_state::{
    EventsState, MonteCarloPreviewSummary, OptimizeState, PercentileView, PortfolioProfilesState,
    ProjectionPreview, ResultsState, ScenarioState, ScenarioSummary,
};
use super::tabs::TabId;

// ========== SimulationResult ==========
// Simplified result structure for TUI display

#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub final_net_worth: f64,
    /// Final net worth in real (today's) dollars
    pub final_real_net_worth: f64,
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
    /// Net worth in real (today's) dollars
    pub real_net_worth: f64,
    /// Income in real (today's) dollars
    pub real_income: f64,
    /// Expenses in real (today's) dollars
    pub real_expenses: f64,
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
    pub fn get_percentile_core(
        &self,
        percentile: f64,
    ) -> Option<&finplan_core::model::SimulationResult> {
        self.percentile_results
            .iter()
            .find(|(p, _, _)| (*p - percentile).abs() < 0.001)
            .map(|(_, _, core)| core)
    }
}

// ========== SimulationStatus ==========
// Track background simulation progress

#[derive(Debug, Clone, Default)]
pub enum SimulationStatus {
    #[default]
    Idle,
    RunningSingle,
    RunningMonteCarlo {
        current: usize,
        total: usize,
    },
    RunningBatch {
        /// Current scenario index (0-based)
        scenario_index: usize,
        /// Total number of scenarios
        scenario_total: usize,
        /// Current iteration within scenario
        iteration_current: usize,
        /// Total iterations per scenario
        iteration_total: usize,
        /// Name of current scenario being processed
        current_scenario_name: Option<String>,
    },
}

impl SimulationStatus {
    pub fn is_running(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

// ========== PendingSimulation ==========
// Request for App to start a background simulation

#[derive(Debug, Clone)]
pub enum PendingSimulation {
    /// Run a single deterministic simulation
    Single,
    /// Run Monte Carlo simulation with specified iterations
    MonteCarlo { iterations: usize },
    /// Run Monte Carlo on all scenarios
    Batch { iterations: usize },
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

    /// Data version for cache invalidation - incremented on every modification
    data_version: u64,
    /// Cached simulation config with version-based invalidation
    cached_config: CachedValue<SimulationConfig>,

    pub simulation_result: Option<SimulationResult>,
    /// Core simulation result (needed for ledger and wealth snapshots)
    pub core_simulation_result: Option<finplan_core::model::SimulationResult>,
    /// Monte Carlo simulation result (4 representative runs + stats)
    pub monte_carlo_result: Option<MonteCarloStoredResult>,

    /// Status of background simulation (for progress display)
    pub simulation_status: SimulationStatus,

    /// Pending simulation request (set by scenario screen, consumed by App)
    pub pending_simulation: Option<PendingSimulation>,

    /// Set of scenario names with unsaved changes
    pub dirty_scenarios: HashSet<String>,

    // Per-screen state
    pub portfolio_profiles_state: PortfolioProfilesState,
    pub events_state: EventsState,
    pub scenario_state: ScenarioState,
    pub results_state: ResultsState,
    pub optimize_state: OptimizeState,

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
            data_version: 1, // Start at 1 so version 0 is always stale
            cached_config: CachedValue::new(),
            simulation_result: None,
            core_simulation_result: None,
            monte_carlo_result: None,
            simulation_status: SimulationStatus::default(),
            pending_simulation: None,
            dirty_scenarios: HashSet::new(),
            portfolio_profiles_state: PortfolioProfilesState::default(),
            events_state: EventsState::default(),
            scenario_state: ScenarioState::default(),
            results_state: ResultsState::default(),
            optimize_state: OptimizeState::new(),
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
            // Increment version to invalidate caches (different scenario = different data)
            self.data_version = self.data_version.wrapping_add(1);
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
        // Increment version for new scenario data
        self.data_version = self.data_version.wrapping_add(1);
    }

    /// Load from the data directory (new per-scenario storage)
    pub fn load_from_data_dir(data_dir: PathBuf) -> Result<Self, LoadError> {
        use crate::data::storage::DataDirectory;

        let storage = DataDirectory::new(data_dir.clone());
        let result = storage.load().map_err(|e| LoadError::Io(e.to_string()))?;

        let mut state = Self {
            app_data: result.app_data,
            current_scenario: result.current_scenario,
            data_dir: Some(data_dir),
            ..Default::default()
        };

        // Load cached scenario summaries
        state.scenario_state.scenario_summaries = result.scenario_summaries;

        Ok(state)
    }

    /// Get the DataDirectory if configured
    fn get_storage(&self) -> Option<crate::data::storage::DataDirectory> {
        self.data_dir
            .as_ref()
            .map(|p| crate::data::storage::DataDirectory::new(p.clone()))
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
        let data = self
            .app_data
            .simulations
            .get(name)
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
        let yaml = data
            .to_yaml()
            .map_err(|e| SaveError::Serialize(e.to_string()))?;
        std::fs::write(dest, yaml).map_err(|e| SaveError::Io(e.to_string()))
    }

    /// Import a scenario from an external file
    pub fn import_scenario(&mut self, source: &std::path::Path) -> Result<String, LoadError> {
        let content = std::fs::read_to_string(source).map_err(|e| LoadError::Io(e.to_string()))?;

        let data =
            SimulationData::from_yaml(&content).map_err(|e| LoadError::Parse(e.to_string()))?;

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

    /// Get or build the cached simulation config using version-based caching.
    /// The cache is automatically invalidated when data_version changes.
    pub fn get_or_build_config(&mut self) -> Result<&SimulationConfig, ConvertError> {
        let version = self.data_version;

        // Check if cache is stale and compute new value if needed
        if self.cached_config.is_stale(version) {
            let config = self.to_simulation_config()?;
            self.cached_config.set(config, version);
        }

        // Safe to unwrap because we just set it if it was stale
        Ok(self.cached_config.get(version).unwrap())
    }

    /// Invalidate the cached config explicitly (rarely needed with version-based caching)
    pub fn invalidate_config_cache(&mut self) {
        self.cached_config.invalidate();
    }

    /// Mark current scenario as modified.
    /// Increments the data version (which invalidates caches) and marks scenario dirty.
    pub fn mark_modified(&mut self) {
        self.data_version = self.data_version.wrapping_add(1);
        self.dirty_scenarios.insert(self.current_scenario.clone());
    }

    /// Get the current data version (for cache consumers)
    pub fn data_version(&self) -> u64 {
        self.data_version
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

    /// Request a single simulation to be run in the background
    /// The App will pick this up and dispatch to the worker
    pub fn request_simulation(&mut self) {
        self.pending_simulation = Some(PendingSimulation::Single);
        self.simulation_status = SimulationStatus::RunningSingle;
    }

    /// Request a Monte Carlo simulation to be run in the background
    /// The App will pick this up and dispatch to the worker
    pub fn request_monte_carlo(&mut self, iterations: usize) {
        self.pending_simulation = Some(PendingSimulation::MonteCarlo { iterations });
        self.simulation_status = SimulationStatus::RunningMonteCarlo {
            current: 0,
            total: iterations,
        };
    }

    /// Request batch Monte Carlo on all scenarios in the background
    /// The App will pick this up and dispatch to the worker
    pub fn request_batch_monte_carlo(&mut self, iterations: usize) {
        let num_scenarios = self.app_data.simulations.len();
        self.pending_simulation = Some(PendingSimulation::Batch { iterations });
        self.simulation_status = SimulationStatus::RunningBatch {
            scenario_index: 0,
            scenario_total: num_scenarios,
            iteration_current: 0,
            iteration_total: iterations,
            current_scenario_name: None,
        };
        self.scenario_state.batch_running = true;
    }

    /// Run the simulation and store results (synchronous, blocks UI)
    /// Use request_simulation() for background execution
    pub fn run_simulation(&mut self) -> Result<(), SimulationError> {
        // Convert TUI data to simulation config
        let config = self
            .to_simulation_config()
            .map_err(|e| SimulationError::Config(e.to_string()))?;

        // Generate a random seed
        let seed = rand::rng().next_u64();

        // Run the simulation
        let core_result = finplan_core::simulation::simulate(&config, seed)
            .map_err(|e| SimulationError::Config(e.to_string()))?;

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
        let mc_summary =
            finplan_core::simulation::monte_carlo_simulate_with_config(&sim_config, &mc_config)
                .map_err(|e| SimulationError::Config(e.to_string()))?;

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
        let (mean_tui_result, mean_core_result) =
            if let Some(mean_core) = mc_summary.get_mean_result() {
                let mean_tui = to_tui_result(&mean_core, birth_date, start_date)
                    .map_err(|e| SimulationError::Conversion(e.to_string()))?;
                (Some(mean_tui), Some(mean_core))
            } else {
                (None, None)
            };

        // Store the P50 run as the default simulation result
        if let Some((_, tui, core)) = percentile_results
            .iter()
            .find(|(p, _, _)| (*p - 0.50).abs() < 0.001)
        {
            self.simulation_result = Some(tui.clone());
            self.core_simulation_result = Some(core.clone());
        }

        // Update scenario preview with MC summary
        if let Some(preview) = &mut self.scenario_state.projection_preview {
            let p5_final = mc_summary
                .stats
                .percentile_values
                .iter()
                .find(|(p, _)| (*p - 0.05).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);
            let p50_final = mc_summary
                .stats
                .percentile_values
                .iter()
                .find(|(p, _)| (*p - 0.50).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);
            let p95_final = mc_summary
                .stats
                .percentile_values
                .iter()
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

        // Update scenario summary cache
        self.update_current_scenario_summary();

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
        let core_result = finplan_core::simulation::simulate(&config, seed)
            .map_err(|e| SimulationError::Config(e.to_string()))?;

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

    /// Update scenario summary for the current scenario after a Monte Carlo run
    pub fn update_current_scenario_summary(&mut self) {
        if let Some(mc) = &self.monte_carlo_result {
            let p5 = mc
                .stats
                .percentile_values
                .iter()
                .find(|(p, _)| (*p - 0.05).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);
            let p50 = mc
                .stats
                .percentile_values
                .iter()
                .find(|(p, _)| (*p - 0.50).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);
            let p95 = mc
                .stats
                .percentile_values
                .iter()
                .find(|(p, _)| (*p - 0.95).abs() < 0.001)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);

            // Get yearly net worth (nominal and real) from P50 TUI result
            let p50_tui = mc.get_percentile_tui(0.50);
            let yearly_nw = p50_tui.map(|tui| {
                tui.years
                    .iter()
                    .map(|y| (y.year, y.net_worth))
                    .collect::<Vec<_>>()
            });
            let yearly_real_nw = p50_tui.map(|tui| {
                tui.years
                    .iter()
                    .map(|y| (y.year, y.real_net_worth))
                    .collect::<Vec<_>>()
            });

            // Get real final net worth and calculate real percentiles
            // Use the inflation factor from P50 to convert all percentiles
            let (final_real_nw, real_p5, real_p50, real_p95) = if let Some(tui) = p50_tui {
                let final_real = tui.final_real_net_worth;
                // Calculate inflation factor from P50: nominal / real
                let inflation_factor = if tui.final_real_net_worth > 0.0 {
                    tui.final_net_worth / tui.final_real_net_worth
                } else {
                    1.0
                };
                // Apply same factor to convert all percentiles to real terms
                let real_p5 = if inflation_factor > 0.0 {
                    p5 / inflation_factor
                } else {
                    p5
                };
                let real_p50 = if inflation_factor > 0.0 {
                    p50 / inflation_factor
                } else {
                    p50
                };
                let real_p95 = if inflation_factor > 0.0 {
                    p95 / inflation_factor
                } else {
                    p95
                };
                (Some(final_real), real_p5, real_p50, real_p95)
            } else {
                (None, p5, p50, p95)
            };

            let summary = ScenarioSummary {
                name: self.current_scenario.clone(),
                final_net_worth: Some(p50),
                success_rate: Some(mc.stats.success_rate),
                percentiles: Some((p5, p50, p95)),
                yearly_net_worth: yearly_nw,
                final_real_net_worth: final_real_nw,
                real_percentiles: Some((real_p5, real_p50, real_p95)),
                yearly_real_net_worth: yearly_real_nw,
            };

            self.scenario_state
                .scenario_summaries
                .insert(self.current_scenario.clone(), summary);

            // Persist summaries to disk
            self.save_scenario_summaries();
        }
    }

    /// Save scenario summaries to disk
    pub fn save_scenario_summaries(&self) {
        if let Some(storage) = self.get_storage()
            && let Err(e) = storage.save_summaries(&self.scenario_state.scenario_summaries)
        {
            tracing::warn!(error = %e, "Failed to save scenario summaries");
        }
    }

    /// Run Monte Carlo simulation on a specific scenario (by name) and cache results
    pub fn run_monte_carlo_for_scenario(
        &mut self,
        scenario_name: &str,
        num_iterations: usize,
    ) -> Result<(), SimulationError> {
        // Get the scenario data
        let scenario_data = self
            .app_data
            .simulations
            .get(scenario_name)
            .ok_or_else(|| {
                SimulationError::Config(format!("Scenario '{}' not found", scenario_name))
            })?
            .clone();

        // Convert to simulation config
        let sim_config = to_simulation_config(&scenario_data)
            .map_err(|e| SimulationError::Config(e.to_string()))?;

        // Configure Monte Carlo simulation
        let mc_config = finplan_core::model::MonteCarloConfig {
            iterations: num_iterations,
            percentiles: vec![0.05, 0.50, 0.95],
            compute_mean: false, // Don't need mean for summary
        };

        // Run the Monte Carlo simulation
        let mc_summary =
            finplan_core::simulation::monte_carlo_simulate_with_config(&sim_config, &mc_config)
                .map_err(|e| SimulationError::Config(e.to_string()))?;

        // Extract summary data
        let p5 = mc_summary
            .stats
            .percentile_values
            .iter()
            .find(|(p, _)| (*p - 0.05).abs() < 0.001)
            .map(|(_, v)| *v)
            .unwrap_or(0.0);
        let p50 = mc_summary
            .stats
            .percentile_values
            .iter()
            .find(|(p, _)| (*p - 0.50).abs() < 0.001)
            .map(|(_, v)| *v)
            .unwrap_or(0.0);
        let p95 = mc_summary
            .stats
            .percentile_values
            .iter()
            .find(|(p, _)| (*p - 0.95).abs() < 0.001)
            .map(|(_, v)| *v)
            .unwrap_or(0.0);

        // Get yearly net worth (nominal and real) from P50 run
        let birth_date = &scenario_data.parameters.birth_date;
        let start_date = &scenario_data.parameters.start_date;
        let p50_tui = mc_summary
            .percentile_runs
            .iter()
            .find(|(p, _)| (*p - 0.50).abs() < 0.001)
            .and_then(|(_, core_result)| to_tui_result(core_result, birth_date, start_date).ok());

        let yearly_nw = p50_tui
            .as_ref()
            .map(|tui| tui.years.iter().map(|y| (y.year, y.net_worth)).collect());
        let yearly_real_nw = p50_tui.as_ref().map(|tui| {
            tui.years
                .iter()
                .map(|y| (y.year, y.real_net_worth))
                .collect()
        });

        // Calculate real values using inflation factor from P50 TUI result
        let (final_real_nw, real_p5, real_p50, real_p95) = if let Some(ref tui) = p50_tui {
            let final_real = tui.final_real_net_worth;
            // Calculate inflation factor from P50: nominal / real
            let inflation_factor = if tui.final_real_net_worth > 0.0 {
                tui.final_net_worth / tui.final_real_net_worth
            } else {
                1.0
            };
            // Apply same factor to convert all percentiles to real terms
            let real_p5 = if inflation_factor > 0.0 {
                p5 / inflation_factor
            } else {
                p5
            };
            let real_p50 = if inflation_factor > 0.0 {
                p50 / inflation_factor
            } else {
                p50
            };
            let real_p95 = if inflation_factor > 0.0 {
                p95 / inflation_factor
            } else {
                p95
            };
            (Some(final_real), real_p5, real_p50, real_p95)
        } else {
            (None, p5, p50, p95)
        };

        let summary = ScenarioSummary {
            name: scenario_name.to_string(),
            final_net_worth: Some(p50),
            success_rate: Some(mc_summary.stats.success_rate),
            percentiles: Some((p5, p50, p95)),
            yearly_net_worth: yearly_nw,
            final_real_net_worth: final_real_nw,
            real_percentiles: Some((real_p5, real_p50, real_p95)),
            yearly_real_net_worth: yearly_real_nw,
        };

        self.scenario_state
            .scenario_summaries
            .insert(scenario_name.to_string(), summary);

        Ok(())
    }

    /// Run Monte Carlo on all scenarios (batch run)
    pub fn run_monte_carlo_all(&mut self, num_iterations: usize) -> Result<usize, SimulationError> {
        self.scenario_state.batch_running = true;
        let scenarios: Vec<String> = self.app_data.simulations.keys().cloned().collect();
        let mut count = 0;

        for scenario_name in scenarios {
            if let Err(e) = self.run_monte_carlo_for_scenario(&scenario_name, num_iterations) {
                // Log error but continue with other scenarios
                tracing::warn!(scenario = scenario_name, error = %e, "Monte Carlo failed");
            } else {
                count += 1;
            }
        }

        self.scenario_state.batch_running = false;

        // Persist all summaries after batch run
        self.save_scenario_summaries();

        Ok(count)
    }

    /// Get a sorted list of scenario names with their summaries
    pub fn get_scenario_list_with_summaries(&self) -> Vec<(String, Option<&ScenarioSummary>)> {
        let mut scenarios: Vec<_> = self
            .app_data
            .simulations
            .keys()
            .map(|name| {
                let summary = self.scenario_state.scenario_summaries.get(name);
                (name.clone(), summary)
            })
            .collect();
        scenarios.sort_by(|a, b| a.0.cmp(&b.0));
        scenarios
    }

    /// Delete a scenario (does not delete from disk)
    pub fn delete_scenario(&mut self, name: &str) -> bool {
        // Don't allow deleting the last scenario
        if self.app_data.simulations.len() <= 1 {
            return false;
        }

        // Don't allow deleting if it doesn't exist
        if !self.app_data.simulations.contains_key(name) {
            return false;
        }

        // Remove from simulations
        self.app_data.simulations.remove(name);
        self.scenario_state.scenario_summaries.remove(name);
        self.dirty_scenarios.remove(name);

        // If we deleted the current scenario, switch to another one
        if self.current_scenario == name
            && let Some(new_current) = self.app_data.simulations.keys().next()
        {
            self.current_scenario = new_current.clone();
            self.simulation_result = None;
            self.core_simulation_result = None;
            self.monte_carlo_result = None;
            // Increment version when switching to different scenario data
            self.data_version = self.data_version.wrapping_add(1);
        }

        // Adjust selected index if needed
        let num_scenarios = self.app_data.simulations.len();
        if self.scenario_state.selected_index >= num_scenarios {
            self.scenario_state.selected_index = num_scenarios.saturating_sub(1);
        }

        true
    }

    /// Duplicate a scenario
    pub fn duplicate_scenario(&mut self, source_name: &str, new_name: &str) -> bool {
        if let Some(source_data) = self.app_data.simulations.get(source_name) {
            let data = source_data.clone();
            self.app_data.simulations.insert(new_name.to_string(), data);
            self.dirty_scenarios.insert(new_name.to_string());
            true
        } else {
            false
        }
    }
}
