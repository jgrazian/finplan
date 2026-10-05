//! Exact annual vectors: bounded by dates × iterations, not ledger size.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::SimulationError;
use crate::model::{RealNetWorthSummary, RealQuantilePoint, RealTerminalStats, SimulationResult};

/// Per-batch real net worth columns, merged across batches before quantiles.
///
/// Serializable so a batch run elsewhere can hand it back to be merged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealAccumulator {
    dates: Vec<jiff::civil::Date>,
    columns: Vec<Vec<f64>>,
    /// The seed of each iteration, aligned to every column's rows.
    #[serde(default)]
    seeds: Vec<u64>,
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
            seeds: Vec::new(),
        }
    }

    pub(super) fn accumulate(
        &mut self,
        seed: u64,
        result: &SimulationResult,
    ) -> Result<(), SimulationError> {
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
        self.seeds.push(seed);
        Ok(())
    }

    pub(super) fn merge(&mut self, mut other: Self) {
        debug_assert_eq!(self.dates, other.dates);
        for (column, mut values) in self.columns.iter_mut().zip(other.columns) {
            column.append(&mut values);
        }
        self.seeds.append(&mut other.seeds);
    }

    /// For each percentile `p`, the seed of the path that tracks `p`'s band.
    ///
    /// A path's rank among all paths at a date, scaled to `[0, 1]` the way
    /// the bands' quantiles are (`(N - 1) * p`, ties sharing their mean rank),
    /// says which band it sits on there. The chosen path minimizes the squared
    /// distance between that rank and `p`, summed over every date of the grid.
    /// Ranks rather than dollars: wealth grows over the horizon and depleted
    /// paths sit at zero, so dollar distances would let the last years decide.
    ///
    /// Equal scores go to the earlier iteration, so the choice depends on the
    /// merge order alone. Empty when nothing was accumulated.
    pub(super) fn representative_seeds(&self, percentiles: &[f64]) -> Vec<(f64, u64)> {
        let n = self.seeds.len();
        if n == 0 {
            return Vec::new();
        }
        let scale = (n - 1).max(1) as f64;
        let mut scores = vec![vec![0.0_f64; n]; percentiles.len()];
        let mut order: Vec<usize> = (0..n).collect();
        for column in &self.columns {
            order.sort_unstable_by(|&a, &b| column[a].total_cmp(&column[b]));
            let mut start = 0;
            while start < n {
                let mut end = start + 1;
                while end < n && column[order[end]] == column[order[start]] {
                    end += 1;
                }
                let rank = (start + end - 1) as f64 / 2.0 / scale;
                for &path in &order[start..end] {
                    for (score, &p) in scores.iter_mut().zip(percentiles) {
                        score[path] += (rank - p).powi(2);
                    }
                }
                start = end;
            }
        }
        percentiles
            .iter()
            .zip(&scores)
            .map(|(&p, score)| {
                let best = (1..n).fold(0, |best, i| if score[i] < score[best] { i } else { best });
                (p, self.seeds[best])
            })
            .collect()
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
                    p10: quantile(&values, 0.10),
                    p25: quantile(&values, 0.25),
                    p50: quantile(&values, 0.5),
                    p75: quantile(&values, 0.75),
                    p90: quantile(&values, 0.90),
                    p95: quantile(&values, 0.95),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
#[path = "../tests/results_quantiles.rs"]
mod results_quantiles;

/// Linear interpolation between the two order statistics around `p`.
///
/// Written as `lo + (hi - lo) * f` and clamped to `[lo, hi]`: the two-product
/// form `lo * (1 - f) + hi * f` rounds an ulp outside the bracket even when
/// `lo == hi`, which breaks the monotonicity of quantiles (`p5 <= p50 <= p95`)
/// that a fully deterministic plan should satisfy exactly.
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let h = (sorted.len() - 1) as f64 * p;
    let (lo, hi) = (sorted[h.floor() as usize], sorted[h.ceil() as usize]);
    if lo == hi {
        return lo;
    }
    (lo + (hi - lo) * h.fract()).clamp(lo, hi)
}
