/// Per-screen state structs.
use std::collections::HashMap;

use super::panels::{EventsPanel, PortfolioProfilesPanel, ResultsPanel, ScenarioPanel};

/// Percentile view for Monte Carlo results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PercentileView {
    P5,
    #[default]
    P50,
    P95,
    Mean,
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
    /// Whether we're in holdings editing mode for the selected account
    pub editing_holdings: bool,
    /// Selected holding index when editing (len = "Add new" option)
    pub selected_holding_index: usize,
    /// Whether we're currently editing a holding's value inline
    pub editing_holding_value: bool,
    /// Buffer for inline value editing
    pub holding_edit_buffer: String,
    /// Whether we're adding a new holding (entering name)
    pub adding_new_holding: bool,
    /// Buffer for new holding name
    pub new_holding_name_buffer: String,
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
            editing_holdings: false,
            selected_holding_index: 0,
            editing_holding_value: false,
            holding_edit_buffer: String::new(),
            adding_new_holding: false,
            new_holding_name_buffer: String::new(),
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

#[derive(Debug)]
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
}

impl Default for ResultsState {
    fn default() -> Self {
        Self {
            scroll_offset: 0,
            focused_panel: ResultsPanel::default(),
            selected_year_index: 0,
            ledger_scroll_offset: 0,
            ledger_filter: LedgerFilter::default(),
            percentile_view: PercentileView::default(),
            viewing_monte_carlo: false,
        }
    }
}
