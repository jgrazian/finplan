//! Parameter sweep evaluator - runs simulations with modified parameters.
//!
//! Supports two modes of operation:
//! 1. **Combined**: Run simulations and compute metrics in one pass (`sweep_evaluate`)
//! 2. **Two-phase**: Run simulations first (`sweep_simulate`), analyze later (`analyze_results`)
//!
//! The two-phase approach allows running expensive simulations up-front and then
//! exploring the results with different metrics without re-running simulations.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::config::SimulationConfig;
use crate::error::MarketError;
use crate::model::{
    AccountSnapshot, EventTrigger, MonteCarloConfig, MonteCarloProgress, MonteCarloStats,
    MonteCarloSummary, SimulationResult, TransferAmount,
};
use crate::simulation::{monte_carlo_simulate_with_progress, monte_carlo_stats_only};

use super::{
    EffectParam, EffectTarget, SweepConfig, SweepGrid, SweepParameter, SweepPointData,
    SweepResults, SweepTarget, TriggerParam,
};

/// Progress tracking for sweep analysis
#[derive(Debug, Clone)]
pub struct SweepProgress {
    /// Completed points counter
    completed: Arc<AtomicUsize>,
    /// Total points
    total: Arc<AtomicUsize>,
    /// Cancellation flag
    cancelled: Arc<AtomicBool>,
}

impl SweepProgress {
    /// Create a new progress tracker
    #[must_use]
    pub fn new(total: usize) -> Self {
        Self {
            completed: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(total)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create from existing atomics (for TUI integration)
    pub fn from_atomics(
        completed: Arc<AtomicUsize>,
        total: Arc<AtomicUsize>,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            completed,
            total,
            cancelled,
        }
    }

    /// Get the number of completed points
    #[must_use]
    pub fn completed(&self) -> usize {
        self.completed.load(Ordering::Relaxed)
    }

    /// Get the total number of points
    #[must_use]
    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    /// Increment the completed counter
    pub fn increment(&self) {
        self.completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Reset the progress
    pub fn reset(&self, total: usize) {
        self.completed.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }

    /// Cancel the sweep
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Check if cancelled
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Create a `MonteCarloProgress` that shares this progress's atomics.
    /// This allows MC iteration progress to flow through to sweep progress.
    /// Uses accumulating mode to avoid resetting progress between sweep points.
    #[must_use]
    pub fn as_mc_progress(&self) -> MonteCarloProgress {
        MonteCarloProgress::from_atomics_accumulating(
            self.completed.clone(),
            self.cancelled.clone(),
        )
    }
}

impl Default for SweepProgress {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Raw simulation results for each point in an N-dimensional sweep grid.
///
/// This stores the full `MonteCarloSummary` for each grid point, allowing
/// metrics to be computed on-demand without re-running simulations.
#[derive(Debug, Clone)]
pub struct SweepSimulationResults {
    /// Values for each parameter dimension
    pub param_values: Vec<Vec<f64>>,
    /// Labels for each parameter
    pub param_labels: Vec<String>,
    /// N-dimensional grid of Monte Carlo summaries
    pub summaries: SweepGrid<Option<MonteCarloSummary>>,
    /// Birth year for metric computation
    pub birth_year: i16,
}

impl SweepSimulationResults {
    /// Get the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.param_values.len()
    }

    /// Get the grid shape
    #[must_use]
    pub fn shape(&self) -> &[usize] {
        self.summaries.shape()
    }

    /// Get the total number of points
    #[must_use]
    pub fn total_points(&self) -> usize {
        self.summaries.len()
    }

    /// Get the simulation summary at the given indices
    #[must_use]
    pub fn get(&self, indices: &[usize]) -> Option<&MonteCarloSummary> {
        self.summaries.get(indices).and_then(|opt| opt.as_ref())
    }

    /// Check if all simulations completed successfully
    pub fn is_complete(&self) -> bool {
        self.summaries.data().iter().all(Option::is_some)
    }

    /// Count completed simulations
    #[must_use]
    pub fn completed_count(&self) -> usize {
        self.summaries
            .data()
            .iter()
            .filter(|opt| opt.is_some())
            .count()
    }

    /// Compute metrics for all points using the given metric definitions.
    /// Returns `SweepResults` with raw data for each grid point.
    ///
    /// Note: The `metrics` parameter is no longer used since raw data is stored
    /// and metrics are computed on-demand. It's kept for API compatibility.
    #[must_use]
    pub fn compute_all_metrics(&self, _metrics: &[super::AnalysisMetric]) -> SweepResults {
        let mut results = SweepResults::new(
            self.param_values.clone(),
            self.param_labels.clone(),
            self.birth_year,
        );

        for indices in self.summaries.indices() {
            if let Some(summary) = self.get(&indices) {
                let point_data = SweepPointData::from_summary(summary, self.birth_year);
                results.set(&indices, point_data);
            }
        }

        // Compute standardized inflation factor from all points for consistent metric display
        results.finalize_inflation_factor();

        results
    }

    /// Compute a single metric for all points, returning a grid of values.
    #[must_use]
    pub fn compute_metric_grid(&self, metric: &super::AnalysisMetric) -> SweepGrid<f64> {
        let mut grid = SweepGrid::new(self.summaries.shape().to_vec(), 0.0);

        for indices in self.summaries.indices() {
            if let Some(summary) = self.get(&indices) {
                let point_data = SweepPointData::from_summary(summary, self.birth_year);
                let value = point_data.compute_metric(metric, self.birth_year);
                grid.set(&indices, value);
            }
        }

        grid
    }
}

/// Memory-efficient sweep results that store only stats and seeds for on-demand percentile runs.
///
/// This struct reduces memory usage by ~1000x compared to `SweepSimulationResults` by storing
/// only `MonteCarloStats` and percentile seeds for each grid point. When a full `SimulationResult`
/// is needed (e.g., to view percentile charts), the simulation is re-run on demand using the
/// stored seed.
///
/// # Memory Comparison
/// - `SweepSimulationResults`: Stores full `MonteCarloSummary` (including 5 full `SimulationResult`s)
///   per grid point. For a 10×10 sweep, this is 500 full results (~25 MB+).
/// - `LazySweepResults`: Stores only `MonteCarloStats` (~200 bytes) and 5 seeds (~80 bytes)
///   per grid point. For a 10×10 sweep, this is ~28 KB.
#[derive(Debug, Clone)]
pub struct LazySweepResults {
    /// Values for each parameter dimension
    pub param_values: Vec<Vec<f64>>,
    /// Labels for each parameter
    pub param_labels: Vec<String>,
    /// Stats for each grid point (compact)
    pub stats: SweepGrid<MonteCarloStats>,
    /// Seeds for percentile runs: (percentile, seed) pairs per grid point
    pub percentile_seeds: SweepGrid<Vec<(f64, u64)>>,
    /// Birth year for metric computation
    pub birth_year: i16,
    /// Base config for re-running simulations (stored once)
    base_config: SimulationConfig,
    /// Sweep config for parameter reconstruction
    sweep_config: SweepConfig,
}

impl LazySweepResults {
    /// Get the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.param_values.len()
    }

    /// Get the grid shape
    #[must_use]
    pub fn shape(&self) -> &[usize] {
        self.stats.shape()
    }

    /// Get the total number of points
    #[must_use]
    pub fn total_points(&self) -> usize {
        self.stats.len()
    }

    /// Get stats at the given indices
    #[must_use]
    pub fn get_stats(&self, indices: &[usize]) -> Option<&MonteCarloStats> {
        self.stats.get(indices)
    }

    /// Get percentile seeds at the given indices
    #[must_use]
    pub fn get_seeds(&self, indices: &[usize]) -> Option<&Vec<(f64, u64)>> {
        self.percentile_seeds.get(indices)
    }

    /// Check if all simulations completed successfully
    #[must_use]
    pub fn is_complete(&self) -> bool {
        // A grid point is complete if it has stats with iterations > 0
        self.stats.data().iter().all(|s| s.num_iterations > 0)
    }

    /// Reconstruct the `SimulationConfig` for a specific grid position
    fn reconstruct_config(&self, indices: &[usize]) -> Result<SimulationConfig, MarketError> {
        let mut config = self.base_config.clone();
        for (dim, &idx) in indices.iter().enumerate() {
            let value = self.param_values[dim][idx];
            config = apply_parameter(&config, &self.sweep_config.parameters[dim], value)?;
        }
        Ok(config)
    }

    /// Get a percentile run on demand (re-runs simulation with stored seed).
    ///
    /// This is the key method for lazy computation - it reconstructs the config
    /// for the given grid position and re-runs the simulation with the specific
    /// seed that produced the requested percentile result.
    pub fn get_percentile_run(
        &self,
        indices: &[usize],
        percentile: f64,
    ) -> Result<SimulationResult, MarketError> {
        let seeds = self
            .percentile_seeds
            .get(indices)
            .ok_or_else(|| MarketError::Config("Invalid grid indices".to_string()))?;

        let (_, seed) = seeds
            .iter()
            .find(|(p, _)| (*p - percentile).abs() < 0.01)
            .ok_or_else(|| {
                MarketError::Config(format!("Percentile {percentile} not found in stored seeds"))
            })?;

        let config = self.reconstruct_config(indices)?;
        crate::simulation::simulate(&config, *seed)
    }

    /// Compute metrics for all points using stats (no re-simulation needed for most metrics).
    ///
    /// This method computes metrics efficiently by using the stored `MonteCarloStats`
    /// directly. For metrics that need full simulation data (like `MaxDrawdown`, `NetWorthAtAge`),
    /// it lazily fetches the P50 run and extracts the needed data.
    #[must_use]
    pub fn compute_all_metrics(&self, _metrics: &[super::AnalysisMetric]) -> SweepResults {
        let mut results = SweepResults::new(
            self.param_values.clone(),
            self.param_labels.clone(),
            self.birth_year,
        );

        for indices in self.stats.indices() {
            let stats = self.stats.get(&indices).unwrap();
            let point_data = self.stats_to_point_data(stats, &indices);
            results.set(&indices, point_data);
        }

        // Compute standardized inflation factor from all points for consistent metric display
        results.finalize_inflation_factor();

        results
    }

    /// Convert `MonteCarloStats` to `SweepPointData`.
    ///
    /// For basic metrics (`success_rate`, percentiles), this uses stored stats directly.
    /// For metrics requiring full simulation data, it would need to fetch the P50 run.
    fn stats_to_point_data(&self, stats: &MonteCarloStats, indices: &[usize]) -> SweepPointData {
        // For metrics that need P50 simulation data (NetWorthAtAge, MaxDrawdown, LifetimeTaxes),
        // we lazily fetch the P50 run. This is cached by the caller if needed.
        let (p50_yearly_net_worth, p50_lifetime_taxes, final_inflation_factor) =
            if let Ok(p50_result) = self.get_percentile_run(indices, 0.5) {
                let yearly: Vec<(i16, f64)> = p50_result
                    .wealth_snapshots
                    .iter()
                    .map(|s| {
                        let total: f64 = s
                            .accounts
                            .iter()
                            .map(|a: &AccountSnapshot| a.total_value())
                            .sum();
                        (s.date.year(), total)
                    })
                    .collect();
                let taxes: f64 = p50_result.yearly_taxes.iter().map(|t| t.total_tax).sum();
                let inflation = p50_result
                    .cumulative_inflation
                    .last()
                    .copied()
                    .unwrap_or(1.0);
                (yearly, taxes, inflation)
            } else {
                (Vec::new(), 0.0, 1.0)
            };

        SweepPointData {
            success_rate: stats.success_rate,
            num_iterations: stats.num_iterations,
            final_percentiles: stats.percentile_values.clone(),
            p50_yearly_net_worth,
            p50_lifetime_taxes,
            final_inflation_factor,
        }
    }

    /// Compute a single metric for all points, returning a grid of values.
    ///
    /// For metrics that only need stats (`SuccessRate`, Percentile), this is very fast.
    /// For metrics needing full results, it fetches P50 runs lazily.
    #[must_use]
    pub fn compute_metric_grid(&self, metric: &super::AnalysisMetric) -> SweepGrid<f64> {
        let mut grid = SweepGrid::new(self.stats.shape().to_vec(), 0.0);

        for indices in self.stats.indices() {
            let stats = self.stats.get(&indices).unwrap();

            // Fast path for stats-only metrics
            let value = match metric {
                super::AnalysisMetric::SuccessRate => stats.success_rate,
                super::AnalysisMetric::Percentile { percentile } => {
                    let target_p = f64::from(*percentile) / 100.0;
                    stats
                        .percentile_values
                        .iter()
                        .find(|(p, _)| (*p - target_p).abs() < 0.01)
                        .map_or(0.0, |(_, v)| *v)
                }
                // Slow path: need to fetch P50 run
                _ => {
                    let point_data = self.stats_to_point_data(stats, &indices);
                    point_data.compute_metric(metric, self.birth_year)
                }
            };

            grid.set(&indices, value);
        }

        grid
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
}

/// Run simulations for all sweep points without computing metrics.
///
/// This is the first phase of two-phase sweep analysis. Call `compute_all_metrics()`
/// on the returned `SweepSimulationResults` to compute metrics afterward.
///
/// # Example
/// ```ignore
/// // Phase 1: Run simulations
/// let sim_results = sweep_simulate(&config, &sweep_config, Some(&progress))?;
///
/// // Phase 2: Compute metrics (can be done multiple times with different metrics)
/// let results = sim_results.compute_all_metrics(&[AnalysisMetric::SuccessRate]);
/// ```
pub fn sweep_simulate(
    base_config: &SimulationConfig,
    sweep_config: &SweepConfig,
    progress: Option<&SweepProgress>,
) -> Result<SweepSimulationResults, MarketError> {
    // Validate configuration
    if sweep_config.parameters.is_empty() {
        return Err(MarketError::Config(
            "At least one sweep parameter required".to_string(),
        ));
    }

    // Get parameter values and labels
    let param_values = sweep_config.all_sweep_values();
    let param_labels = sweep_config.labels();
    let shape = sweep_config.grid_shape();

    // Monte Carlo config for each point
    let mc_config = MonteCarloConfig {
        iterations: sweep_config.mc_iterations,
        percentiles: vec![0.05, 0.25, 0.50, 0.75, 0.95],
        compute_mean: false,
        parallel_batches: sweep_config.parallel_batches,
        seed: sweep_config.seed,
        ..Default::default()
    };

    // Reset progress to track total iterations (points × iterations per point)
    let total_points = sweep_config.total_points();
    let total_iterations = total_points * mc_config.iterations;
    if let Some(p) = progress {
        p.reset(total_iterations);
    }

    // Extract birth year
    let birth_year = base_config.birth_date.map_or(1980, jiff::civil::Date::year);

    // Create the result grid
    let mut summaries: SweepGrid<Option<MonteCarloSummary>> = SweepGrid::new(shape.clone(), None);

    // Iterate through all grid points
    for indices in summaries.indices() {
        if let Some(p) = progress
            && p.is_cancelled()
        {
            return Err(MarketError::Cancelled);
        }

        // Apply all parameters for this grid point
        let mut modified_config = base_config.clone();
        // Disable ledger collection for sweep simulations to save CPU/memory
        modified_config.collect_ledger = false;
        for (dim, &idx) in indices.iter().enumerate() {
            let value = param_values[dim][idx];
            modified_config =
                apply_parameter(&modified_config, &sweep_config.parameters[dim], value)?;
        }

        // Run Monte Carlo simulation with progress shared from sweep
        // Each MC iteration increments the shared progress counter
        let mc_progress = progress
            .map(SweepProgress::as_mc_progress)
            .unwrap_or_default();
        let summary =
            monte_carlo_simulate_with_progress(&modified_config, &mc_config, &mc_progress)?;

        summaries.set(&indices, Some(summary));
    }

    Ok(SweepSimulationResults {
        param_values,
        param_labels,
        summaries,
        birth_year,
    })
}

/// Run a parameter sweep analysis (combined simulation + metric computation).
///
/// Supports N-dimensional sweeps. For each point in the grid, runs a Monte Carlo
/// simulation and computes the requested metrics.
///
/// This function uses memory-efficient lazy sweep simulation internally, which stores
/// only stats and seeds during the sweep. Percentile runs are reconstructed on demand
/// when computing metrics that need them (e.g., `NetWorthAtAge`, `MaxDrawdown`).
///
/// For fine-grained control or to analyze results with different metrics, use
/// `sweep_simulate_lazy()` and call `compute_all_metrics()` on the result.
///
/// Returns `SweepResults` containing computed metrics for each point.
pub fn sweep_evaluate(
    base_config: &SimulationConfig,
    sweep_config: &SweepConfig,
    progress: Option<&SweepProgress>,
) -> Result<SweepResults, MarketError> {
    // Use memory-efficient lazy sweep simulation
    let lazy_results = sweep_simulate_lazy(base_config, sweep_config, progress)?;

    // Compute metrics (this will lazily fetch P50 runs when needed)
    Ok(lazy_results.compute_all_metrics(&sweep_config.metrics))
}

/// Memory-efficient sweep simulation that stores only stats and seeds.
///
/// This function is similar to `sweep_simulate` but uses much less memory by storing
/// only `MonteCarloStats` and percentile seeds (instead of full `MonteCarloSummary`
/// with complete `SimulationResult` objects).
///
/// Use this for large sweep grids where memory is a concern. Percentile runs can be
/// reconstructed on demand using `LazySweepResults::get_percentile_run()`.
///
/// # Example
/// ```ignore
/// // Run memory-efficient sweep
/// let lazy_results = sweep_simulate_lazy(&config, &sweep_config, Some(&progress))?;
///
/// // Get stats directly (fast)
/// let stats = lazy_results.get_stats(&[0, 5]).unwrap();
///
/// // Get full P50 run on demand (re-runs simulation)
/// let p50_result = lazy_results.get_percentile_run(&[0, 5], 0.5)?;
/// ```
pub fn sweep_simulate_lazy(
    base_config: &SimulationConfig,
    sweep_config: &SweepConfig,
    progress: Option<&SweepProgress>,
) -> Result<LazySweepResults, MarketError> {
    // Validate configuration
    if sweep_config.parameters.is_empty() {
        return Err(MarketError::Config(
            "At least one sweep parameter required".to_string(),
        ));
    }

    // Get parameter values and labels
    let param_values = sweep_config.all_sweep_values();
    let param_labels = sweep_config.labels();
    let shape = sweep_config.grid_shape();

    // Monte Carlo config for each point
    let mc_config = MonteCarloConfig {
        iterations: sweep_config.mc_iterations,
        percentiles: vec![0.05, 0.25, 0.50, 0.75, 0.95],
        compute_mean: false,
        parallel_batches: sweep_config.parallel_batches,
        seed: sweep_config.seed,
        ..Default::default()
    };

    // Reset progress to track total iterations (points × iterations per point)
    let total_points = sweep_config.total_points();
    let total_iterations = total_points * mc_config.iterations;
    if let Some(p) = progress {
        p.reset(total_iterations);
    }

    // Extract birth year
    let birth_year = base_config.birth_date.map_or(1980, jiff::civil::Date::year);

    // Create result grids with compact default values
    let default_stats = MonteCarloStats {
        num_iterations: 0,
        success_rate: 0.0,
        mean_final_net_worth: 0.0,
        std_dev_final_net_worth: 0.0,
        min_final_net_worth: 0.0,
        max_final_net_worth: 0.0,
        percentile_values: Vec::new(),
        converged: None,
        convergence_metric: None,
        convergence_value: None,
    };
    let mut stats_grid: SweepGrid<MonteCarloStats> =
        SweepGrid::new(shape.clone(), default_stats.clone());
    let mut seeds_grid: SweepGrid<Vec<(f64, u64)>> = SweepGrid::new(shape.clone(), Vec::new());

    // Iterate through all grid points
    for indices in stats_grid.indices() {
        if let Some(p) = progress
            && p.is_cancelled()
        {
            return Err(MarketError::Cancelled);
        }

        // Apply all parameters for this grid point
        let mut modified_config = base_config.clone();
        // Disable ledger collection for sweep simulations to save CPU/memory
        modified_config.collect_ledger = false;
        for (dim, &idx) in indices.iter().enumerate() {
            let value = param_values[dim][idx];
            modified_config =
                apply_parameter(&modified_config, &sweep_config.parameters[dim], value)?;
        }

        // Run Monte Carlo simulation with stats-only mode
        // This skips Phase 2 (re-running percentile seeds) and returns seeds instead
        let mc_progress = progress
            .map(SweepProgress::as_mc_progress)
            .unwrap_or_default();
        let (stats, percentile_seeds) =
            monte_carlo_stats_only(&modified_config, &mc_config, &mc_progress)?;

        stats_grid.set(&indices, stats);
        seeds_grid.set(&indices, percentile_seeds);
    }

    Ok(LazySweepResults {
        param_values,
        param_labels,
        stats: stats_grid,
        percentile_seeds: seeds_grid,
        birth_year,
        base_config: base_config.clone(),
        sweep_config: sweep_config.clone(),
    })
}

/// Apply a parameter value to a simulation config
fn apply_parameter(
    config: &SimulationConfig,
    param: &SweepParameter,
    value: f64,
) -> Result<SimulationConfig, MarketError> {
    let mut modified = config.clone();

    // Find the event
    let event_idx = (param.event_id.0 as usize).saturating_sub(1);
    if event_idx >= modified.events.len() {
        return Err(MarketError::Config(format!(
            "Event {} not found",
            param.event_id.0
        )));
    }

    match &param.target {
        SweepTarget::Trigger(trigger_param) => {
            apply_trigger_param(
                &mut modified.events[event_idx].trigger,
                trigger_param,
                value,
            )?;
        }
        SweepTarget::Effect {
            param: effect_param,
            target,
        } => {
            apply_effect_param(
                &mut modified.events[event_idx].effects,
                effect_param,
                target,
                value,
            )?;
        }
        SweepTarget::AssetAllocation { .. } => {
            // This would modify asset allocation in the account
            // For now, skip - this is more complex and requires special handling
        }
    }

    Ok(modified)
}

/// Apply a trigger parameter modification
fn apply_trigger_param(
    trigger: &mut EventTrigger,
    param: &TriggerParam,
    value: f64,
) -> Result<(), MarketError> {
    match param {
        TriggerParam::Age => {
            if let EventTrigger::Age { years, .. } = trigger {
                *years = value as u8;
            } else {
                return Err(MarketError::Config(
                    "Target trigger is not an Age trigger".to_string(),
                ));
            }
        }
        TriggerParam::Date => {
            if let EventTrigger::Date(date) = trigger {
                // Modify the year while preserving month/day
                let new_year = value as i16;
                *date = jiff::civil::date(new_year, date.month(), date.day());
            } else {
                return Err(MarketError::Config(
                    "Target trigger is not a Date trigger".to_string(),
                ));
            }
        }
        TriggerParam::RepeatingStart(inner_param) => {
            if let EventTrigger::Repeating {
                start_condition: Some(start),
                ..
            } = trigger
            {
                apply_trigger_param(start, inner_param, value)?;
            } else {
                return Err(MarketError::Config(
                    "Target trigger is not a Repeating trigger with start condition".to_string(),
                ));
            }
        }
        TriggerParam::RepeatingEnd(inner_param) => {
            if let EventTrigger::Repeating {
                end_condition: Some(end),
                ..
            } = trigger
            {
                apply_trigger_param(end, inner_param, value)?;
            } else {
                return Err(MarketError::Config(
                    "Target trigger is not a Repeating trigger with end condition".to_string(),
                ));
            }
        }
    }
    Ok(())
}

/// Apply an effect parameter modification
fn apply_effect_param(
    effects: &mut [crate::model::EventEffect],
    param: &EffectParam,
    target: &EffectTarget,
    value: f64,
) -> Result<(), MarketError> {
    use crate::model::EventEffect;

    // Find the target effect
    let effect_idx = match target {
        EffectTarget::FirstEligible => effects.iter().position(has_sweepable_amount),
        EffectTarget::Index(idx) => {
            if *idx < effects.len() {
                Some(*idx)
            } else {
                None
            }
        }
    };

    let Some(idx) = effect_idx else {
        return Err(MarketError::Config(
            "No eligible effect found for sweep".to_string(),
        ));
    };

    // Modify the effect's amount
    let effect = &mut effects[idx];
    match effect {
        EventEffect::Income { amount, .. }
        | EventEffect::Expense { amount, .. }
        | EventEffect::AssetPurchase { amount, .. }
        | EventEffect::AssetSale { amount, .. }
        | EventEffect::Sweep { amount, .. }
        | EventEffect::AdjustBalance { amount, .. }
        | EventEffect::CashTransfer { amount, .. } => {
            apply_amount_param(amount, param, value)?;
        }
        _ => {
            return Err(MarketError::Config(
                "Effect does not have a sweepable amount".to_string(),
            ));
        }
    }

    Ok(())
}

/// Check if an effect has a sweepable amount
fn has_sweepable_amount(effect: &crate::model::EventEffect) -> bool {
    use crate::model::EventEffect;
    matches!(
        effect,
        EventEffect::Income { .. }
            | EventEffect::Expense { .. }
            | EventEffect::AssetPurchase { .. }
            | EventEffect::AssetSale { .. }
            | EventEffect::Sweep { .. }
            | EventEffect::AdjustBalance { .. }
            | EventEffect::CashTransfer { .. }
    )
}

/// Apply a parameter modification to a `TransferAmount`
fn apply_amount_param(
    amount: &mut TransferAmount,
    param: &EffectParam,
    value: f64,
) -> Result<(), MarketError> {
    match param {
        EffectParam::Value => {
            // Unwrap InflationAdjusted if present, modify Fixed value
            match amount {
                TransferAmount::Fixed(v) => {
                    *v = value;
                }
                TransferAmount::InflationAdjusted(inner) => {
                    if let TransferAmount::Fixed(v) = inner.as_mut() {
                        *v = value;
                    } else {
                        return Err(MarketError::Config(
                            "InflationAdjusted does not contain a Fixed amount".to_string(),
                        ));
                    }
                }
                _ => {
                    return Err(MarketError::Config(
                        "Amount is not a Fixed or InflationAdjusted(Fixed) value".to_string(),
                    ));
                }
            }
        }
        EffectParam::Multiplier => {
            if let TransferAmount::Scale(multiplier, _) = amount {
                *multiplier = value;
            } else {
                return Err(MarketError::Config(
                    "Amount is not a Scale type".to_string(),
                ));
            }
        }
    }
    Ok(())
}
