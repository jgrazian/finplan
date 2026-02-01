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
    EventTrigger, MonteCarloConfig, MonteCarloProgress, MonteCarloSummary, TransferAmount,
};
use crate::simulation::monte_carlo_simulate_with_progress;

use super::{
    AnalysisMetric, EffectParam, EffectTarget, SweepConfig, SweepGrid, SweepParameter,
    SweepResults, SweepTarget, TriggerParam, compute_metrics,
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
    pub fn completed(&self) -> usize {
        self.completed.load(Ordering::Relaxed)
    }

    /// Get the total number of points
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
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Default for SweepProgress {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Raw simulation results for each point in an N-dimensional sweep grid.
///
/// This stores the full MonteCarloSummary for each grid point, allowing
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
    pub fn ndim(&self) -> usize {
        self.param_values.len()
    }

    /// Get the grid shape
    pub fn shape(&self) -> &[usize] {
        self.summaries.shape()
    }

    /// Get the total number of points
    pub fn total_points(&self) -> usize {
        self.summaries.len()
    }

    /// Get the simulation summary at the given indices
    pub fn get(&self, indices: &[usize]) -> Option<&MonteCarloSummary> {
        self.summaries.get(indices).and_then(|opt| opt.as_ref())
    }

    /// Check if all simulations completed successfully
    pub fn is_complete(&self) -> bool {
        self.summaries.data().iter().all(|opt| opt.is_some())
    }

    /// Count completed simulations
    pub fn completed_count(&self) -> usize {
        self.summaries
            .data()
            .iter()
            .filter(|opt| opt.is_some())
            .count()
    }

    /// Compute metrics for all points using the given metric definitions.
    /// Returns SweepResults with computed metrics for each grid point.
    pub fn compute_all_metrics(&self, metrics: &[AnalysisMetric]) -> SweepResults {
        let mut results = SweepResults::new(self.param_values.clone(), self.param_labels.clone());

        for indices in self.summaries.indices() {
            if let Some(summary) = self.get(&indices) {
                let computed = compute_metrics(summary, metrics, self.birth_year);
                results.set(&indices, computed);
            }
        }

        results
    }

    /// Compute a single metric for all points, returning a grid of values.
    pub fn compute_metric_grid(&self, metric: &AnalysisMetric) -> SweepGrid<f64> {
        let mut grid = SweepGrid::new(self.summaries.shape().to_vec(), 0.0);

        for indices in self.summaries.indices() {
            if let Some(summary) = self.get(&indices) {
                let computed =
                    compute_metrics(summary, std::slice::from_ref(metric), self.birth_year);
                let value = match metric {
                    AnalysisMetric::SuccessRate => computed.success_rate.unwrap_or(0.0),
                    AnalysisMetric::NetWorthAtAge { .. } => {
                        computed.net_worth_at_age.unwrap_or(0.0)
                    }
                    AnalysisMetric::Percentile { .. } => computed.percentile_value.unwrap_or(0.0),
                    AnalysisMetric::LifetimeTaxes => computed.lifetime_taxes.unwrap_or(0.0),
                    AnalysisMetric::MaxDrawdown => computed.max_drawdown.unwrap_or(0.0),
                    AnalysisMetric::SafeWithdrawalRate { .. } => {
                        computed.safe_withdrawal_rate.unwrap_or(0.0)
                    }
                };
                grid.set(&indices, value);
            }
        }

        grid
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

    let total_points = sweep_config.total_points();
    if let Some(p) = progress {
        p.reset(total_points);
    }

    // Extract birth year
    let birth_year = base_config.birth_date.map(|d| d.year()).unwrap_or(1980);

    // Create the result grid
    let mut summaries: SweepGrid<Option<MonteCarloSummary>> = SweepGrid::new(shape.clone(), None);

    // Monte Carlo config for each point
    let mc_config = MonteCarloConfig {
        iterations: sweep_config.mc_iterations,
        percentiles: vec![0.05, 0.50, 0.95],
        compute_mean: false,
        parallel_batches: sweep_config.parallel_batches,
        ..Default::default()
    };

    // Iterate through all grid points
    for indices in summaries.indices() {
        if let Some(p) = progress
            && p.is_cancelled()
        {
            return Err(MarketError::Cancelled);
        }

        // Apply all parameters for this grid point
        let mut modified_config = base_config.clone();
        for (dim, &idx) in indices.iter().enumerate() {
            let value = param_values[dim][idx];
            modified_config =
                apply_parameter(&modified_config, &sweep_config.parameters[dim], value)?;
        }

        // Run Monte Carlo simulation
        let mc_progress = MonteCarloProgress::new();
        let summary =
            monte_carlo_simulate_with_progress(&modified_config, &mc_config, &mc_progress)?;

        summaries.set(&indices, Some(summary));

        if let Some(p) = progress {
            p.increment();
        }
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
/// For fine-grained control or to analyze results with different metrics, use
/// `sweep_simulate()` followed by `SweepSimulationResults::compute_all_metrics()`.
///
/// Returns SweepResults containing computed metrics for each point.
pub fn sweep_evaluate(
    base_config: &SimulationConfig,
    sweep_config: &SweepConfig,
    progress: Option<&SweepProgress>,
) -> Result<SweepResults, MarketError> {
    // Run simulations first
    let sim_results = sweep_simulate(base_config, sweep_config, progress)?;

    // Compute metrics
    Ok(sim_results.compute_all_metrics(&sweep_config.metrics))
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

/// Apply a parameter modification to a TransferAmount
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
