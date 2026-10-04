//! Where an analysis gets its Monte Carlo runs from.
//!
//! A sweep, a goal seek and a what-if all reduce to "simulate this config, this
//! many times, with this seed" over and over. [`McRunner`] is that one call, so
//! the loops that decide *which* configs to simulate (the grid, the bisection,
//! the stack of overrides) do not care who simulates them: [`ProgressRunner`]
//! is the engine's own call, rayon-parallel when the `parallel` feature is on
//! and sequential otherwise, and a caller on another substrate (a browser
//! coordinating workers) supplies its own.

use crate::config::SimulationConfig;
use crate::error::SimulationError;
use crate::model::{MonteCarloConfig, MonteCarloStats, MonteCarloSummary};
use crate::simulation::{monte_carlo_simulate_with_progress, monte_carlo_stats_only};

use super::SweepProgress;

/// First-pass output of a stats-only run: the stats and the seeds of the
/// percentile paths (as `(percentile, seed)`).
pub type StatsRun = (MonteCarloStats, Vec<(f64, u64)>);

/// Runs Monte Carlo simulations on behalf of an analysis.
///
/// Implementations own progress and cancellation: the analysis announces how
/// much work it expects with [`begin`](Self::begin), asks
/// [`cancelled`](Self::cancelled) between simulations, and counts nothing
/// itself. Results must depend only on the arguments, so a sequential runner
/// and a parallel one agree for the same seed and batch plan.
pub trait McRunner {
    /// Stats and percentile seeds only: no second pass rebuilding percentile
    /// paths. Every analysis but a what-if reads nothing more.
    fn stats(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<StatsRun, SimulationError>;

    /// The full summary, including the real-dollar envelope a what-if's fans
    /// are read from.
    fn summary(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<MonteCarloSummary, SimulationError>;

    /// Whether the analysis should stop. Checked between simulations.
    fn cancelled(&self) -> bool;

    /// An analysis phase is starting that will run `total_iterations`
    /// simulations; a runner that reports progress restarts its count against
    /// this. The default does nothing.
    fn begin(&mut self, total_iterations: usize) {
        let _ = total_iterations;
    }
}

/// The engine's own runner, reporting through a [`SweepProgress`] when given
/// one. What [`sweep_simulate_lazy`](super::sweep_simulate_lazy) and
/// [`solve`](super::solve) use.
#[derive(Debug, Clone, Copy)]
pub struct ProgressRunner<'a> {
    progress: Option<&'a SweepProgress>,
}

impl<'a> ProgressRunner<'a> {
    #[must_use]
    pub fn new(progress: Option<&'a SweepProgress>) -> Self {
        Self { progress }
    }
}

impl McRunner for ProgressRunner<'_> {
    fn stats(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<StatsRun, SimulationError> {
        let progress = self
            .progress
            .map(SweepProgress::as_mc_progress)
            .unwrap_or_default();
        monte_carlo_stats_only(config, mc, &progress)
    }

    fn summary(
        &mut self,
        config: &SimulationConfig,
        mc: &MonteCarloConfig,
    ) -> Result<MonteCarloSummary, SimulationError> {
        let progress = self
            .progress
            .map(SweepProgress::as_mc_progress)
            .unwrap_or_default();
        monte_carlo_simulate_with_progress(config, mc, &progress)
    }

    fn cancelled(&self) -> bool {
        self.progress.is_some_and(SweepProgress::is_cancelled)
    }

    fn begin(&mut self, total_iterations: usize) {
        if let Some(p) = self.progress {
            p.reset(total_iterations);
        }
    }
}
