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
    /// Scroll offset for the simulation errors panel
    pub error_scroll_offset: usize,
}

/// Granularity for the cash flow breakdown panel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BreakdownGranularity {
    #[default]
    Yearly,
    Monthly,
}

impl BreakdownGranularity {
    /// Toggle between yearly and monthly
    pub fn toggle(self) -> Self {
        match self {
            Self::Yearly => Self::Monthly,
            Self::Monthly => Self::Yearly,
        }
    }

    /// Short label for display in titles
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Yearly => "Yearly",
            Self::Monthly => "Monthly",
        }
    }
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
    /// Breakdown granularity (yearly vs monthly)
    pub breakdown_granularity: BreakdownGranularity,
    /// Selected row index for monthly breakdown navigation
    pub selected_month_index: usize,
}

// ========== Analysis Screen State (Parameter Sweep) ==========

use std::collections::HashSet;

use super::panels::AnalysisPanel;
use crate::data::analysis_data::{
    AnalysisConfigData, AnalysisMetricData, CachedSweepResults, ChartConfigData,
    SweepConfigFingerprint, SweepParameterData,
};
use finplan_core::analysis::{AnalysisMetric, SweepResults};

/// A single parameter's sensitivity to a metric (for tornado chart)
#[derive(Debug, Clone)]
pub struct SensitivityEntry {
    /// Index of the parameter in sweep_parameters
    pub param_index: usize,
    /// Display label for the parameter
    pub param_label: String,
    /// Metric value when param is at its minimum (averaged over other params)
    pub low_value: f64,
    /// Metric value when param is at its maximum (averaged over other params)
    pub high_value: f64,
    /// Signed impact: high_value - low_value
    pub impact: f64,
    /// Absolute impact for sorting
    pub abs_impact: f64,
}

/// Results from a sweep analysis - wraps core's N-dimensional SweepResults
#[derive(Debug, Clone)]
pub struct AnalysisResults {
    /// The core sweep results with N-dimensional data
    pub sweep_results: SweepResults,
}

impl AnalysisResults {
    /// Create new analysis results from core SweepResults
    pub fn new(sweep_results: SweepResults) -> Self {
        Self { sweep_results }
    }

    /// Get number of dimensions
    pub fn ndim(&self) -> usize {
        self.sweep_results.ndim()
    }

    /// Check if this is a 1D result
    pub fn is_1d(&self) -> bool {
        self.sweep_results.is_1d()
    }

    /// Check if this is a 2D result
    pub fn is_2d(&self) -> bool {
        self.sweep_results.is_2d()
    }

    /// Get parameter values for a dimension
    pub fn param_values(&self, dim: usize) -> &[f64] {
        self.sweep_results
            .param_values
            .get(dim)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get parameter label for a dimension
    pub fn param_label(&self, dim: usize) -> &str {
        self.sweep_results
            .param_labels
            .get(dim)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Get midpoint index for a dimension (for default fixed values)
    pub fn midpoint_index(&self, dim: usize) -> usize {
        let len = self
            .sweep_results
            .param_values
            .get(dim)
            .map(|v| v.len())
            .unwrap_or(1);
        len / 2
    }

    /// Get the grid shape
    pub fn shape(&self) -> &[usize] {
        self.sweep_results.shape()
    }

    // ===== Backwards compatibility helpers for existing rendering code =====

    /// Get param1 values (backwards compatibility)
    pub fn param1_values(&self) -> &[f64] {
        self.param_values(0)
    }

    /// Get param2 values (backwards compatibility)
    pub fn param2_values(&self) -> &[f64] {
        self.param_values(1)
    }

    /// Get param1 label (backwards compatibility)
    pub fn param1_label(&self) -> &str {
        self.param_label(0)
    }

    /// Get param2 label (backwards compatibility)
    pub fn param2_label(&self) -> &str {
        self.param_label(1)
    }

    /// Convert TUI metric to core metric
    fn to_core_metric(metric: &AnalysisMetricData) -> AnalysisMetric {
        match metric {
            AnalysisMetricData::SuccessRate => AnalysisMetric::SuccessRate,
            AnalysisMetricData::NetWorthAtAge { age } => {
                AnalysisMetric::NetWorthAtAge { age: *age }
            }
            AnalysisMetricData::P5FinalNetWorth => AnalysisMetric::Percentile { percentile: 5 },
            AnalysisMetricData::P25FinalNetWorth => AnalysisMetric::Percentile { percentile: 25 },
            AnalysisMetricData::P50FinalNetWorth => AnalysisMetric::Percentile { percentile: 50 },
            AnalysisMetricData::P75FinalNetWorth => AnalysisMetric::Percentile { percentile: 75 },
            AnalysisMetricData::P95FinalNetWorth => AnalysisMetric::Percentile { percentile: 95 },
            AnalysisMetricData::LifetimeTaxes => AnalysisMetric::LifetimeTaxes,
            AnalysisMetricData::MaxDrawdown => AnalysisMetric::MaxDrawdown,
        }
    }

    /// Get metric scale factor (some metrics display as percentages)
    fn metric_scale(metric: &AnalysisMetricData) -> f64 {
        match metric {
            AnalysisMetricData::SuccessRate | AnalysisMetricData::MaxDrawdown => 100.0,
            _ => 1.0,
        }
    }

    /// Get 1D metric data for charting (simple case: first dimension, no fixed values)
    /// Returns (param_values, metric_values)
    pub fn get_1d_metric_data(&self, metric: &AnalysisMetricData) -> (Vec<f64>, Vec<f64>) {
        let core_metric = Self::to_core_metric(metric);
        let scale = Self::metric_scale(metric);

        // For backwards compatibility with existing 1D/2D rendering:
        // Get the full metric grid and return first dimension values
        let (values, _rows, cols) = self.sweep_results.get_metric_grid(&core_metric);

        if cols <= 1 {
            // True 1D data
            let param_vals = self.param1_values().to_vec();
            let metric_vals: Vec<f64> = values.iter().map(|v| v * scale).collect();
            (param_vals, metric_vals)
        } else {
            // 2D+ data - take a slice at midpoint of other dimensions
            let fixed: Vec<Option<usize>> = (0..self.ndim())
                .map(|dim| {
                    if dim == 0 {
                        None
                    } else {
                        Some(self.midpoint_index(dim))
                    }
                })
                .collect();

            if let Some(slice) = self
                .sweep_results
                .get_metric_1d_slice(&core_metric, 0, &fixed)
            {
                let (params, metrics): (Vec<f64>, Vec<f64>) = slice.into_iter().unzip();
                let scaled: Vec<f64> = metrics.iter().map(|v| v * scale).collect();
                (params, scaled)
            } else {
                (Vec::new(), Vec::new())
            }
        }
    }

    /// Get 2D metric data for heatmap (simple case: first two dimensions)
    /// Returns (matrix in row-major, rows, cols, min_val, max_val)
    pub fn get_2d_metric_matrix(
        &self,
        metric: &AnalysisMetricData,
    ) -> Option<(Vec<Vec<f64>>, f64, f64)> {
        let core_metric = Self::to_core_metric(metric);
        let scale = Self::metric_scale(metric);

        // For backwards compatibility: use first two dimensions
        let (values, rows, cols) = self.sweep_results.get_metric_grid(&core_metric);

        if rows == 0 || cols == 0 {
            return None;
        }

        // Reshape into 2D matrix
        let matrix: Vec<Vec<f64>> = values
            .chunks(cols)
            .map(|chunk| chunk.iter().map(|v| v * scale).collect())
            .collect();

        let scaled_values: Vec<f64> = values.iter().map(|v| v * scale).collect();
        let min_val = scaled_values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_val = scaled_values
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);

        Some((matrix, min_val, max_val))
    }

    /// Get 1D metric data for a specific dimension with fixed values for other dimensions
    /// Returns (param_values, metric_values)
    pub fn get_1d_metric_data_for_config(
        &self,
        metric: &AnalysisMetricData,
        x_dim: usize,
        fixed_values: &std::collections::HashMap<usize, usize>,
    ) -> (Vec<f64>, Vec<f64>) {
        let core_metric = Self::to_core_metric(metric);
        let scale = Self::metric_scale(metric);

        // Build fixed array: None for x_dim, Some for others
        let fixed: Vec<Option<usize>> = (0..self.ndim())
            .map(|dim| {
                if dim == x_dim {
                    None
                } else {
                    // Use provided fixed value or midpoint
                    Some(
                        fixed_values
                            .get(&dim)
                            .copied()
                            .unwrap_or_else(|| self.midpoint_index(dim)),
                    )
                }
            })
            .collect();

        if let Some(slice) = self
            .sweep_results
            .get_metric_1d_slice(&core_metric, x_dim, &fixed)
        {
            let (params, metrics): (Vec<f64>, Vec<f64>) = slice.into_iter().unzip();
            let scaled: Vec<f64> = metrics.iter().map(|v| v * scale).collect();
            (params, scaled)
        } else {
            (Vec::new(), Vec::new())
        }
    }

    /// Get 2D metric data for specific dimensions with fixed values for other dimensions
    /// Returns (matrix in row-major, min_val, max_val)
    pub fn get_2d_metric_matrix_for_config(
        &self,
        metric: &AnalysisMetricData,
        x_dim: usize,
        y_dim: usize,
        fixed_values: &std::collections::HashMap<usize, usize>,
    ) -> Option<(Vec<Vec<f64>>, f64, f64)> {
        let core_metric = Self::to_core_metric(metric);
        let scale = Self::metric_scale(metric);

        // Build fixed array: None for x_dim and y_dim, Some for others
        let fixed: Vec<Option<usize>> = (0..self.ndim())
            .map(|dim| {
                if dim == x_dim || dim == y_dim {
                    None
                } else {
                    Some(
                        fixed_values
                            .get(&dim)
                            .copied()
                            .unwrap_or_else(|| self.midpoint_index(dim)),
                    )
                }
            })
            .collect();

        // Get 2D slice - returns (values, x_params, y_params)
        // slice_2d iterates: outer loop = x_dim, inner loop = y_dim
        // So data is [x0y0, x0y1, ..., x0yN, x1y0, x1y1, ..., x1yN, ...]
        let (values, _x_params, _y_params) =
            self.sweep_results
                .get_metric_2d_slice(&core_metric, x_dim, y_dim, &fixed)?;

        let x_len = self.shape().get(x_dim).copied().unwrap_or(1);
        let y_len = self.shape().get(y_dim).copied().unwrap_or(1);

        if x_len == 0 || y_len == 0 || values.is_empty() {
            return None;
        }

        // First reshape with correct chunk size (y_len values per x)
        // This gives matrix_xy[x][y]
        let matrix_xy: Vec<Vec<f64>> = values
            .chunks(y_len)
            .map(|chunk| chunk.iter().map(|v| v * scale).collect())
            .collect();

        // Transpose to get matrix[y][x] (rows = y_dim, cols = x_dim)
        // Reverse Y so matrix[0] = highest Y value (standard chart: Y increases upward)
        // Screen row 0 is at top, so highest Y appears at top
        let matrix: Vec<Vec<f64>> = (0..y_len)
            .rev()
            .map(|y| (0..x_len).map(|x| matrix_xy[x][y]).collect())
            .collect();

        let scaled: Vec<f64> = values.iter().map(|v| v * scale).collect();
        let min_val = scaled.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_val = scaled.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        Some((matrix, min_val, max_val))
    }

    /// Get the min/max spread of a metric across all non-X dimensions for each X value.
    /// This shows how sensitive the metric is to the "hidden" sweep parameters.
    /// Returns (param_values, min_values, max_values)
    pub fn get_1d_metric_spread_across_other_dims(
        &self,
        metric: &AnalysisMetricData,
        x_dim: usize,
    ) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let core_metric = Self::to_core_metric(metric);
        let scale = Self::metric_scale(metric);

        let ndim = self.ndim();
        let shape = self.shape();
        let x_len = shape.get(x_dim).copied().unwrap_or(0);

        if x_len == 0 || ndim == 0 {
            return (Vec::new(), Vec::new(), Vec::new());
        }

        let param_values = self.param_values(x_dim).to_vec();
        let mut min_values = Vec::with_capacity(x_len);
        let mut max_values = Vec::with_capacity(x_len);

        // For each X value, collect all metric values across other dimensions
        for x_idx in 0..x_len {
            let mut values_at_x = Vec::new();

            // Generate all index combinations for other dimensions
            self.collect_values_for_x(
                &core_metric,
                x_dim,
                x_idx,
                &mut vec![0; ndim],
                0,
                shape,
                &mut values_at_x,
            );

            // Scale values and find min/max
            let scaled: Vec<f64> = values_at_x.iter().map(|v| v * scale).collect();

            let (min_val, max_val) = if scaled.is_empty() {
                (0.0, 0.0)
            } else {
                let min = scaled.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = scaled.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                (min, max)
            };

            min_values.push(min_val);
            max_values.push(max_val);
        }

        (param_values, min_values, max_values)
    }

    /// Compute sensitivity of a metric to each sweep parameter.
    /// For each parameter, averages the metric across all other parameter combinations
    /// at the parameter's min and max values, showing the "main effect".
    /// Results are sorted by absolute impact (most impactful first).
    pub fn compute_sensitivity(&self, metric: &AnalysisMetricData) -> Vec<SensitivityEntry> {
        let ndim = self.ndim();
        let shape = self.shape();

        if ndim == 0 || shape.contains(&0) {
            return Vec::new();
        }

        let core_metric = Self::to_core_metric(metric);
        let scale = Self::metric_scale(metric);

        let mut entries = Vec::new();

        for dim in 0..ndim {
            let dim_size = shape[dim];
            if dim_size < 2 {
                continue;
            }

            // Collect all metric values for first and last index of this dimension
            let mut values_at_min = Vec::new();
            let mut values_at_max = Vec::new();

            self.collect_values_for_x(
                &core_metric,
                dim,
                0, // min index
                &mut vec![0; ndim],
                0,
                shape,
                &mut values_at_min,
            );

            self.collect_values_for_x(
                &core_metric,
                dim,
                dim_size - 1, // max index
                &mut vec![0; ndim],
                0,
                shape,
                &mut values_at_max,
            );

            let avg_at_min = if values_at_min.is_empty() {
                0.0
            } else {
                values_at_min.iter().sum::<f64>() / values_at_min.len() as f64
            };

            let avg_at_max = if values_at_max.is_empty() {
                0.0
            } else {
                values_at_max.iter().sum::<f64>() / values_at_max.len() as f64
            };

            let low_value = avg_at_min * scale;
            let high_value = avg_at_max * scale;
            let impact = high_value - low_value;

            entries.push(SensitivityEntry {
                param_index: dim,
                param_label: self.param_label(dim).to_string(),
                low_value,
                high_value,
                impact,
                abs_impact: impact.abs(),
            });
        }

        // Sort by absolute impact (most impactful first)
        entries.sort_by(|a, b| {
            b.abs_impact
                .partial_cmp(&a.abs_impact)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        entries
    }

    /// Compute the baseline metric value (all parameters at midpoint).
    pub fn compute_baseline(&self, metric: &AnalysisMetricData) -> f64 {
        let ndim = self.ndim();
        let shape = self.shape();
        if ndim == 0 || shape.contains(&0) {
            return 0.0;
        }

        let core_metric = Self::to_core_metric(metric);
        let scale = Self::metric_scale(metric);

        let mid_indices: Vec<usize> = (0..ndim).map(|d| self.midpoint_index(d)).collect();
        if let Some(point) = self.sweep_results.get(&mid_indices) {
            point.compute_metric(&core_metric, self.sweep_results.birth_year) * scale
        } else {
            0.0
        }
    }

    /// Compute the overall min/max of a metric across all grid points.
    pub fn compute_metric_range(&self, metric: &AnalysisMetricData) -> (f64, f64) {
        let core_metric = Self::to_core_metric(metric);
        let scale = Self::metric_scale(metric);

        let (values, _rows, _cols) = self.sweep_results.get_metric_grid(&core_metric);
        if values.is_empty() {
            return (0.0, 0.0);
        }

        let min = values.iter().cloned().fold(f64::INFINITY, f64::min) * scale;
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max) * scale;
        (min, max)
    }

    /// Helper to recursively collect metric values for a fixed X index across all other dimensions
    #[allow(clippy::too_many_arguments)]
    fn collect_values_for_x(
        &self,
        metric: &AnalysisMetric,
        x_dim: usize,
        x_idx: usize,
        indices: &mut Vec<usize>,
        current_dim: usize,
        shape: &[usize],
        values: &mut Vec<f64>,
    ) {
        if current_dim == shape.len() {
            // We have a complete index set, get the value
            if let Some(point_data) = self.sweep_results.get(indices) {
                let value = point_data.compute_metric(metric, self.sweep_results.birth_year);
                values.push(value);
            }
            return;
        }

        if current_dim == x_dim {
            // Fix this dimension to x_idx
            indices[current_dim] = x_idx;
            self.collect_values_for_x(
                metric,
                x_dim,
                x_idx,
                indices,
                current_dim + 1,
                shape,
                values,
            );
        } else {
            // Iterate over all values of this dimension
            for i in 0..shape[current_dim] {
                indices[current_dim] = i;
                self.collect_values_for_x(
                    metric,
                    x_dim,
                    x_idx,
                    indices,
                    current_dim + 1,
                    shape,
                    values,
                );
            }
        }
    }
}

/// State for the Analysis screen
#[derive(Debug, Default)]
pub struct AnalysisState {
    /// Currently focused panel
    pub focused_panel: AnalysisPanel,
    /// Sweep parameters (N-dimensional, no hard limit)
    pub sweep_parameters: Vec<SweepParameterData>,
    /// Selected parameter index (for navigation)
    pub selected_param_index: usize,
    /// Selected metric index (for navigation in metrics panel)
    pub selected_metric_index: usize,
    /// Selected metric index for sensitivity tornado chart
    pub sensitivity_metric_index: usize,
    /// Selected metrics to compute
    pub selected_metrics: HashSet<AnalysisMetricData>,
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
    /// Analysis results (can be persisted via cache)
    pub results: Option<AnalysisResults>,
    /// Fingerprint of the config that produced the current results (for cache validation)
    pub results_fingerprint: Option<SweepConfigFingerprint>,
    /// Selected result cursor for 2D navigation (legacy)
    pub selected_result: (usize, usize),
    /// Configured charts for the results panel
    pub chart_configs: Vec<ChartConfigData>,
    /// Selected chart index (for h/l navigation between chart slots)
    pub selected_chart_index: usize,
}

impl AnalysisState {
    pub fn new() -> Self {
        let mut selected_metrics = HashSet::new();
        selected_metrics.insert(AnalysisMetricData::SuccessRate);
        selected_metrics.insert(AnalysisMetricData::P50FinalNetWorth);

        Self {
            focused_panel: AnalysisPanel::Parameters,
            sweep_parameters: Vec::new(),
            selected_param_index: 0,
            selected_metric_index: 0,
            sensitivity_metric_index: 0,
            selected_metrics,
            mc_iterations: 500,
            default_steps: 6,
            running: false,
            current_point: 0,
            total_points: 0,
            results: None,
            results_fingerprint: None,
            selected_result: (0, 0),
            chart_configs: Vec::new(),
            selected_chart_index: 0,
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

    /// Convert persistable config to runtime state (loads from scenario)
    pub fn load_from_config(&mut self, config: &AnalysisConfigData) {
        self.sweep_parameters = config.sweep_parameters.clone();
        self.selected_metrics = config.selected_metrics.clone();
        self.mc_iterations = config.mc_iterations;
        self.default_steps = config.default_steps;
        self.chart_configs = config.chart_configs.clone();
        // Reset transient state
        self.selected_param_index = 0;
        self.selected_metric_index = 0;
        self.sensitivity_metric_index = 0;
        self.selected_chart_index = 0;
        self.results = None;
        self.results_fingerprint = None;
        self.running = false;
        self.current_point = 0;
        self.total_points = 0;
    }

    /// Convert runtime state to persistable config (saves to scenario)
    pub fn to_config(&self) -> AnalysisConfigData {
        AnalysisConfigData {
            sweep_parameters: self.sweep_parameters.clone(),
            selected_metrics: self.selected_metrics.clone(),
            mc_iterations: self.mc_iterations,
            default_steps: self.default_steps,
            chart_configs: self.chart_configs.clone(),
        }
    }

    /// Check if the current results match the current config
    /// Returns false if results are stale (config has changed since results were computed)
    pub fn results_match_config(&self) -> bool {
        let Some(ref fingerprint) = self.results_fingerprint else {
            return false;
        };

        let current_fingerprint = SweepConfigFingerprint::from_config(&self.to_config());
        *fingerprint == current_fingerprint
    }

    /// Try to load results from a cache entry
    /// Returns true if the cache was valid and loaded successfully
    pub fn try_load_from_cache(
        &mut self,
        cached: CachedSweepResults,
        sweep_params: &[SweepParameterData],
    ) -> bool {
        // Verify the cache matches current config
        let current_fingerprint = SweepConfigFingerprint::from_config(&self.to_config());
        if cached.fingerprint != current_fingerprint {
            tracing::debug!("Sweep cache fingerprint mismatch, ignoring cached results");
            return false;
        }

        // Convert SweepResults to AnalysisResults
        let analysis_results = convert_sweep_to_analysis_results(&cached.results, sweep_params);

        self.results = Some(analysis_results);
        self.results_fingerprint = Some(cached.fingerprint);
        true
    }

    /// Clear results if they no longer match the current config
    pub fn invalidate_stale_results(&mut self) {
        if self.results.is_some() && !self.results_match_config() {
            self.results = None;
            self.results_fingerprint = None;
        }
    }
}

/// Convert core SweepResults to TUI AnalysisResults (helper for cache loading)
fn convert_sweep_to_analysis_results(
    results: &finplan_core::analysis::SweepResults,
    sweep_params: &[SweepParameterData],
) -> AnalysisResults {
    // Clone the results and update labels with event names
    let mut sweep_results = results.clone();

    // Generate labels from sweep parameters (e.g., "Retirement Age" instead of "Age (Event 8)")
    for (idx, param) in sweep_params.iter().enumerate() {
        if idx < sweep_results.param_labels.len() {
            sweep_results.param_labels[idx] =
                format!("{} {}", param.event_name, param.sweep_type.display_name());
        }
    }

    AnalysisResults::new(sweep_results)
}
