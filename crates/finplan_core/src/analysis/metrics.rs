//! Analysis metrics that can be computed from simulation results.

use std::collections::HashMap;

use crate::model::{MonteCarloSummary, SimulationResult};
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
    pub fn label(&self) -> String {
        match self {
            Self::SuccessRate => "Success Rate".to_string(),
            Self::NetWorthAtAge { age } => format!("Net Worth at {}", age),
            Self::Percentile { percentile } => format!("P{}", percentile),
            Self::LifetimeTaxes => "Lifetime Taxes".to_string(),
            Self::MaxDrawdown => "Max Drawdown".to_string(),
            Self::SafeWithdrawalRate {
                target_success_rate,
            } => format!("SWR @ {:.0}%", target_success_rate * 100.0),
        }
    }

    /// Get a short label suitable for chart axes
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
    /// Final net worth percentiles: (percentile as 0-1, value)
    /// Typically includes 0.05, 0.25, 0.50, 0.75, 0.95
    pub final_percentiles: Vec<(f64, f64)>,
    /// P50 yearly net worth: (year, net_worth) - for NetWorthAtAge and MaxDrawdown
    pub p50_yearly_net_worth: Vec<(i16, f64)>,
    /// Total lifetime taxes from P50 run
    pub p50_lifetime_taxes: f64,
}

impl Default for SweepPointData {
    fn default() -> Self {
        Self {
            success_rate: 0.0,
            num_iterations: 0,
            final_percentiles: Vec::new(),
            p50_yearly_net_worth: Vec::new(),
            p50_lifetime_taxes: 0.0,
        }
    }
}

impl SweepPointData {
    /// Extract SweepPointData from a MonteCarloSummary
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
                        let total: f64 = s.accounts.iter().map(|a| a.total_value()).sum();
                        (s.date.year(), total)
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Extract lifetime taxes from P50 run
        let p50_lifetime_taxes = p50_run
            .map(|result| result.yearly_taxes.iter().map(|t| t.total_tax).sum())
            .unwrap_or(0.0);

        Self {
            success_rate,
            num_iterations,
            final_percentiles,
            p50_yearly_net_worth,
            p50_lifetime_taxes,
        }
    }

    /// Compute a specific metric from this raw data
    pub fn compute_metric(&self, metric: &AnalysisMetric, birth_year: i16) -> f64 {
        match metric {
            AnalysisMetric::SuccessRate => self.success_rate,

            AnalysisMetric::Percentile { percentile } => {
                let target_p = *percentile as f64 / 100.0;
                self.final_percentiles
                    .iter()
                    .find(|(p, _)| (*p - target_p).abs() < 0.01)
                    .map(|(_, v)| *v)
                    .unwrap_or(0.0)
            }

            AnalysisMetric::NetWorthAtAge { age } => {
                let target_year = birth_year + *age as i16;
                self.p50_yearly_net_worth
                    .iter()
                    .find(|(year, _)| *year == target_year)
                    .map(|(_, nw)| *nw)
                    .unwrap_or(0.0)
            }

            AnalysisMetric::LifetimeTaxes => self.p50_lifetime_taxes,

            AnalysisMetric::MaxDrawdown => self.compute_max_drawdown(),

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

    /// Convert to legacy ComputedMetrics format (for compatibility)
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
                    let target_year = birth_year + *age as i16;
                    result.net_worth_at_age = p50_result
                        .wealth_snapshots
                        .iter()
                        .find(|s| s.date.year() == target_year)
                        .map(snapshot_total_value);
                }
            }
            AnalysisMetric::Percentile { percentile } => {
                let target_p = *percentile as f64 / 100.0;
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
    snapshot.accounts.iter().map(|acc| acc.total_value()).sum()
}

/// Compute maximum drawdown from a simulation result
/// Returns the drawdown as a positive fraction (e.g., 0.25 = 25% drawdown)
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
}

impl SweepResults {
    /// Create new sweep results
    pub fn new(param_values: Vec<Vec<f64>>, param_labels: Vec<String>, birth_year: i16) -> Self {
        let shape: Vec<usize> = param_values.iter().map(|v| v.len()).collect();
        Self {
            param_values,
            param_labels,
            data: super::SweepGrid::new(shape, SweepPointData::default()),
            birth_year,
        }
    }

    /// Get the number of dimensions
    pub fn ndim(&self) -> usize {
        self.param_values.len()
    }

    /// Check if this is a 1D result
    pub fn is_1d(&self) -> bool {
        self.param_values.len() == 1
    }

    /// Check if this is a 2D result
    pub fn is_2d(&self) -> bool {
        self.param_values.len() == 2
    }

    /// Get values for parameter 1 (for backwards compatibility)
    pub fn param1_values(&self) -> &[f64] {
        self.param_values
            .first()
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get values for parameter 2 (for backwards compatibility)
    pub fn param2_values(&self) -> &[f64] {
        self.param_values
            .get(1)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get label for parameter 1
    pub fn param1_label(&self) -> &str {
        self.param_labels.first().map(|s| s.as_str()).unwrap_or("")
    }

    /// Get label for parameter 2
    pub fn param2_label(&self) -> &str {
        self.param_labels.get(1).map(|s| s.as_str()).unwrap_or("")
    }

    /// Get the grid shape
    pub fn shape(&self) -> &[usize] {
        self.data.shape()
    }

    /// Get raw data at specific indices
    pub fn get(&self, indices: &[usize]) -> Option<&SweepPointData> {
        self.data.get(indices)
    }

    /// Set raw data at specific indices
    pub fn set(&mut self, indices: &[usize], value: SweepPointData) -> bool {
        self.data.set(indices, value)
    }

    /// Compute a metric value from raw data at a point
    fn compute_metric_at(&self, point: &SweepPointData, metric: &AnalysisMetric) -> f64 {
        point.compute_metric(metric, self.birth_year)
    }

    /// Get results for a specific metric as a flat grid (for 1D/2D rendering)
    /// Returns (values, rows, cols) suitable for rendering
    pub fn get_metric_grid(&self, metric: &AnalysisMetric) -> (Vec<f64>, usize, usize) {
        let rows = self.param_values.first().map(|v| v.len()).unwrap_or(0);
        let cols = self.param_values.get(1).map(|v| v.len()).unwrap_or(1);

        let values: Vec<f64> = self
            .data
            .data()
            .iter()
            .map(|point| self.compute_metric_at(point, metric))
            .collect();

        (values, rows, cols)
    }

    /// Get 1D slice of a metric along a specific dimension, with other dimensions fixed
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
    /// Returns (values in row-major, param1_vals, param2_vals)
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
