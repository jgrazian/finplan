//! Analysis metrics that can be computed from simulation results.

use std::collections::HashMap;

use crate::model::{AccountSnapshot, MonteCarloSummary, SimulationResult};
use serde::{Deserialize, Serialize};

/// Metrics that can be computed from Monte Carlo results
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnalysisMetric {
    /// Success rate (proportion of runs with positive final net worth)
    SuccessRate,
    /// Net worth at a specific age
    NetWorthAtAge { age: u8 },
    /// Specific percentile of final net worth
    Percentile { percentile: u8 },
    /// Total lifetime taxes paid
    LifetimeTaxes,
    /// Maximum drawdown (peak-to-trough decline)
    MaxDrawdown,
    /// Safe withdrawal rate achieving a target success rate
    SafeWithdrawalRate { target_success_rate: f64 },
}

impl AnalysisMetric {
    /// Get a display label for the metric
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::SuccessRate => "Success Rate".to_string(),
            Self::NetWorthAtAge { age } => format!("Net Worth at {age}"),
            Self::Percentile { percentile } => format!("P{percentile}"),
            Self::LifetimeTaxes => "Lifetime Taxes".to_string(),
            Self::MaxDrawdown => "Max Drawdown".to_string(),
            Self::SafeWithdrawalRate {
                target_success_rate,
            } => format!("SWR @ {:.0}%", target_success_rate * 100.0),
        }
    }

    /// Get a short label suitable for chart axes
    #[must_use]
    pub fn short_label(&self) -> &str {
        match self {
            Self::SuccessRate => "Success %",
            Self::NetWorthAtAge { .. } => "Net Worth",
            Self::Percentile { .. } => "NW Percentile",
            Self::LifetimeTaxes => "Taxes",
            Self::MaxDrawdown => "Drawdown",
            Self::SafeWithdrawalRate { .. } => "SWR",
        }
    }
}

/// Computed metrics from a single sweep point (legacy, for backwards compatibility)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComputedMetrics {
    pub success_rate: Option<f64>,
    pub net_worth_at_age: Option<f64>,
    /// Percentile values indexed by percentile (e.g., 5 -> P5 value, 50 -> P50 value)
    pub percentile_values: HashMap<u8, f64>,
    pub lifetime_taxes: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub safe_withdrawal_rate: Option<f64>,
}

/// Raw data from a single sweep point - stores enough data to compute any metric on demand.
///
/// This enables adding new metrics or changing metric configuration without re-running
/// the expensive Monte Carlo simulations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepPointData {
    /// Success rate (0.0 to 1.0)
    pub success_rate: f64,
    /// Number of Monte Carlo iterations used
    pub num_iterations: usize,
    /// Final net worth percentiles: (percentile as 0-1, value) in NOMINAL dollars
    /// Typically includes 0.05, 0.25, 0.50, 0.75, 0.95
    pub final_percentiles: Vec<(f64, f64)>,
    /// P50 yearly net worth: (year, `net_worth`) in NOMINAL dollars - for `NetWorthAtAge` and `MaxDrawdown`
    pub p50_yearly_net_worth: Vec<(i16, f64)>,
    /// Total lifetime taxes from P50 run in NOMINAL dollars
    pub p50_lifetime_taxes: f64,
    /// Cumulative inflation factor at end of simulation (for converting to real dollars)
    /// Value of 1.0 means no inflation adjustment; higher values indicate more inflation.
    /// To convert nominal to real: `real_value` = `nominal_value` / `final_inflation_factor`
    #[serde(default = "default_inflation_factor")]
    pub final_inflation_factor: f64,
}

/// Default inflation factor for backwards compatibility with old serialized data
fn default_inflation_factor() -> f64 {
    1.0
}

impl Default for SweepPointData {
    fn default() -> Self {
        Self {
            success_rate: 0.0,
            num_iterations: 0,
            final_percentiles: Vec::new(),
            p50_yearly_net_worth: Vec::new(),
            p50_lifetime_taxes: 0.0,
            final_inflation_factor: 1.0,
        }
    }
}

impl SweepPointData {
    /// Extract `SweepPointData` from a `MonteCarloSummary`
    #[must_use]
    pub fn from_summary(summary: &MonteCarloSummary, _birth_year: i16) -> Self {
        let success_rate = summary.stats.success_rate;
        let num_iterations = summary.stats.num_iterations;

        // Extract percentile values for final net worth
        let final_percentiles = summary.stats.percentile_values.clone();

        // Find P50 run for detailed data
        let p50_run = summary
            .percentile_runs
            .iter()
            .find(|(p, _)| (*p - 0.5).abs() < 0.01)
            .map(|(_, result)| result);

        // Extract yearly net worth from P50 run
        let p50_yearly_net_worth = p50_run
            .map(|result| {
                result
                    .wealth_snapshots
                    .iter()
                    .map(|s| {
                        let total: f64 = s.accounts.iter().map(AccountSnapshot::total_value).sum();
                        (s.date.year(), total)
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Extract lifetime taxes from P50 run
        let p50_lifetime_taxes = p50_run.map_or(0.0, |result| {
            result.yearly_taxes.iter().map(|t| t.total_tax).sum()
        });

        // Extract final cumulative inflation factor from P50 run
        // This is the factor needed to convert end-of-simulation nominal values to real dollars
        let final_inflation_factor = p50_run
            .and_then(|result| result.cumulative_inflation.last().copied())
            .unwrap_or(1.0);

        Self {
            success_rate,
            num_iterations,
            final_percentiles,
            p50_yearly_net_worth,
            p50_lifetime_taxes,
            final_inflation_factor,
        }
    }

    /// Compute a specific metric from this raw data.
    ///
    /// All monetary values are returned in inflation-adjusted (real) dollars.
    /// This uses the cumulative inflation factor from the end of the simulation
    /// to convert nominal values to today's dollars.
    #[must_use]
    pub fn compute_metric(&self, metric: &AnalysisMetric, birth_year: i16) -> f64 {
        self.compute_metric_with_inflation(metric, birth_year, self.final_inflation_factor)
    }

    /// Compute a metric using a specified inflation factor.
    ///
    /// This allows using a standardized inflation factor across multiple sweep points
    /// for consistent comparison (e.g., in heatmaps).
    #[must_use]
    pub fn compute_metric_with_inflation(
        &self,
        metric: &AnalysisMetric,
        birth_year: i16,
        inflation_factor: f64,
    ) -> f64 {
        // Helper to convert nominal to real dollars
        let to_real = |nominal: f64| -> f64 {
            if inflation_factor > 0.0 {
                nominal / inflation_factor
            } else {
                nominal
            }
        };

        match metric {
            // SuccessRate is already a ratio, no adjustment needed
            AnalysisMetric::SuccessRate => self.success_rate,

            // Percentile net worth values need inflation adjustment
            AnalysisMetric::Percentile { percentile } => {
                let target_p = f64::from(*percentile) / 100.0;
                let nominal = self
                    .final_percentiles
                    .iter()
                    .find(|(p, _)| (*p - target_p).abs() < 0.01)
                    .map_or(0.0, |(_, v)| *v);
                to_real(nominal)
            }

            // Net worth at specific age needs inflation adjustment
            // Note: We use final inflation factor as approximation (could be refined
            // with per-year factors if stored, but final factor is reasonable for analysis)
            AnalysisMetric::NetWorthAtAge { age } => {
                let target_year = birth_year + i16::from(*age);
                let nominal = self
                    .p50_yearly_net_worth
                    .iter()
                    .find(|(year, _)| *year == target_year)
                    .map_or(0.0, |(_, nw)| *nw);
                to_real(nominal)
            }

            // Lifetime taxes need inflation adjustment
            AnalysisMetric::LifetimeTaxes => to_real(self.p50_lifetime_taxes),

            // MaxDrawdown is a percentage (peak-to-trough ratio), no adjustment needed
            AnalysisMetric::MaxDrawdown => self.compute_max_drawdown(),

            // SWR is a percentage, no adjustment needed
            AnalysisMetric::SafeWithdrawalRate { .. } => {
                // SWR requires iterative search - not computed from stored data
                0.0
            }
        }
    }

    /// Compute max drawdown from yearly net worth data
    fn compute_max_drawdown(&self) -> f64 {
        if self.p50_yearly_net_worth.is_empty() {
            return 0.0;
        }

        let mut peak = self.p50_yearly_net_worth[0].1;
        let mut max_drawdown = 0.0;

        for (_, net_worth) in &self.p50_yearly_net_worth {
            if *net_worth > peak {
                peak = *net_worth;
            }
            if peak > 0.0 {
                let drawdown = (peak - net_worth) / peak;
                if drawdown > max_drawdown {
                    max_drawdown = drawdown;
                }
            }
        }

        max_drawdown
    }

    /// Convert to legacy `ComputedMetrics` format (for compatibility)
    #[must_use]
    pub fn to_computed_metrics(
        &self,
        metrics: &[AnalysisMetric],
        birth_year: i16,
    ) -> ComputedMetrics {
        let mut result = ComputedMetrics::default();

        for metric in metrics {
            match metric {
                AnalysisMetric::SuccessRate => {
                    result.success_rate = Some(self.success_rate);
                }
                AnalysisMetric::Percentile { percentile } => {
                    let value = self.compute_metric(metric, birth_year);
                    result.percentile_values.insert(*percentile, value);
                }
                AnalysisMetric::NetWorthAtAge { .. } => {
                    result.net_worth_at_age = Some(self.compute_metric(metric, birth_year));
                }
                AnalysisMetric::LifetimeTaxes => {
                    result.lifetime_taxes = Some(self.p50_lifetime_taxes);
                }
                AnalysisMetric::MaxDrawdown => {
                    result.max_drawdown = Some(self.compute_max_drawdown());
                }
                AnalysisMetric::SafeWithdrawalRate { .. } => {
                    // Not computed
                }
            }
        }

        result
    }
}

/// Compute metrics from a Monte Carlo summary
pub fn compute_metrics(
    summary: &MonteCarloSummary,
    metrics: &[AnalysisMetric],
    birth_year: i16,
) -> ComputedMetrics {
    let mut result = ComputedMetrics::default();

    for metric in metrics {
        match metric {
            AnalysisMetric::SuccessRate => {
                result.success_rate = Some(summary.stats.success_rate);
            }
            AnalysisMetric::NetWorthAtAge { age } => {
                // Get net worth at specific age from the P50 run
                if let Some((_, p50_result)) = summary
                    .percentile_runs
                    .iter()
                    .find(|(p, _)| (*p - 0.5).abs() < 0.01)
                {
                    let target_year = birth_year + i16::from(*age);
                    result.net_worth_at_age = p50_result
                        .wealth_snapshots
                        .iter()
                        .find(|s| s.date.year() == target_year)
                        .map(snapshot_total_value);
                }
            }
            AnalysisMetric::Percentile { percentile } => {
                let target_p = f64::from(*percentile) / 100.0;
                if let Some((_, value)) = summary
                    .stats
                    .percentile_values
                    .iter()
                    .find(|(p, _)| (*p - target_p).abs() < 0.01)
                {
                    result.percentile_values.insert(*percentile, *value);
                }
            }
            AnalysisMetric::LifetimeTaxes => {
                // Sum taxes from P50 run
                if let Some((_, p50_result)) = summary
                    .percentile_runs
                    .iter()
                    .find(|(p, _)| (*p - 0.5).abs() < 0.01)
                {
                    result.lifetime_taxes = Some(
                        p50_result
                            .yearly_taxes
                            .iter()
                            .map(|t| t.total_tax)
                            .sum::<f64>(),
                    );
                }
            }
            AnalysisMetric::MaxDrawdown => {
                // Compute max drawdown from P50 run
                if let Some((_, p50_result)) = summary
                    .percentile_runs
                    .iter()
                    .find(|(p, _)| (*p - 0.5).abs() < 0.01)
                {
                    result.max_drawdown = Some(compute_max_drawdown(p50_result));
                }
            }
            AnalysisMetric::SafeWithdrawalRate { .. } => {
                // This requires iterative search - skip for now
                // Could be computed separately if needed
            }
        }
    }

    result
}

/// Compute total value from a wealth snapshot
fn snapshot_total_value(snapshot: &crate::model::WealthSnapshot) -> f64 {
    snapshot
        .accounts
        .iter()
        .map(AccountSnapshot::total_value)
        .sum()
}

/// Compute maximum drawdown from a simulation result
/// Returns the drawdown as a positive fraction (e.g., 0.25 = 25% drawdown)
#[must_use]
pub fn compute_max_drawdown(result: &SimulationResult) -> f64 {
    let snapshots = &result.wealth_snapshots;
    if snapshots.is_empty() {
        return 0.0;
    }

    let mut peak = snapshot_total_value(&snapshots[0]);
    let mut max_drawdown = 0.0;

    for snapshot in snapshots {
        let total = snapshot_total_value(snapshot);
        if total > peak {
            peak = total;
        }
        if peak > 0.0 {
            let drawdown = (peak - total) / peak;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }
    }

    max_drawdown
}

/// Result of a sweep analysis (N-dimensional)
///
/// Stores raw simulation data at each grid point, allowing metrics to be computed
/// on-demand without re-running simulations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepResults {
    /// Values for each parameter dimension
    pub param_values: Vec<Vec<f64>>,
    /// Labels for each parameter
    pub param_labels: Vec<String>,
    /// N-dimensional grid of raw sweep data
    pub data: super::SweepGrid<SweepPointData>,
    /// Birth year for age-based metric calculations
    pub birth_year: i16,
    /// Standardized inflation factor used for all metric computations.
    /// This ensures consistent inflation adjustment across all sweep points,
    /// avoiding visual artifacts in heatmaps from varying simulation durations.
    /// Computed as the maximum inflation factor across all sweep points.
    #[serde(default = "default_inflation_factor")]
    pub standard_inflation_factor: f64,
}

impl SweepResults {
    /// Create new sweep results
    pub fn new(param_values: Vec<Vec<f64>>, param_labels: Vec<String>, birth_year: i16) -> Self {
        let shape: Vec<usize> = param_values.iter().map(Vec::len).collect();
        Self {
            param_values,
            param_labels,
            data: super::SweepGrid::new(shape, SweepPointData::default()),
            birth_year,
            standard_inflation_factor: 1.0, // Will be computed after data is populated
        }
    }

    /// Compute and set the standard inflation factor from all populated sweep points.
    /// Uses the maximum inflation factor across all points to ensure consistent
    /// conversion to real dollars (the longest simulation duration sets the baseline).
    pub fn finalize_inflation_factor(&mut self) {
        let max_factor = self
            .data
            .data()
            .iter()
            .map(|point| point.final_inflation_factor)
            .fold(1.0_f64, f64::max);
        self.standard_inflation_factor = max_factor;
    }

    /// Get the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.param_values.len()
    }

    /// Check if this is a 1D result
    #[must_use]
    pub fn is_1d(&self) -> bool {
        self.param_values.len() == 1
    }

    /// Check if this is a 2D result
    #[must_use]
    pub fn is_2d(&self) -> bool {
        self.param_values.len() == 2
    }

    /// Get values for parameter 1 (for backwards compatibility)
    #[must_use]
    pub fn param1_values(&self) -> &[f64] {
        self.param_values.first().map_or(&[], |v| v.as_slice())
    }

    /// Get values for parameter 2 (for backwards compatibility)
    #[must_use]
    pub fn param2_values(&self) -> &[f64] {
        self.param_values.get(1).map_or(&[], |v| v.as_slice())
    }

    /// Get label for parameter 1
    #[must_use]
    pub fn param1_label(&self) -> &str {
        self.param_labels.first().map_or("", |s| s.as_str())
    }

    /// Get label for parameter 2
    #[must_use]
    pub fn param2_label(&self) -> &str {
        self.param_labels.get(1).map_or("", |s| s.as_str())
    }

    /// Get the grid shape
    #[must_use]
    pub fn shape(&self) -> &[usize] {
        self.data.shape()
    }

    /// Get raw data at specific indices
    #[must_use]
    pub fn get(&self, indices: &[usize]) -> Option<&SweepPointData> {
        self.data.get(indices)
    }

    /// Set raw data at specific indices
    pub fn set(&mut self, indices: &[usize], value: SweepPointData) -> bool {
        self.data.set(indices, value)
    }

    /// Compute a metric value from raw data at a point using standardized inflation
    #[must_use]
    fn compute_metric_at(&self, point: &SweepPointData, metric: &AnalysisMetric) -> f64 {
        point.compute_metric_with_inflation(metric, self.birth_year, self.standard_inflation_factor)
    }

    /// Get results for a specific metric as a flat grid (for 1D/2D rendering)
    /// Returns (values, rows, cols) suitable for rendering
    #[must_use]
    pub fn get_metric_grid(&self, metric: &AnalysisMetric) -> (Vec<f64>, usize, usize) {
        let rows = self.param_values.first().map_or(0, Vec::len);
        let cols = self.param_values.get(1).map_or(1, Vec::len);

        let values: Vec<f64> = self
            .data
            .data()
            .iter()
            .map(|point| self.compute_metric_at(point, metric))
            .collect();

        (values, rows, cols)
    }

    /// Get 1D slice of a metric along a specific dimension, with other dimensions fixed
    #[must_use]
    pub fn get_metric_1d_slice(
        &self,
        metric: &AnalysisMetric,
        dim: usize,
        fixed_indices: &[Option<usize>],
    ) -> Option<Vec<(f64, f64)>> {
        let slice = self.data.slice_1d(dim, fixed_indices)?;
        let param_vals = &self.param_values[dim];

        Some(
            slice
                .into_iter()
                .enumerate()
                .map(|(i, (_, point))| (param_vals[i], self.compute_metric_at(point, metric)))
                .collect(),
        )
    }

    /// Get 2D slice of a metric for two dimensions, with others fixed
    /// Returns (values in row-major, `param1_vals`, `param2_vals`)
    #[must_use]
    pub fn get_metric_2d_slice(
        &self,
        metric: &AnalysisMetric,
        dim1: usize,
        dim2: usize,
        fixed_indices: &[Option<usize>],
    ) -> Option<(Vec<f64>, &[f64], &[f64])> {
        let (slice, _rows, _cols) = self.data.slice_2d(dim1, dim2, fixed_indices)?;
        let values: Vec<f64> = slice
            .into_iter()
            .map(|point| self.compute_metric_at(point, metric))
            .collect();
        Some((values, &self.param_values[dim1], &self.param_values[dim2]))
    }
}
