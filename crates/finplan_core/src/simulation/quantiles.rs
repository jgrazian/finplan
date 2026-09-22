//! Exact annual vectors: bounded by dates × iterations, not ledger size.
use std::collections::BTreeMap;

use crate::error::SimulationError;
use crate::model::{RealNetWorthSummary, RealQuantilePoint, RealTerminalStats, SimulationResult};

pub(super) struct RealAccumulator {
    dates: Vec<jiff::civil::Date>,
    columns: Vec<Vec<f64>>,
}

fn snapshots(result: &SimulationResult) -> BTreeMap<jiff::civil::Date, f64> {
    // Terminal can duplicate a year-end checkpoint. The final snapshot wins.
    result
        .wealth_snapshots
        .iter()
        .map(|s| (s.date, s.accounts.iter().map(|a| a.total_value()).sum()))
        .collect()
}

impl RealAccumulator {
    pub(super) fn new(template: &SimulationResult) -> Self {
        let dates: Vec<_> = snapshots(template).into_keys().collect();
        Self {
            columns: vec![Vec::new(); dates.len()],
            dates,
        }
    }

    pub(super) fn accumulate(&mut self, result: &SimulationResult) -> Result<(), SimulationError> {
        let points = snapshots(result);
        if points.len() != self.dates.len()
            || !points.keys().eq(self.dates.iter())
            || points.is_empty()
        {
            return Err(SimulationError::Config(
                "inconsistent real quantile date grid".into(),
            ));
        }
        for ((date, nominal), column) in points.into_iter().zip(&mut self.columns) {
            let index = (date.year() - self.dates[0].year()) as usize;
            let factor = result
                .cumulative_inflation
                .get(index)
                .copied()
                .unwrap_or(f64::NAN);
            let real = nominal / factor;
            if !nominal.is_finite() || !factor.is_finite() || factor <= 0.0 || !real.is_finite() {
                return Err(SimulationError::Config(format!(
                    "invalid real net worth or inflation at {date}"
                )));
            }
            column.push(real);
        }
        Ok(())
    }

    pub(super) fn merge(&mut self, other: Self) {
        debug_assert_eq!(self.dates, other.dates);
        for (column, mut values) in self.columns.iter_mut().zip(other.columns) {
            column.append(&mut values);
        }
    }

    pub(super) fn finish(mut self) -> Result<RealNetWorthSummary, SimulationError> {
        for column in &mut self.columns {
            column.sort_unstable_by(f64::total_cmp);
        }
        let terminal = self.columns.last().expect("validated nonempty grid");
        let n = terminal.len() as f64;
        // Divide before summing so finite large values do not overflow the sum.
        let mean: f64 = terminal.iter().map(|v| v / n).sum();
        let std_dev = terminal
            .iter()
            .fold(0.0_f64, |sd, v| sd.hypot((v - mean) / n.sqrt()));
        if !mean.is_finite() || !std_dev.is_finite() {
            return Err(SimulationError::Config(
                "nonfinite real terminal aggregates".into(),
            ));
        }
        Ok(RealNetWorthSummary {
            base_date: self.dates[0],
            num_iterations: terminal.len(),
            terminal: RealTerminalStats {
                mean,
                std_dev,
                min: terminal[0],
                max: terminal[terminal.len() - 1],
            },
            points: self
                .dates
                .into_iter()
                .zip(self.columns)
                .map(|(date, values)| RealQuantilePoint {
                    date,
                    p5: quantile(&values, 0.05),
                    p50: quantile(&values, 0.5),
                    p95: quantile(&values, 0.95),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
#[path = "../tests/results_quantiles.rs"]
mod results_quantiles;

fn quantile(sorted: &[f64], p: f64) -> f64 {
    let h = (sorted.len() - 1) as f64 * p;
    sorted[h.floor() as usize] * (1.0 - h.fract()) + sorted[h.ceil() as usize] * h.fract()
}
