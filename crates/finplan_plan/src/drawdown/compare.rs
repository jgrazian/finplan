//! The same question over many markets: a small Monte Carlo per strategy.

use finplan_core::analysis::McRunner;
use finplan_core::model::MonteCarloConfig;
use finplan_core::simulation::simulate;

use super::normalize::normalize;
use super::retirement::resolve;
use super::{CompareRequest, ComparisonRow, DrawdownComparison};
use crate::analysis::run::AnalysisError;
use crate::analysis::{ANALYSIS_SEED, iterations_or_default};
use crate::compile;
use crate::graph::ScenarioGraph;

/// Iterations per strategy when the request names none.
pub const DEFAULT_COMPARE_ITERATIONS: usize = 200;
/// The most a request may ask for per strategy, so every strategy together
/// stays inside one request (server) or one job (browser).
pub const MAX_COMPARE_ITERATIONS: usize = 500;

/// The batch plan is part of the answer; this is `CreateRun`'s default and what
/// the other analyses use, so a server and a browser agree.
const PARALLEL_BATCHES: usize = 4;

/// Run a Monte Carlo of `graph` per choice in `body.request`, all on
/// [`ANALYSIS_SEED`] (common random numbers: differences come from the
/// strategy, not from sampling noise).
///
/// Per strategy it reports success rates and the median final net worth, plus
/// the tax and real value on the median path (one more simulation each: the
/// Monte Carlo keeps no per-iteration tax).
pub fn compare(
    graph: &ScenarioGraph,
    body: &CompareRequest,
    runner: &mut dyn McRunner,
) -> Result<DrawdownComparison, AnalysisError> {
    let request = body.request.clone().unwrap_or_default();
    let choices = request.choices()?;
    let iterations = iterations_or_default(
        body.iterations,
        DEFAULT_COMPARE_ITERATIONS,
        MAX_COMPARE_ITERATIONS,
    )?;
    let compiled = compile::compile(graph)?;

    let mut base = None;
    let retirement = resolve(&compiled, request.retirement_year, ANALYSIS_SEED, &mut base)?;
    drop(base);

    let mc = MonteCarloConfig {
        iterations,
        percentiles: vec![0.5],
        compute_mean: false,
        parallel_batches: PARALLEL_BATCHES,
        seed: Some(ANALYSIS_SEED),
        ..Default::default()
    };
    runner.begin((iterations + 1) * choices.len());

    let mut rows = Vec::with_capacity(choices.len());
    for choice in choices {
        if runner.cancelled() {
            return Err(AnalysisError::Cancelled);
        }
        let (mut config, overlay) = normalize(&compiled.config, &choice, retirement.date)?;
        config.collect_ledger = false;
        let (stats, seeds) = runner.stats(&config, &mc)?;
        let median = stats
            .percentile_values
            .iter()
            .find(|(p, _)| (*p - 0.5).abs() < 1e-9)
            .map_or(stats.mean_final_net_worth, |(_, v)| *v);
        let path = seeds
            .iter()
            .find(|(p, _)| (*p - 0.5).abs() < 1e-9)
            .and_then(|(_, seed)| simulate(&config, *seed).ok());
        rows.push(ComparisonRow {
            choice,
            overlay,
            success_rate: stats.success_rate,
            funding_success_rate: stats.funding_success_rate,
            median_final_net_worth: median,
            median_final_net_worth_real: path
                .as_ref()
                .and_then(|p| p.cumulative_inflation.last())
                .map(|factor| median / factor),
            median_path_tax: path.as_ref().map(|p| {
                p.yearly_taxes
                    .iter()
                    .map(|t| t.total_tax + t.early_withdrawal_penalties)
                    .sum()
            }),
        });
    }

    Ok(DrawdownComparison {
        iterations: iterations as i64,
        retirement: retirement.info(),
        rows,
    })
}
