use std::path::PathBuf;

use finplan_core::config::SimulationConfig;
use rand::RngCore;

use crate::data::app_data::{AppData, SimulationData};
use crate::data::convert::{to_simulation_config, to_tui_result, ConvertError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabId {
    Portfolio,
    Profiles,
    Scenario,
    Events,
    Results,
}

impl TabId {
    pub const ALL: [TabId; 5] = [
        TabId::Portfolio,
        TabId::Profiles,
        TabId::Scenario,
        TabId::Events,
        TabId::Results,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            TabId::Portfolio => "Portfolio",
            TabId::Profiles => "Profiles",
            TabId::Scenario => "Scenario",
            TabId::Events => "Events",
            TabId::Results => "Results",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            TabId::Portfolio => 0,
            TabId::Profiles => 1,
            TabId::Scenario => 2,
            TabId::Events => 3,
            TabId::Results => 4,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(TabId::Portfolio),
            1 => Some(TabId::Profiles),
            2 => Some(TabId::Scenario),
            3 => Some(TabId::Events),
            4 => Some(TabId::Results),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Left,
    Right,
}

#[derive(Debug)]
pub struct PortfolioState {
    pub selected_account_index: usize,
    pub focused_panel: FocusedPanel,
}

impl Default for PortfolioState {
    fn default() -> Self {
        Self {
            selected_account_index: 0,
            focused_panel: FocusedPanel::Left,
        }
    }
}

#[derive(Debug)]
pub struct ProfilesState {
    pub selected_return_profile_index: usize,
    pub focused_panel: FocusedPanel,
}

impl Default for ProfilesState {
    fn default() -> Self {
        Self {
            selected_return_profile_index: 0,
            focused_panel: FocusedPanel::Left,
        }
    }
}

#[derive(Debug)]
pub struct EventsState {
    pub selected_event_index: usize,
    pub focused_panel: FocusedPanel,
}

impl Default for EventsState {
    fn default() -> Self {
        Self {
            selected_event_index: 0,
            focused_panel: FocusedPanel::Left,
        }
    }
}

#[derive(Debug, Default)]
pub struct ScenarioState {
    pub focused_field: usize,
}

#[derive(Debug, Default)]
pub struct ResultsState {
    pub scroll_offset: usize,
}

#[derive(Debug)]
pub enum ModalState {
    None,
    TextInput(TextInputModal),
    Message(MessageModal),
    ScenarioPicker(ScenarioPickerModal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalAction {
    SaveAs,
    Load,
    SwitchTo,
}

#[derive(Debug)]
pub struct ScenarioPickerModal {
    pub title: String,
    pub scenarios: Vec<String>,
    pub selected_index: usize,
    pub action: ModalAction,
    /// For SaveAs: allow entering a new name
    pub new_name: Option<String>,
    pub editing_new_name: bool,
}

impl ScenarioPickerModal {
    pub fn new(title: &str, scenarios: Vec<String>, action: ModalAction) -> Self {
        Self {
            title: title.to_string(),
            scenarios,
            selected_index: 0,
            action,
            new_name: if action == ModalAction::SaveAs {
                Some(String::new())
            } else {
                None
            },
            editing_new_name: false,
        }
    }

    pub fn move_up(&mut self) {
        if self.editing_new_name {
            return;
        }
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.editing_new_name {
            return;
        }
        // +1 for "New scenario" option when saving
        let max_index = if self.action == ModalAction::SaveAs {
            self.scenarios.len()
        } else {
            self.scenarios.len().saturating_sub(1)
        };
        if self.selected_index < max_index {
            self.selected_index += 1;
        }
    }

    pub fn selected_name(&self) -> Option<String> {
        if self.action == ModalAction::SaveAs && self.selected_index == self.scenarios.len() {
            // "New scenario" selected
            self.new_name.clone()
        } else {
            self.scenarios.get(self.selected_index).cloned()
        }
    }

    pub fn is_new_scenario_selected(&self) -> bool {
        self.action == ModalAction::SaveAs && self.selected_index == self.scenarios.len()
    }
}

#[derive(Debug)]
pub struct TextInputModal {
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub cursor_pos: usize,
    pub action: ModalAction,
}

impl TextInputModal {
    pub fn new(title: &str, prompt: &str, default_value: &str, action: ModalAction) -> Self {
        let value = default_value.to_string();
        let cursor_pos = value.len();
        Self {
            title: title.to_string(),
            prompt: prompt.to_string(),
            value,
            cursor_pos,
            action,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.value.insert(self.cursor_pos, c);
        self.cursor_pos += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.value.remove(self.cursor_pos);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor_pos < self.value.len() {
            self.value.remove(self.cursor_pos);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.value.len() {
            self.cursor_pos += 1;
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_pos = self.value.len();
    }
}

#[derive(Debug)]
pub struct MessageModal {
    pub title: String,
    pub message: String,
    pub is_error: bool,
}

impl MessageModal {
    pub fn info(title: &str, message: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            is_error: false,
        }
    }

    pub fn error(title: &str, message: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            is_error: true,
        }
    }
}

/// Main application state
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

    // Per-screen state
    pub portfolio_state: PortfolioState,
    pub profiles_state: ProfilesState,
    pub events_state: EventsState,
    pub scenario_state: ScenarioState,
    pub results_state: ResultsState,

    pub modal: ModalState,
    pub error_message: Option<String>,
    pub exit: bool,
}

impl AppState {
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
        self.invalidate_config_cache();
    }
}

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

impl Default for AppState {
    fn default() -> Self {
        let default_name = "Default".to_string();
        let mut app_data = AppData::new();
        app_data
            .simulations
            .insert(default_name.clone(), SimulationData::default());

        Self {
            active_tab: TabId::Portfolio,
            app_data,
            current_scenario: default_name,
            config_path: None,
            cached_config: None,
            simulation_result: None,
            portfolio_state: PortfolioState::default(),
            profiles_state: ProfilesState::default(),
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

    /// Load AppData from a YAML file
    pub fn load_from_file(path: PathBuf) -> Result<Self, LoadError> {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| LoadError::Io(e.to_string()))?;

        // Try to parse as AppData first, fall back to SimulationData
        let (app_data, current_scenario) = if let Ok(app_data) = AppData::from_yaml(&content) {
            let first_scenario = app_data
                .simulations
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "Default".to_string());
            (app_data, first_scenario)
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
    pub fn save_to_file(&self, path: &PathBuf) -> Result<(), SaveError> {
        let yaml = self.app_data.to_yaml()
            .map_err(|e| SaveError::Serialize(e.to_string()))?;
        std::fs::write(path, yaml)
            .map_err(|e| SaveError::Io(e.to_string()))?;
        Ok(())
    }

    /// Save to current config path (if set)
    pub fn save(&self) -> Result<(), SaveError> {
        match &self.config_path {
            Some(path) => self.save_to_file(path),
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
        Ok(())
    }
}

#[derive(Debug)]
pub enum SimulationError {
    Config(String),
    Conversion(String),
}

impl std::fmt::Display for SimulationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimulationError::Config(msg) => write!(f, "Configuration error: {}", msg),
            SimulationError::Conversion(msg) => write!(f, "Conversion error: {}", msg),
        }
    }
}

#[derive(Debug)]
pub enum LoadError {
    Io(String),
    Parse(String),
}

#[derive(Debug)]
pub enum SaveError {
    Io(String),
    Serialize(String),
    NoPath,
}
