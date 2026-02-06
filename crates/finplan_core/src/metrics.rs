//! Simulation metrics collection for profiling and debugging
//!
//! This module provides instrumentation for tracking simulation performance,
//! particularly useful for identifying infinite loops and performance issues.

use std::collections::HashMap;

use jiff::civil::Date;

use crate::model::EventId;

/// Configuration for simulation instrumentation
#[derive(Debug, Clone)]
pub struct InstrumentationConfig {
    /// Whether to collect detailed metrics
    pub collect_metrics: bool,
    /// Maximum iterations allowed per date before breaking (safety limit)
    pub max_same_date_iterations: u64,
}

impl Default for InstrumentationConfig {
    fn default() -> Self {
        Self {
            collect_metrics: true,
            max_same_date_iterations: 1000,
        }
    }
}

impl InstrumentationConfig {
    /// Create a config with metrics disabled (fastest execution)
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            collect_metrics: false,
            max_same_date_iterations: 1000,
        }
    }

    /// Create a config with a custom iteration limit
    #[must_use]
    pub fn with_limit(max_iterations: u64) -> Self {
        Self {
            collect_metrics: true,
            max_same_date_iterations: max_iterations,
        }
    }
}

/// Metrics collected during simulation execution
#[derive(Debug, Clone, Default)]
pub struct SimulationMetrics {
    /// Total number of date advances (outer loop iterations)
    pub time_steps: u64,
    /// Total inner loop iterations across all dates
    pub same_date_iterations: u64,
    /// Maximum iterations at any single date
    pub max_same_date_iterations: u64,
    /// Total events triggered during simulation
    pub total_events_triggered: u64,
    /// Per-event trigger counts
    pub events_by_id: HashMap<EventId, u64>,
    /// Dates where the iteration limit was hit (potential infinite loops)
    pub iteration_limit_dates: Vec<Date>,
}

impl SimulationMetrics {
    /// Create empty metrics
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an inner loop iteration at the current date
    pub fn record_iteration(&mut self, date: Date, iteration: u64) {
        self.same_date_iterations += 1;
        if iteration > self.max_same_date_iterations {
            self.max_same_date_iterations = iteration;
        }
        // Dates where limit was hit are recorded separately
        let _ = date; // Used for context in iteration_limit_dates
    }

    /// Record that an event was triggered
    pub fn record_event_triggered(&mut self, event_id: EventId) {
        self.total_events_triggered += 1;
        *self.events_by_id.entry(event_id).or_insert(0) += 1;
    }

    /// Record a time step (date advance)
    pub fn record_time_step(&mut self) {
        self.time_steps += 1;
    }

    /// Record that the iteration limit was hit at a date
    pub fn record_limit_hit(&mut self, date: Date) {
        self.iteration_limit_dates.push(date);
    }

    /// Check if any iteration limits were hit
    #[must_use]
    pub fn had_iteration_limit_hits(&self) -> bool {
        !self.iteration_limit_dates.is_empty()
    }

    /// Get average iterations per time step
    #[must_use]
    pub fn avg_iterations_per_step(&self) -> f64 {
        if self.time_steps == 0 {
            0.0
        } else {
            self.same_date_iterations as f64 / self.time_steps as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_basic() {
        let mut metrics = SimulationMetrics::new();

        let date = jiff::civil::date(2025, 1, 1);
        metrics.record_time_step();
        metrics.record_iteration(date, 1);
        metrics.record_iteration(date, 2);
        metrics.record_event_triggered(EventId(1));
        metrics.record_event_triggered(EventId(1));
        metrics.record_event_triggered(EventId(2));

        assert_eq!(metrics.time_steps, 1);
        assert_eq!(metrics.same_date_iterations, 2);
        assert_eq!(metrics.max_same_date_iterations, 2);
        assert_eq!(metrics.total_events_triggered, 3);
        assert_eq!(metrics.events_by_id.get(&EventId(1)), Some(&2));
        assert_eq!(metrics.events_by_id.get(&EventId(2)), Some(&1));
        assert!(!metrics.had_iteration_limit_hits());
    }

    #[test]
    fn test_metrics_limit_recording() {
        let mut metrics = SimulationMetrics::new();
        let date = jiff::civil::date(2025, 6, 15);

        metrics.record_limit_hit(date);

        assert!(metrics.had_iteration_limit_hits());
        assert_eq!(metrics.iteration_limit_dates.len(), 1);
        assert_eq!(metrics.iteration_limit_dates[0], date);
    }

    #[test]
    fn test_instrumentation_config_defaults() {
        let config = InstrumentationConfig::default();
        assert!(config.collect_metrics);
        assert_eq!(config.max_same_date_iterations, 1000);

        let disabled = InstrumentationConfig::disabled();
        assert!(!disabled.collect_metrics);

        let custom = InstrumentationConfig::with_limit(500);
        assert!(custom.collect_metrics);
        assert_eq!(custom.max_same_date_iterations, 500);
    }
}
