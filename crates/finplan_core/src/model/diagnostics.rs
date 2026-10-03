//! Why iterations fail the funding check.
//!
//! `MonteCarloStats::funding_success_rate` says how many iterations failed;
//! this says when, where and how badly. Each path records a few facts about
//! its own funding as it runs (`PathDiagnostics`), and Monte Carlo folds them
//! into one summary (`FundingDiagnostics`) with a mergeable accumulator, the
//! same way the mean snapshots are built — so nothing here needs the ledger.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ids::{AccountId, EventId};
use super::results::{SimulationResult, WarningKind};
use super::{AccountSnapshot, AccountSnapshotFlavor};

/// The first settled cash deficit on a path.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShortfallStart {
    pub date: jiff::civil::Date,
    /// The most overdrawn cash account at that checkpoint.
    pub account_id: AccountId,
    /// Nominal dollars below zero, positive.
    pub deficit: f64,
}

/// Funding facts one path records about itself while it runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PathDiagnostics {
    pub first_shortfall: Option<ShortfallStart>,
    /// Largest settled cash deficit on the path, nominal and positive; zero
    /// when cash never went short.
    pub max_deficit: f64,
    /// Distinct calendar years in which a checkpoint settled with a deficit.
    /// Separates a one-off timing blip from a plan that is short every year.
    pub shortfall_years: u16,
    /// First wealth snapshot at which bank and investment balances together
    /// were no longer positive — out of money to spend, whatever property or
    /// debt the path also holds.
    pub liquid_depleted: Option<jiff::civil::Date>,
}

impl PathDiagnostics {
    /// Fold one settled checkpoint's most overdrawn cash account into the path.
    pub(crate) fn observe_cash(
        &mut self,
        date: jiff::civil::Date,
        year_of: jiff::civil::Date,
        lowest: Option<(AccountId, f64)>,
        last_year: &mut Option<i16>,
    ) {
        let Some((account_id, balance)) = lowest else {
            return;
        };
        if balance >= -0.005 {
            return;
        }
        let deficit = -balance;
        if self.first_shortfall.is_none() {
            self.first_shortfall = Some(ShortfallStart {
                date,
                account_id,
                deficit,
            });
        }
        self.max_deficit = self.max_deficit.max(deficit);
        if *last_year != Some(year_of.year()) {
            *last_year = Some(year_of.year());
            self.shortfall_years += 1;
        }
    }

    /// Read liquid depletion off the path's wealth snapshots.
    pub(crate) fn observe_snapshots(&mut self, result: &SimulationResult) {
        let liquid = |accounts: &[AccountSnapshot]| -> f64 {
            accounts
                .iter()
                .filter(|a| {
                    matches!(
                        a.flavor,
                        AccountSnapshotFlavor::Bank(_) | AccountSnapshotFlavor::Investment { .. }
                    )
                })
                .map(AccountSnapshot::total_value)
                .sum()
        };
        // A plan that starts with nothing liquid has not run out of anything.
        let started_liquid = result
            .wealth_snapshots
            .first()
            .is_some_and(|s| liquid(&s.accounts) > 0.005);
        if !started_liquid {
            return;
        }
        self.liquid_depleted = result
            .wealth_snapshots
            .iter()
            .find(|s| liquid(&s.accounts) <= 0.005)
            .map(|s| s.date);
    }
}

/// How the iterations that failed the funding check failed.
///
/// Counts are iterations, not warnings: a path short in twelve months counts
/// once. Histograms are `(key, iterations)` pairs sorted by key, or by count
/// descending for accounts and events.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FundingDiagnostics {
    pub iterations: usize,
    /// Failed the funding check: any warning at all.
    pub failed: usize,
    /// Had at least one settled cash shortfall.
    pub cash_shortfall: usize,
    /// Had an effect skipped or an evaluation fail.
    pub event_failure: usize,
    /// Hit the same-date iteration limit — almost always a trigger loop.
    pub iteration_limit: usize,
    /// Failed, yet finished with positive net worth: a liquidity or funding
    /// rule problem rather than too little wealth.
    pub failed_solvent: usize,
    /// Year of each shortfall path's first deficit.
    pub first_shortfall_years: Vec<(i16, usize)>,
    /// Account most overdrawn at each shortfall path's first deficit.
    pub shortfall_accounts: Vec<(AccountId, usize)>,
    /// Iterations in which each event had an effect skipped or fail to evaluate.
    pub event_failures: Vec<(EventId, usize)>,
    /// Year liquid balances first ran out, over paths where they did.
    pub liquid_depleted_years: Vec<(i16, usize)>,
    /// Median over shortfall paths of each path's largest deficit, nominal.
    pub median_max_deficit: Option<f64>,
    /// Median over shortfall paths of the years spent short.
    pub median_shortfall_years: Option<f64>,
    /// The failing iteration that failed earliest, the larger deficit breaking
    /// ties: re-simulate this seed to show the worst path.
    pub worst_seed: Option<u64>,
}

impl FundingDiagnostics {
    /// Median year of first shortfall, read off the histogram.
    #[must_use]
    pub fn median_first_shortfall_year(&self) -> Option<i16> {
        median_key(&self.first_shortfall_years)
    }
}

fn median_key(histogram: &[(i16, usize)]) -> Option<i16> {
    let total: usize = histogram.iter().map(|(_, n)| n).sum();
    if total == 0 {
        return None;
    }
    let middle = total.div_ceil(2);
    let mut seen = 0;
    histogram.iter().find_map(|&(key, n)| {
        seen += n;
        (seen >= middle).then_some(key)
    })
}

/// Most frequent first; the stable sort keeps key order among equal counts.
fn by_count<K>(map: BTreeMap<K, usize>) -> Vec<(K, usize)> {
    let mut v: Vec<_> = map.into_iter().collect();
    v.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    v
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let n = values.len();
    Some(if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    })
}

/// Mergeable per-batch state behind `FundingDiagnostics`.
///
/// Serializable because a batch can run in another process (a WebAssembly
/// worker) and hand this back to be merged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FundingAccumulator {
    iterations: usize,
    failed: usize,
    cash_shortfall: usize,
    event_failure: usize,
    iteration_limit: usize,
    failed_solvent: usize,
    first_shortfall_years: BTreeMap<i16, usize>,
    shortfall_accounts: BTreeMap<AccountId, usize>,
    event_failures: BTreeMap<EventId, usize>,
    liquid_depleted_years: BTreeMap<i16, usize>,
    max_deficits: Vec<f64>,
    shortfall_years: Vec<f64>,
    /// (first failure date, -deficit, seed): the minimum is the worst path.
    worst: Option<(jiff::civil::Date, f64, u64)>,
}

impl FundingAccumulator {
    pub fn add(&mut self, seed: u64, result: &SimulationResult, final_net_worth: f64) {
        self.iterations += 1;
        let d = &result.diagnostics;
        if let Some(date) = d.liquid_depleted {
            *self.liquid_depleted_years.entry(date.year()).or_default() += 1;
        }
        if result.warnings.is_empty() {
            return;
        }

        self.failed += 1;
        self.failed_solvent += usize::from(final_net_worth > 0.0);

        let mut events: Vec<EventId> = Vec::new();
        let (mut processing, mut limit) = (false, false);
        for w in &result.warnings {
            match w.kind {
                WarningKind::EffectSkipped | WarningKind::EvaluationFailed => {
                    processing = true;
                    if let Some(id) = w.event_id
                        && !events.contains(&id)
                    {
                        events.push(id);
                    }
                }
                WarningKind::IterationLimitHit => limit = true,
                WarningKind::CashShortfall => {}
            }
        }
        self.event_failure += usize::from(processing);
        self.iteration_limit += usize::from(limit);
        for id in events {
            *self.event_failures.entry(id).or_default() += 1;
        }

        if let Some(start) = d.first_shortfall {
            self.cash_shortfall += 1;
            *self
                .first_shortfall_years
                .entry(start.date.year())
                .or_default() += 1;
            *self.shortfall_accounts.entry(start.account_id).or_default() += 1;
            self.max_deficits.push(d.max_deficit);
            self.shortfall_years.push(f64::from(d.shortfall_years));
        }

        if let Some(first) = result.warnings.iter().map(|w| w.date).min() {
            self.consider_worst((first, -d.max_deficit, seed));
        }
    }

    fn consider_worst(&mut self, candidate: (jiff::civil::Date, f64, u64)) {
        let worse = self.worst.is_none_or(|current| {
            (candidate.0, candidate.1, candidate.2)
                .partial_cmp(&current)
                .is_some_and(std::cmp::Ordering::is_lt)
        });
        if worse {
            self.worst = Some(candidate);
        }
    }

    pub fn merge(&mut self, other: Self) {
        self.iterations += other.iterations;
        self.failed += other.failed;
        self.cash_shortfall += other.cash_shortfall;
        self.event_failure += other.event_failure;
        self.iteration_limit += other.iteration_limit;
        self.failed_solvent += other.failed_solvent;
        for (k, n) in other.first_shortfall_years {
            *self.first_shortfall_years.entry(k).or_default() += n;
        }
        for (k, n) in other.shortfall_accounts {
            *self.shortfall_accounts.entry(k).or_default() += n;
        }
        for (k, n) in other.event_failures {
            *self.event_failures.entry(k).or_default() += n;
        }
        for (k, n) in other.liquid_depleted_years {
            *self.liquid_depleted_years.entry(k).or_default() += n;
        }
        self.max_deficits.extend(other.max_deficits);
        self.shortfall_years.extend(other.shortfall_years);
        if let Some(w) = other.worst {
            self.consider_worst(w);
        }
    }

    #[must_use]
    pub fn finish(mut self) -> FundingDiagnostics {
        FundingDiagnostics {
            iterations: self.iterations,
            failed: self.failed,
            cash_shortfall: self.cash_shortfall,
            event_failure: self.event_failure,
            iteration_limit: self.iteration_limit,
            failed_solvent: self.failed_solvent,
            first_shortfall_years: self.first_shortfall_years.into_iter().collect(),
            shortfall_accounts: by_count(self.shortfall_accounts),
            event_failures: by_count(self.event_failures),
            liquid_depleted_years: self.liquid_depleted_years.into_iter().collect(),
            median_max_deficit: median(&mut self.max_deficits),
            median_shortfall_years: median(&mut self.shortfall_years),
            worst_seed: self.worst.map(|(_, _, seed)| seed),
        }
    }
}
