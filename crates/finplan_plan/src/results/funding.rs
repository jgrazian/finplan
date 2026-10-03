//! Why iterations failed the funding check, as the API reports it.
//!
//! The engine folds every path's funding facts into
//! `finplan_core::model::FundingDiagnostics`, keyed by its dense simulation
//! ids. [`funding_view`] translates that into database ids once, and the
//! result is what both a stored run (`run_stats.funding_diagnostics`) and an
//! unpersisted simulation serve — so the mapping lives in one pure function.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::compile::idmap::IdMap;

/// How the iterations that failed the funding check failed.
///
/// Counts are iterations, not warnings: a path short in twelve months counts
/// once. Year histograms are sorted by year; account and event histograms by
/// count, most frequent first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FundingDiagnostics {
    pub iterations: i64,
    /// Failed the funding check: any cash shortfall or event warning.
    pub failed: i64,
    /// Had at least one settled cash shortfall.
    pub cash_shortfall: i64,
    /// Had an effect skipped or an evaluation fail.
    pub event_failure: i64,
    /// Hit the same-date iteration limit — almost always a trigger loop.
    pub iteration_limit: i64,
    /// Failed, yet finished with positive net worth: a liquidity or funding
    /// rule problem rather than too little wealth.
    pub failed_solvent: i64,
    /// Year of each shortfall path's first deficit.
    pub first_shortfall_years: Vec<YearCount>,
    /// Median of `first_shortfall_years`; null when no path ran short.
    pub median_first_shortfall_year: Option<i64>,
    /// Account most overdrawn at each shortfall path's first deficit.
    pub shortfall_accounts: Vec<AccountCount>,
    /// Iterations in which each event had an effect skipped or fail to evaluate.
    pub event_failures: Vec<EventCount>,
    /// Year liquid (bank and investment) balances first ran out, over paths
    /// where they did.
    pub liquid_depleted_years: Vec<YearCount>,
    /// Median over shortfall paths of each path's largest deficit, nominal.
    pub median_max_deficit: Option<f64>,
    /// Median over shortfall paths of the calendar years spent short.
    pub median_shortfall_years: Option<f64>,
    /// Seed of the failing iteration that failed earliest (larger deficit
    /// breaks ties). Re-simulating it against the run's inputs reproduces the
    /// worst path. A decimal string: seeds span the full `u64` range, which a
    /// JSON number would round.
    pub worst_seed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct YearCount {
    pub year: i64,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AccountCount {
    /// Null for an account created mid-simulation, which has no row.
    pub account_id: Option<i64>,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventCount {
    /// Null for an event with no row.
    pub event_id: Option<i64>,
    pub count: i64,
}

fn years(histogram: &[(i16, usize)]) -> Vec<YearCount> {
    histogram
        .iter()
        .map(|&(year, count)| YearCount {
            year: i64::from(year),
            count: count as i64,
        })
        .collect()
}

/// Re-key a count-sorted histogram by database id. Engine ids without a row
/// collapse into one `None` entry; the result is re-sorted by count, most
/// frequent first, ties by id.
fn by_db_id<K: Copy>(
    histogram: &[(K, usize)],
    db_id: impl Fn(K) -> Option<i64>,
) -> Vec<(Option<i64>, i64)> {
    let mut counts: BTreeMap<Option<i64>, i64> = BTreeMap::new();
    for &(key, count) in histogram {
        *counts.entry(db_id(key)).or_default() += count as i64;
    }
    let mut out: Vec<_> = counts.into_iter().collect();
    out.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    out
}

/// Translate the engine's funding diagnostics into the API's, attributing
/// accounts and events to their database rows through the compile's `IdMap`.
///
/// Pure: a persisted run and an in-memory preview produce the same view.
pub fn funding_view(
    id_map: &IdMap,
    diagnostics: &finplan_core::model::FundingDiagnostics,
) -> FundingDiagnostics {
    FundingDiagnostics {
        iterations: diagnostics.iterations as i64,
        failed: diagnostics.failed as i64,
        cash_shortfall: diagnostics.cash_shortfall as i64,
        event_failure: diagnostics.event_failure as i64,
        iteration_limit: diagnostics.iteration_limit as i64,
        failed_solvent: diagnostics.failed_solvent as i64,
        first_shortfall_years: years(&diagnostics.first_shortfall_years),
        median_first_shortfall_year: diagnostics.median_first_shortfall_year().map(i64::from),
        shortfall_accounts: by_db_id(&diagnostics.shortfall_accounts, |id| {
            id_map.account_db_id(id)
        })
        .into_iter()
        .map(|(account_id, count)| AccountCount { account_id, count })
        .collect(),
        event_failures: by_db_id(&diagnostics.event_failures, |id| id_map.event_db_id(id))
            .into_iter()
            .map(|(event_id, count)| EventCount { event_id, count })
            .collect(),
        liquid_depleted_years: years(&diagnostics.liquid_depleted_years),
        median_max_deficit: diagnostics.median_max_deficit,
        median_shortfall_years: diagnostics.median_shortfall_years,
        worst_seed: diagnostics.worst_seed.map(|seed| seed.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use finplan_core::model::{AccountId, EventId};

    #[test]
    fn maps_engine_ids_to_rows_and_collapses_unmapped() {
        let mut ids = IdMap::new();
        let usaa = ids.intern_account(60).unwrap();
        let brokerage = ids.intern_account(10).unwrap();
        let sweep = ids.intern_event(7).unwrap();

        let engine = finplan_core::model::FundingDiagnostics {
            iterations: 100,
            failed: 9,
            cash_shortfall: 8,
            event_failure: 2,
            iteration_limit: 0,
            failed_solvent: 3,
            first_shortfall_years: vec![(2041, 3), (2042, 5)],
            shortfall_accounts: vec![
                (usaa, 5),
                (brokerage, 1),
                (AccountId(40), 1),
                (AccountId(41), 1),
            ],
            event_failures: vec![(sweep, 2), (EventId(90), 1)],
            liquid_depleted_years: vec![(2070, 4)],
            median_max_deficit: Some(38_200.0),
            median_shortfall_years: Some(2.0),
            worst_seed: Some(u64::MAX),
        };

        let view = funding_view(&ids, &engine);
        assert_eq!(view.failed, 9);
        assert_eq!(view.median_first_shortfall_year, Some(2042));
        assert_eq!(
            view.shortfall_accounts,
            vec![
                AccountCount {
                    account_id: Some(60),
                    count: 5
                },
                // Two engine-only accounts fold into one row-less entry.
                AccountCount {
                    account_id: None,
                    count: 2
                },
                AccountCount {
                    account_id: Some(10),
                    count: 1
                },
            ]
        );
        assert_eq!(view.event_failures[0].event_id, Some(7));
        assert_eq!(view.event_failures[1].event_id, None);
        assert_eq!(view.worst_seed.as_deref(), Some("18446744073709551615"));
    }
}
