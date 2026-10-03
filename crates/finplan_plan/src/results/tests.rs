//! Unit tests for the projection. That it equals what the server's SQL read
//! path serves is tested in the server (`tests/cases/results_projection.rs`),
//! which has the database to compare against.

use finplan_core::model::MonteCarloConfig;
use finplan_core::simulation::monte_carlo_simulate_with_config;

use super::shape::{check_category, factor_for, nearest_stored, resolve_series, year_of};
use super::{RunResults, RunSettings, project};
use crate::compile::compile;
use crate::error::PlanError;
use crate::graph::ScenarioGraph;

fn projected(settings: &RunSettings) -> RunResults {
    let graph: ScenarioGraph =
        serde_json::from_str(include_str!("../../testdata/default_snapshot.json")).unwrap();
    let compiled = compile(&graph).unwrap();
    let config = MonteCarloConfig {
        iterations: 20,
        percentiles: vec![0.1, 0.5, 0.9],
        compute_mean: true,
        seed: Some(3),
        ..MonteCarloConfig::default()
    };
    let summary = monte_carlo_simulate_with_config(&compiled.config, &config).unwrap();
    project(&compiled, &summary, settings)
}

#[test]
fn a_series_resolves_to_the_nearest_stored_path() {
    let stored = [None, Some(0.1), Some(0.5), Some(0.9)];
    assert_eq!(resolve_series(None, &stored), Ok(Some(0.5)));
    assert_eq!(resolve_series(Some("mean"), &stored), Ok(None));
    assert_eq!(resolve_series(Some("0.05"), &stored), Ok(Some(0.1)));
    assert_eq!(resolve_series(Some("0.8"), &stored), Ok(Some(0.9)));
    assert_eq!(
        resolve_series(Some("mean"), &[Some(0.5)]),
        Err(PlanError::NotFound("stored mean series"))
    );
    assert_eq!(
        resolve_series(None, &[]),
        Err(PlanError::NotFound("representative path"))
    );
    assert_eq!(
        resolve_series(Some("1.5"), &stored),
        Err(PlanError::invalid("series percentile must be in 0..1"))
    );
    assert_eq!(
        resolve_series(Some("p50"), &stored),
        Err(PlanError::invalid(
            "series must be 'mean' or a percentile such as 0.5"
        ))
    );
    assert_eq!(nearest_stored(&[None], 0.5), None);
}

#[test]
fn inflation_factors_clamp_at_both_ends() {
    let factors = [(2026, 1.0), (2027, 1.03), (2028, 1.06)];
    assert_eq!(factor_for(&factors, 2025), 1.0);
    assert_eq!(factor_for(&factors, 2027), 1.03);
    assert_eq!(factor_for(&factors, 2040), 1.06);
    assert_eq!(factor_for(&[], 2030), 1.0);
    assert_eq!(year_of("2031-12-31"), 2031);
    assert_eq!(year_of("x"), 0);
}

#[test]
fn only_the_four_ledger_buckets_are_filters() {
    assert!(check_category(None).is_ok());
    assert!(check_category(Some("tax")).is_ok());
    assert!(check_category(Some("misc")).is_err());
}

#[test]
fn a_projection_serves_every_path_it_stored() {
    let results = projected(&RunSettings::default());
    let served = results.results(1, 2, None).unwrap();

    // The mean first, then the percentiles ascending.
    let ids: Vec<&str> = served.bands.iter().map(|b| b.path_id.as_str()).collect();
    assert_eq!(ids, ["mean", "0.1", "0.5", "0.9"]);
    assert!(served.path_details);
    assert_eq!(served.series_id, "0.5");
    assert_eq!((served.run_id, served.scenario_id), (1, 2));
    assert_eq!(served.stats.num_iterations, 20);
    assert!(served.real_net_worth.is_some());
    assert!(!served.account_series.is_empty());
    assert!(!served.cash_flows.is_empty());

    let mean = results.results(1, 2, Some("mean")).unwrap();
    assert_eq!(mean.series_percentile, None);
    // The mean replays no sequence of events, so it keeps no ledger.
    assert!(mean.ledger_years.is_empty());
    assert!(!served.ledger_years.is_empty());
}

#[test]
fn a_ledger_page_is_filtered_then_paged() {
    let results = projected(&RunSettings::default());
    let all = results
        .ledger_page(1, None, None, None, Some(500), None)
        .unwrap();
    assert!(all.total > 3);
    assert_eq!(all.series_id, "0.5");
    let positions: Vec<i64> = all.entries.iter().map(|e| e.position).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]));

    let page = results
        .ledger_page(1, None, None, None, Some(2), Some(1))
        .unwrap();
    assert_eq!(page.total, all.total);
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].position, all.entries[1].position);

    let cash = results
        .ledger_page(1, None, None, Some("cash"), None, None)
        .unwrap();
    assert!(cash.entries.iter().all(|e| e.category == "cash"));
    assert!(
        results
            .ledger_page(1, None, None, Some("misc"), None, None)
            .is_err()
    );
}

#[test]
fn a_run_can_skip_its_ledger() {
    let results = projected(&RunSettings {
        include_ledger: false,
    });
    assert!(results.paths.iter().all(|path| path.ledger.is_empty()));
    let served = results.results(1, 2, None).unwrap();
    assert!(served.ledger_years.is_empty());
    assert!(!served.cash_flows.is_empty());
}

/// Preview pairs a base and an edited run by seed, and the browser (spec 19)
/// must agree with the server, so a seed has to reproduce a run to the bit.
/// The default plan holds several assets per account, which is what used to
/// make it drift in the last bits (account snapshots summed a `HashMap`).
#[test]
fn a_seed_reproduces_the_projection_exactly() {
    let body = || {
        let results = projected(&RunSettings::default())
            .results(1, 1, None)
            .unwrap();
        serde_json::to_string(&results).unwrap()
    };
    let first = body();
    for _ in 0..3 {
        assert_eq!(first, body());
    }
}
