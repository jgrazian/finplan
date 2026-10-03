//! Pure helpers that shape a run's results, shared by the server's SQL read
//! path and [`super::RunResults`]: which stored path a `series` names, a path's
//! identity, deflating by its inflation, and the ledger's paging limits.

use crate::error::{PlanError, PlanResult};

/// The four buckets the ledger filter offers.
pub const LEDGER_CATEGORIES: [&str; 4] = ["cash", "asset", "tax", "event"];

/// The most entries one request will return. A year of a busy plan runs to a
/// few hundred; the cap is what stops `year` being omitted by accident from
/// serialising the whole run.
pub const LEDGER_PAGE_MAX: i64 = 500;

/// A path's run-local identity: its percentile, or `mean` for the synthetic
/// average.
#[must_use]
pub fn path_id(percentile: Option<f64>) -> String {
    percentile.map_or_else(|| "mean".into(), |p| p.to_string())
}

/// Which stored path a `series` query names: the mean, an explicit percentile,
/// or — by default — whichever stored percentile sits closest to the median.
///
/// An explicit percentile resolves to the nearest stored path rather than to
/// itself. A run keeps the percentiles it was started with, and a caller asking
/// for `0.05` is naming the run that stands for the bad case, not asserting
/// that a path was stored at exactly that mark — so a run stored at 0.1 answers
/// with the path it has instead of with an empty series.
pub fn resolve_series(series: Option<&str>, stored: &[Option<f64>]) -> PlanResult<Option<f64>> {
    Ok(match series {
        Some("mean") if stored.contains(&None) => None,
        Some("mean") => return Err(PlanError::NotFound("stored mean series")),
        Some(other) => {
            let target = other.parse::<f64>().map_err(|_| {
                PlanError::invalid("series must be 'mean' or a percentile such as 0.5")
            })?;
            if !(0.0..=1.0).contains(&target) {
                return Err(PlanError::invalid("series percentile must be in 0..1"));
            }
            Some(nearest_stored(stored, target).ok_or(PlanError::NotFound("representative path"))?)
        }
        None => {
            Some(nearest_stored(stored, 0.5).ok_or(PlanError::NotFound("representative path"))?)
        }
    })
}

/// The stored percentile closest to `target`, if the run stored any at all.
#[must_use]
pub fn nearest_stored(stored: &[Option<f64>], target: f64) -> Option<f64> {
    stored.iter().flatten().copied().min_by(|a, b| {
        (a - target)
            .abs()
            .partial_cmp(&(b - target).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// The calendar year at the front of an ISO date, 0 when it is not one.
#[must_use]
pub fn year_of(date: &str) -> i64 {
    date.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0)
}

/// The factor for one year: the plan's first year before the table starts, the
/// last recorded factor beyond its end, and 1.0 for a run that stored none.
/// `factors` is ascending by year.
#[must_use]
pub fn factor_for(factors: &[(i64, f64)], year: i64) -> f64 {
    if factors.is_empty() {
        return 1.0;
    }
    match factors.binary_search_by_key(&year, |(y, _)| *y) {
        Ok(i) => factors[i].1,
        Err(0) => 1.0,
        Err(i) => factors[i - 1].1,
    }
}

/// The check on a ledger `category` filter, with the message the API gives.
pub fn check_category(category: Option<&str>) -> PlanResult<()> {
    match category {
        Some(category) if !LEDGER_CATEGORIES.contains(&category) => Err(PlanError::invalid(
            "category must be one of cash, asset, tax, event",
        )),
        _ => Ok(()),
    }
}
