/// Per-screen state structs.
use std::collections::HashMap;

use finplan_core::model::{AccountId, EventId};

use super::panels::{
    EventsPanel, OptimizePanel, PortfolioProfilesPanel, ResultsPanel, ScenarioPanel,
};

/// Percentile view for Monte Carlo results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PercentileView {
    P5,
    #[default]
    P50,
    P95,
    Mean,
}

/// Display mode for monetary values in results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueDisplayMode {
    /// Display nominal (future) dollar values
    Nominal,
    /// Display real (today's) inflation-adjusted dollar values
    #[default]
    Real,
}

impl ValueDisplayMode {
    /// Toggle between nominal and real display modes
    pub fn toggle(self) -> Self {
        match self {
            Self::Nominal => Self::Real,
            Self::Real => Self::Nominal,
        }
    }

    /// Get a short label for display in titles
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Nominal => "Nominal $",
            Self::Real => "Real $",
        }
    }
}

// ========== Account Interaction State Machine ==========

/// Interaction mode for the account panel in Portfolio & Profiles screen.
/// Replaces boolean flags with an explicit state machine.
#[derive(Debug, Default)]
pub enum AccountInteractionMode {
    /// Normal browsing mode - navigating accounts
    #[default]
    Browsing,
    /// Editing holdings within an account
    EditingHoldings {
        selected_index: usize,
        edit_state: HoldingEditState,
    },
}

/// State for editing holdings within an account
#[derive(Debug, Default, Clone)]
pub enum HoldingEditState {
    /// Selecting which holding to edit/add
    #[default]
    Selecting,
    /// Editing a holding's value (with buffer)
    EditingValue(String),
    /// Adding a new holding (with name buffer)
    AddingNew(String),
}

impl AccountInteractionMode {
    /// Check if we're in holdings editing mode
    pub fn is_editing_holdings(&self) -> bool {
        matches!(self, Self::EditingHoldings { .. })
    }

    /// Check if we're editing a value
    pub fn is_editing_value(&self) -> bool {
        matches!(
            self,
            Self::EditingHoldings {
                edit_state: HoldingEditState::EditingValue(_),
                ..
            }
        )
    }

    /// Check if we're adding a new holding
    pub fn is_adding_new(&self) -> bool {
        matches!(
            self,
            Self::EditingHoldings {
                edit_state: HoldingEditState::AddingNew(_),
                ..
            }
        )
    }

    /// Get the selected holding index (if editing)
    pub fn selected_holding_index(&self) -> Option<usize> {
        match self {
            Self::EditingHoldings { selected_index, .. } => Some(*selected_index),
            Self::Browsing => None,
        }
    }

    /// Enter holdings editing mode
    pub fn enter_editing(selected_index: usize) -> Self {
        Self::EditingHoldings {
            selected_index,
            edit_state: HoldingEditState::Selecting,
        }
    }

    /// Exit to browsing mode
    pub fn exit_editing(&mut self) {
        *self = Self::Browsing;
    }
}

impl PercentileView {
    pub fn next(self) -> Self {
        match self {
            Self::P5 => Self::P50,
            Self::P50 => Self::P95,
            Self::P95 => Self::Mean,
            Self::Mean => Self::P5,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::P5 => "5th Percentile (Worst Case)",
            Self::P50 => "50th Percentile (Median)",
            Self::P95 => "95th Percentile (Best Case)",
            Self::Mean => "Mean (Average)",
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            Self::P5 => "P5",
            Self::P50 => "P50",
            Self::P95 => "P95",
            Self::Mean => "Mean",
        }
    }
}

#[derive(Debug)]
pub struct PortfolioProfilesState {
    pub selected_account_index: usize,
    pub selected_profile_index: usize,
    pub selected_mapping_index: usize,
    pub selected_config_index: usize,
    pub focused_panel: PortfolioProfilesPanel,
    /// Whether the asset mappings panel is collapsed
    pub mappings_collapsed: bool,
    /// Whether the tax/inflation config panel is collapsed
    pub config_collapsed: bool,
    /// Account interaction mode (browsing vs editing holdings)
    pub account_mode: AccountInteractionMode,
    /// Scroll offset for accounts list
    pub account_scroll_offset: usize,
    /// Scroll offset for profiles list
    pub profile_scroll_offset: usize,
    /// Scroll offset for asset mappings list
    pub mapping_scroll_offset: usize,
    /// Scroll offset for holdings list (when editing)
    pub holdings_scroll_offset: usize,
}

impl Default for PortfolioProfilesState {
    fn default() -> Self {
        Self {
            selected_account_index: 0,
            selected_profile_index: 0,
            selected_mapping_index: 0,
            selected_config_index: 0,
            focused_panel: PortfolioProfilesPanel::Accounts,
            mappings_collapsed: false,
            config_collapsed: false,
            account_mode: AccountInteractionMode::Browsing,
            account_scroll_offset: 0,
            profile_scroll_offset: 0,
            mapping_scroll_offset: 0,
            holdings_scroll_offset: 0,
        }
    }
}

#[derive(Debug)]
pub struct EventsState {
    pub selected_event_index: usize,
    pub focused_panel: EventsPanel,
    /// Whether the timeline panel is collapsed
    pub timeline_collapsed: bool,
}

impl Default for EventsState {
    fn default() -> Self {
        Self {
            selected_event_index: 0,
            focused_panel: EventsPanel::EventList,
            timeline_collapsed: false,
        }
    }
}

/// Summary stats for Monte Carlo preview in scenario tab
#[derive(Debug, Clone)]
pub struct MonteCarloPreviewSummary {
    pub num_iterations: usize,
    pub success_rate: f64,
    pub p5_final: f64,
    pub p50_final: f64,
    pub p95_final: f64,
}

/// Cached projection preview for scenario tab
#[derive(Debug, Clone)]
pub struct ProjectionPreview {
    pub final_net_worth: f64,
    pub total_income: f64,
    pub total_expenses: f64,
    pub total_taxes: f64,
    pub milestones: Vec<(i32, String)>, // (year, description)
    /// Yearly net worth data for bar chart
    pub yearly_net_worth: Vec<(i32, f64)>,
    /// Monte Carlo summary (if MC was run)
    pub mc_summary: Option<MonteCarloPreviewSummary>,
}

/// Summary of a scenario's simulation results (cached per-scenario)
/// This is persisted to disk so summaries are available on app restart.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScenarioSummary {
    pub name: String,
    pub final_net_worth: Option<f64>,
    pub success_rate: Option<f64>,
    pub percentiles: Option<(f64, f64, f64)>, // P5, P50, P95
    pub yearly_net_worth: Option<Vec<(i32, f64)>>, // For overlay chart
    /// Final net worth in real (today's) dollars
    #[serde(default)]
    pub final_real_net_worth: Option<f64>,
    /// Percentiles in real (today's) dollars: P5, P50, P95
    #[serde(default)]
    pub real_percentiles: Option<(f64, f64, f64)>,
    /// Yearly net worth in real (today's) dollars
    #[serde(default)]
    pub yearly_real_net_worth: Option<Vec<(i32, f64)>>,
}

#[derive(Debug, Default)]
pub struct ScenarioState {
    pub focused_field: usize,
    /// Cached projection preview (run on tab enter)
    pub projection_preview: Option<ProjectionPreview>,
    /// Whether projection is currently running
    pub projection_running: bool,
    /// Focused panel in the scenario comparison view
    pub focused_panel: ScenarioPanel,
    /// Selected scenario index in the list
    pub selected_index: usize,
    /// Cached summaries per-scenario (populated after batch/individual runs)
    pub scenario_summaries: HashMap<String, ScenarioSummary>,
    /// Whether batch run is in progress
    pub batch_running: bool,
    /// Selected scenarios for comparison (by name)
    pub comparison_scenarios: Vec<String>,
}

/// Filter for the ledger view in Results tab
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LedgerFilter {
    #[default]
    All,
    CashOnly,
    AssetsOnly,
    TaxesOnly,
    EventsOnly,
}

impl LedgerFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::CashOnly,
            Self::CashOnly => Self::AssetsOnly,
            Self::AssetsOnly => Self::TaxesOnly,
            Self::TaxesOnly => Self::EventsOnly,
            Self::EventsOnly => Self::All,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::CashOnly => "Cash",
            Self::AssetsOnly => "Assets",
            Self::TaxesOnly => "Taxes",
            Self::EventsOnly => "Events",
        }
    }
}

#[derive(Debug, Default)]
pub struct ResultsState {
    /// Scroll offset for yearly breakdown panel
    pub scroll_offset: usize,
    /// Currently focused panel
    pub focused_panel: ResultsPanel,
    /// Selected year index for account chart
    pub selected_year_index: usize,
    /// Scroll offset for ledger view
    pub ledger_scroll_offset: usize,
    /// Filter for ledger entries
    pub ledger_filter: LedgerFilter,
    /// Current percentile view for Monte Carlo results
    pub percentile_view: PercentileView,
    /// Whether we're viewing Monte Carlo results
    pub viewing_monte_carlo: bool,
    /// Display mode for monetary values (nominal vs real/inflation-adjusted)
    pub value_display_mode: ValueDisplayMode,
}

// ========== Analysis Screen State (Parameter Sweep) ==========

use std::collections::HashSet;

use super::panels::AnalysisPanel;

/// Sweep parameter for analysis
#[derive(Debug, Clone)]
pub struct AnalysisSweepParameter {
    /// Event ID being swept
    pub event_id: EventId,
    /// Display name for the parameter
    pub name: String,
    /// What is being swept (trigger age, effect value, etc.)
    pub sweep_type: AnalysisSweepType,
    /// Minimum value
    pub min_value: f64,
    /// Maximum value
    pub max_value: f64,
    /// Number of steps
    pub step_count: usize,
    /// Current value being displayed
    pub current_value: f64,
}

/// Type of parameter being swept
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisSweepType {
    /// Age trigger (years)
    TriggerAge,
    /// Date trigger (year)
    TriggerDate,
    /// Effect amount (dollars)
    EffectValue,
    /// Repeating event start age
    RepeatingStartAge,
    /// Repeating event end age
    RepeatingEndAge,
}

impl AnalysisSweepType {
    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::TriggerAge => "Age",
            Self::TriggerDate => "Year",
            Self::EffectValue => "Amount",
            Self::RepeatingStartAge => "Start Age",
            Self::RepeatingEndAge => "End Age",
        }
    }
}

/// Metrics that can be computed in analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnalysisMetricType {
    SuccessRate,
    NetWorthAtAge { age: u8 },
    P5FinalNetWorth,
    P50FinalNetWorth,
    P95FinalNetWorth,
    LifetimeTaxes,
    MaxDrawdown,
}

impl AnalysisMetricType {
    pub fn label(&self) -> String {
        match self {
            Self::SuccessRate => "Success Rate".to_string(),
            Self::NetWorthAtAge { age } => format!("Net Worth at {}", age),
            Self::P5FinalNetWorth => "P5 Final Net Worth".to_string(),
            Self::P50FinalNetWorth => "P50 Final Net Worth".to_string(),
            Self::P95FinalNetWorth => "P95 Final Net Worth".to_string(),
            Self::LifetimeTaxes => "Lifetime Taxes".to_string(),
            Self::MaxDrawdown => "Max Drawdown".to_string(),
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            Self::SuccessRate => "Success %",
            Self::NetWorthAtAge { .. } => "Net Worth",
            Self::P5FinalNetWorth => "P5",
            Self::P50FinalNetWorth => "P50",
            Self::P95FinalNetWorth => "P95",
            Self::LifetimeTaxes => "Taxes",
            Self::MaxDrawdown => "Drawdown",
        }
    }
}

/// Results from a sweep analysis
#[derive(Debug, Clone)]
pub struct AnalysisResults {
    /// Parameter 1 values
    pub param1_values: Vec<f64>,
    /// Parameter 2 values (empty for 1D)
    pub param2_values: Vec<f64>,
    /// Results for each metric
    pub metric_results: HashMap<AnalysisMetricType, Vec<Vec<f64>>>,
    /// Parameter 1 label
    pub param1_label: String,
    /// Parameter 2 label (empty for 1D)
    pub param2_label: String,
}

impl AnalysisResults {
    /// Check if this is a 1D result
    pub fn is_1d(&self) -> bool {
        self.param2_values.is_empty()
    }

    /// Get flat values for a metric (for 1D charts)
    pub fn get_1d_values(&self, metric: &AnalysisMetricType) -> Vec<f64> {
        self.metric_results
            .get(metric)
            .map(|rows| rows.iter().filter_map(|r| r.first().copied()).collect())
            .unwrap_or_default()
    }
}

/// State for the Analysis screen
#[derive(Debug, Default)]
pub struct AnalysisState {
    /// Currently focused panel
    pub focused_panel: AnalysisPanel,
    /// Sweep parameters (max 2)
    pub sweep_parameters: Vec<AnalysisSweepParameter>,
    /// Selected parameter index (for navigation)
    pub selected_param_index: usize,
    /// Selected metrics to compute
    pub selected_metrics: HashSet<AnalysisMetricType>,
    /// Monte Carlo iterations per point
    pub mc_iterations: usize,
    /// Number of steps for sweeps
    pub default_steps: usize,
    /// Whether analysis is running
    pub running: bool,
    /// Current point being processed
    pub current_point: usize,
    /// Total points to process
    pub total_points: usize,
    /// Analysis results (session only, not persisted)
    pub results: Option<AnalysisResults>,
    /// Selected result cursor for 2D navigation
    pub selected_result: (usize, usize),
}

impl AnalysisState {
    pub fn new() -> Self {
        let mut selected_metrics = HashSet::new();
        selected_metrics.insert(AnalysisMetricType::SuccessRate);
        selected_metrics.insert(AnalysisMetricType::P50FinalNetWorth);

        Self {
            focused_panel: AnalysisPanel::Parameters,
            sweep_parameters: Vec::new(),
            selected_param_index: 0,
            selected_metrics,
            mc_iterations: 500,
            default_steps: 6,
            running: false,
            current_point: 0,
            total_points: 0,
            results: None,
            selected_result: (0, 0),
        }
    }

    /// Check if this is a 1D analysis
    pub fn is_1d(&self) -> bool {
        self.sweep_parameters.len() == 1
    }

    /// Check if this is a 2D analysis
    pub fn is_2d(&self) -> bool {
        self.sweep_parameters.len() == 2
    }

    /// Calculate total sweep points
    pub fn total_sweep_points(&self) -> usize {
        self.sweep_parameters
            .iter()
            .map(|p| p.step_count)
            .product::<usize>()
            .max(1)
    }
}

// ========== Legacy Optimize State (kept for backwards compatibility) ==========

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParameterType {
    #[default]
    RetirementAge,
    ContributionRate,
    WithdrawalAmount,
    AssetAllocation,
}

#[derive(Debug, Clone)]
pub struct SelectedParameter {
    pub param_type: ParameterType,
    pub event_id: Option<EventId>,
    pub account_id: Option<AccountId>,
    pub min_value: f64,
    pub max_value: f64,
}

#[derive(Debug, Clone, Default)]
pub enum OptimizationObjectiveSelection {
    #[default]
    MaxWealthAtDeath,
    MaxWealthAtRetirement {
        event_id: Option<EventId>,
    },
    MaxSustainableWithdrawal {
        event_id: Option<EventId>,
        success_rate: f64,
    },
    MinLifetimeTax,
}

#[derive(Debug, Clone)]
pub struct OptimizationResultDisplay {
    pub optimal_values: Vec<(String, f64)>,
    pub objective_value: f64,
    pub success_rate: f64,
    pub converged: bool,
    pub iterations: usize,
}

#[derive(Debug, Default)]
pub struct OptimizeState {
    pub focused_panel: OptimizePanel,
    pub selected_param_index: usize,
    pub selected_parameters: Vec<SelectedParameter>,
    pub objective: OptimizationObjectiveSelection,
    pub mc_iterations: usize,
    pub max_iterations: usize,
    pub running: bool,
    pub current_iteration: usize,
    pub result: Option<OptimizationResultDisplay>,
    pub convergence_data: Vec<(usize, f64)>,
}

impl OptimizeState {
    pub fn new() -> Self {
        Self {
            mc_iterations: 500,
            max_iterations: 100,
            ..Default::default()
        }
    }
}
