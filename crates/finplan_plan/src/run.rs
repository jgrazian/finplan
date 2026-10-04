//! Run settings: what a caller may ask of a Monte Carlo run, checked and turned
//! into the engine's [`MonteCarloConfig`].
//!
//! The server (stored runs, offloaded runs) and the browser (local runs) must
//! build the very same configuration from the same settings, or the same seed
//! would describe different runs. So the pure parts live here and both call
//! them: [`CreateRun`], [`validate_run`], [`run_cost`] and [`mc_config`].
//! What a caller is *entitled* to stays with the caller: [`validate_run`] takes
//! the iteration cap as an argument (a server tier's cap; the browser passes
//! [`NO_ITERATION_CAP`]).

use finplan_core::model::{ConvergenceConfig, MonteCarloConfig};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{PlanError, PlanResult};
use crate::graph::ScenarioGraph;

/// Ceiling on a converging run, before the caller's iteration cap is applied.
///
/// A converging run is asked for by someone who does not want to pick a count,
/// so it needs an answer in the time a count would have taken. Ten thousand
/// iterations is roughly twice the largest fixed size the UI offers.
pub const CONVERGE_CEILING: i64 = 10_000;

/// The most cost units one run may be, however much budget is left.
pub const MAX_RUN_COST: i64 = 100_000_000;

/// The `entitled_max` of a caller with no iteration cap (the browser, where the
/// CPU is the visitor's). [`MAX_RUN_COST`] still bounds a run.
pub const NO_ITERATION_CAP: usize = i64::MAX as usize;

pub fn default_iterations() -> i64 {
    1000
}

/// The example runs stored alongside the envelope: a bad case, the middle and
/// a good case, at the fan's outer band rather than out at P5/P95, where a
/// path is often a degenerate wipe-out or a runaway.
pub fn default_percentiles() -> Vec<f64> {
    vec![0.10, 0.50, 0.90]
}

pub fn default_batch() -> i64 {
    100
}

pub fn default_parallel() -> i64 {
    4
}

fn yes() -> bool {
    true
}

/// The body of `POST /scenarios/{id}/runs`, and the settings of a local run.
/// Every field has a default, so `{}` is a run.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateRun {
    #[serde(default = "default_iterations")]
    pub iterations: i64,
    #[serde(default = "default_percentiles")]
    pub percentiles: Vec<f64>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default = "default_batch")]
    pub batch_size: i64,
    #[serde(default = "default_parallel")]
    pub parallel_batches: i64,
    #[serde(default = "yes")]
    pub compute_mean: bool,
    /// Keep sampling until the median settles instead of stopping at
    /// `iterations`, which then reads as the minimum sample to take first.
    #[serde(default)]
    pub converge: bool,
}

impl Default for CreateRun {
    /// A run as if the body were `{}`: every field its serde default.
    fn default() -> Self {
        Self {
            iterations: default_iterations(),
            percentiles: default_percentiles(),
            seed: None,
            batch_size: default_batch(),
            parallel_batches: default_parallel(),
            compute_mean: yes(),
            converge: false,
        }
    }
}

/// A run's settings once checked against what the caller may ask for.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedRun {
    /// Fixed runs: the count. Converging runs: the minimum sample, clamped to
    /// the ceiling.
    pub iterations: i64,
    /// Set only on a converging run: the most iterations it may take.
    pub ceiling: Option<i64>,
    /// Sorted, deduplicated, within 0..1.
    pub percentiles: Vec<f64>,
}

impl ValidatedRun {
    /// What the run may cost at most: a converging run's ceiling, since that
    /// is the most it can spend, else its count.
    pub fn sample(&self) -> i64 {
        self.ceiling.unwrap_or(self.iterations)
    }
}

/// The checks every run's settings pass, whether it is stored against a
/// scenario, offloaded from a local plan, or run in the browser: iterations in
/// `1..=entitled_max`, a converging run's ceiling, batch shape and percentiles.
pub fn validate_run(
    iterations: i64,
    converge: bool,
    percentiles: &[f64],
    batch_size: i64,
    parallel_batches: i64,
    entitled_max: usize,
) -> PlanResult<ValidatedRun> {
    // `usize` is 32 bits on wasm32, so compare as i64 throughout.
    let entitled_max = i64::try_from(entitled_max).unwrap_or(i64::MAX);
    if iterations < 1 {
        return Err(PlanError::invalid("iterations must be at least 1"));
    }
    if iterations > entitled_max {
        return Err(PlanError::invalid(format!(
            "iterations must not exceed {entitled_max}"
        )));
    }

    // A converging run's `iterations` is its minimum sample, so it is clamped
    // to the ceiling rather than refused: asking to look at the metric later
    // than the run is allowed to go just means looking at it once, at the end.
    let ceiling = converge.then(|| CONVERGE_CEILING.min(entitled_max));
    let iterations = match ceiling {
        Some(cap) => iterations.min(cap),
        None => iterations,
    };

    if percentiles.len() > 21
        || !(1..=10_000).contains(&batch_size)
        || !(1..=16).contains(&parallel_batches)
    {
        return Err(PlanError::invalid(
            "Use at most 21 percentiles, batch size 1–10000, and parallel batches 1–16.",
        ));
    }

    let mut percentiles = percentiles.to_vec();
    percentiles.retain(|p| (0.0..=1.0).contains(p));
    percentiles.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    percentiles.dedup();
    if percentiles.is_empty() {
        return Err(PlanError::invalid(
            "percentiles must contain at least one value in 0..1",
        ));
    }
    Ok(ValidatedRun {
        iterations,
        ceiling,
        percentiles,
    })
}

impl CreateRun {
    /// [`validate_run`] over these settings.
    pub fn validate(&self, entitled_max: usize) -> PlanResult<ValidatedRun> {
        validate_run(
            self.iterations,
            self.converge,
            &self.percentiles,
            self.batch_size,
            self.parallel_batches,
            entitled_max,
        )
    }

    /// The engine configuration for these settings, as [`validate_run`] left
    /// them. `seed` is the caller's choice (a server draws one when the
    /// request names none; the browser always supplies one).
    pub fn mc_config(&self, validated: &ValidatedRun, seed: Option<i64>) -> MonteCarloConfig {
        mc_config(
            validated.iterations,
            seed,
            self.batch_size,
            self.parallel_batches,
            self.compute_mean,
            validated.ceiling,
            validated.percentiles.clone(),
        )
    }
}

/// What a run of `sample` iterations (a converging run's ceiling, since that is
/// the most it can spend) costs on this plan: `sample x duration_years x
/// max(accounts + assets + events, 1)`. Refuses a run over [`MAX_RUN_COST`].
pub fn run_cost(graph: &ScenarioGraph, sample: i64) -> PlanResult<i64> {
    let cost = sample
        .saturating_mul(graph.scenario.duration_years)
        .saturating_mul(
            (graph.accounts.len() + graph.assets.len() + graph.events.len()).max(1) as i64,
        );
    if cost > MAX_RUN_COST {
        return Err(PlanError::invalid(
            "Run is too large. Reduce iterations, duration, or plan complexity.",
        ));
    }
    Ok(cost)
}

/// The engine settings of a run, shared by stored runs, offloaded compute jobs
/// and local runs, so the same settings and seed give the same results in all.
///
/// On a converging run (`converge_ceiling` set) `iterations` is the minimum
/// sample before the metric is tested, and the ceiling is the most it may take.
pub fn mc_config(
    iterations: i64,
    seed: Option<i64>,
    batch_size: i64,
    parallel_batches: i64,
    compute_mean: bool,
    converge_ceiling: Option<i64>,
    percentiles: Vec<f64>,
) -> MonteCarloConfig {
    MonteCarloConfig {
        iterations: iterations as usize,
        percentiles: if percentiles.is_empty() {
            default_percentiles()
        } else {
            percentiles
        },
        compute_mean,
        convergence: converge_ceiling.map(|ceiling| ConvergenceConfig {
            max_iterations: ceiling as usize,
            relative_threshold: 0.01,
            ..ConvergenceConfig::default()
        }),
        batch_size: batch_size as usize,
        parallel_batches: parallel_batches as usize,
        seed: seed.map(|s| s as u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_body_is_the_default_run() {
        let run: CreateRun = serde_json::from_str("{}").unwrap();
        let validated = run.validate(NO_ITERATION_CAP).unwrap();
        assert_eq!(validated.iterations, 1000);
        assert_eq!(validated.ceiling, None);
        assert_eq!(validated.percentiles, vec![0.1, 0.5, 0.9]);
        let config = run.mc_config(&validated, Some(7));
        assert_eq!(config.seed, Some(7));
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.parallel_batches, 4);
        assert!(config.compute_mean);
        assert!(config.convergence.is_none());
    }

    #[test]
    fn validation_normalises_and_bounds() {
        let ok = validate_run(10, false, &[0.9, 0.5, 2.0, 0.5, -1.0], 100, 4, 1000).unwrap();
        assert_eq!(ok.percentiles, vec![0.5, 0.9]);
        assert!(validate_run(0, false, &[0.5], 100, 4, 1000).is_err());
        assert!(validate_run(1001, false, &[0.5], 100, 4, 1000).is_err());
        assert!(validate_run(10, false, &[2.0], 100, 4, 1000).is_err());
        assert!(validate_run(10, false, &[0.5], 0, 4, 1000).is_err());
        assert!(validate_run(10, false, &[0.5], 100, 17, 1000).is_err());
        let many = vec![0.5; 22];
        assert!(validate_run(10, false, &many, 100, 4, 1000).is_err());
    }

    #[test]
    fn converging_runs_clamp_to_the_ceiling() {
        let capped = validate_run(700, true, &[0.5], 100, 4, 800).unwrap();
        assert_eq!((capped.iterations, capped.ceiling), (700, Some(800)));
        // The count is checked against the cap before it is clamped.
        assert!(validate_run(5000, true, &[0.5], 100, 4, 800).is_err());
        let open = validate_run(20_000, true, &[0.5], 100, 4, NO_ITERATION_CAP).unwrap();
        assert_eq!((open.iterations, open.ceiling), (10_000, Some(10_000)));
        assert_eq!(open.sample(), 10_000);
    }
}
