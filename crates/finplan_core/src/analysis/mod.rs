//! Parameter sweep sensitivity analysis module.
//!
//! This module provides functionality for running N-dimensional parameter sweeps
//! to analyze how changes to simulation parameters affect outcomes.
//!
//! # Two-Phase Analysis
//!
//! The module supports a two-phase approach where simulations can be run up-front
//! and metrics computed afterward:
//!
//! ```ignore
//! use finplan_core::analysis::{SweepConfig, SweepParameter, AnalysisMetric, sweep_simulate};
//!
//! // Phase 1: Run simulations (expensive, done once)
//! let sim_results = sweep_simulate(&sim_config, &sweep_config, Some(&progress))?;
//!
//! // Phase 2: Compute metrics (fast, can be done multiple times)
//! let results = sim_results.compute_all_metrics(&[AnalysisMetric::SuccessRate]);
//!
//! // Compute different metrics without re-running simulations
//! let other_results = sim_results.compute_all_metrics(&[
//!     AnalysisMetric::NetWorthAtAge { age: 75 },
//!     AnalysisMetric::MaxDrawdown,
//! ]);
//! ```
//!
//! # Combined Mode
//!
//! For simpler use cases, use `sweep_evaluate` to run simulations and compute
//! metrics in one pass:
//!
//! ```ignore
//! use finplan_core::analysis::{SweepConfig, SweepParameter, SweepTarget, AnalysisMetric};
//!
//! let config = SweepConfig {
//!     parameters: vec![
//!         SweepParameter {
//!             event_id: EventId(1),
//!             target: SweepTarget::Trigger(TriggerParam::Age),
//!             min_value: 60.0,
//!             max_value: 70.0,
//!             step_count: 6,
//!         },
//!     ],
//!     metrics: vec![AnalysisMetric::SuccessRate, AnalysisMetric::NetWorthAtAge { age: 75 }],
//!     mc_iterations: 500,
//!     ..Default::default()
//! };
//!
//! let results = sweep_evaluate(&sim_config, &config, Some(&progress))?;
//! ```
//!
//! # N-Dimensional Grid
//!
//! Use `SweepGrid<T>` to store and access N-dimensional data with stride-based
//! indexing. The grid supports slicing operations for extracting 1D/2D views
//! from higher-dimensional data.

mod config;
mod evaluator;
mod metrics;

pub use config::*;
pub use evaluator::*;
pub use metrics::*;
