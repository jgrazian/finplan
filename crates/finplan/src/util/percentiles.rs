//! Percentile extraction utilities for Monte Carlo results

/// Tolerance for floating-point percentile comparison
pub const PERCENTILE_TOLERANCE: f64 = 0.001;

/// Standard percentiles used in the application
pub mod standard {
    pub const P5: f64 = 0.05;
    pub const P50: f64 = 0.50;
    pub const P95: f64 = 0.95;
}

/// Find a percentile value from a slice of (percentile, value) pairs
///
/// Uses `PERCENTILE_TOLERANCE` for floating-point comparison.
///
/// # Example
/// ```ignore
/// let values = vec![(0.05, 100.0), (0.50, 200.0), (0.95, 300.0)];
/// assert_eq!(find_percentile_value(&values, 0.50), Some(200.0));
/// ```
#[inline]
pub fn find_percentile_value(values: &[(f64, f64)], target: f64) -> Option<f64> {
    values
        .iter()
        .find(|(p, _)| (*p - target).abs() < PERCENTILE_TOLERANCE)
        .map(|(_, v)| *v)
}

/// Find a percentile result from a slice of (percentile, result) tuples
///
/// Generic version that works with any result type.
#[inline]
pub fn find_percentile_result<T>(results: &[(f64, T)], target: f64) -> Option<&T> {
    results
        .iter()
        .find(|(p, _)| (*p - target).abs() < PERCENTILE_TOLERANCE)
        .map(|(_, result)| result)
}

/// Find a percentile result from a slice of (percentile, T1, T2) triples
///
/// Returns references to both result types.
#[inline]
pub fn find_percentile_result_pair<T1, T2>(
    results: &[(f64, T1, T2)],
    target: f64,
) -> Option<(&T1, &T2)> {
    results
        .iter()
        .find(|(p, _, _)| (*p - target).abs() < PERCENTILE_TOLERANCE)
        .map(|(_, r1, r2)| (r1, r2))
}

/// Standard percentile set extracted from Monte Carlo results
#[derive(Debug, Clone, Copy)]
pub struct PercentileSet {
    pub p5: f64,
    pub p50: f64,
    pub p95: f64,
}

impl PercentileSet {
    /// Extract standard percentiles (P5, P50, P95) from a slice of (percentile, value) pairs
    ///
    /// Returns `None` if any of the three percentiles are missing.
    pub fn from_values(values: &[(f64, f64)]) -> Option<Self> {
        Some(Self {
            p5: find_percentile_value(values, standard::P5)?,
            p50: find_percentile_value(values, standard::P50)?,
            p95: find_percentile_value(values, standard::P95)?,
        })
    }

    /// Extract standard percentiles with defaults for missing values
    ///
    /// Uses 0.0 as the default for any missing percentile.
    pub fn from_values_or_default(values: &[(f64, f64)]) -> Self {
        Self {
            p5: find_percentile_value(values, standard::P5).unwrap_or(0.0),
            p50: find_percentile_value(values, standard::P50).unwrap_or(0.0),
            p95: find_percentile_value(values, standard::P95).unwrap_or(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_percentile_value() {
        let values = vec![(0.05, 100.0), (0.50, 200.0), (0.95, 300.0)];

        assert_eq!(find_percentile_value(&values, 0.05), Some(100.0));
        assert_eq!(find_percentile_value(&values, 0.50), Some(200.0));
        assert_eq!(find_percentile_value(&values, 0.95), Some(300.0));
        assert_eq!(find_percentile_value(&values, 0.25), None);
    }

    #[test]
    fn test_find_percentile_value_with_tolerance() {
        let values = vec![(0.0500001, 100.0)];

        // Should match within tolerance
        assert_eq!(find_percentile_value(&values, 0.05), Some(100.0));
    }

    #[test]
    fn test_find_percentile_result() {
        let results = vec![(0.05, "low"), (0.50, "mid"), (0.95, "high")];

        assert_eq!(find_percentile_result(&results, 0.50), Some(&"mid"));
        assert_eq!(find_percentile_result(&results, 0.10), None);
    }

    #[test]
    fn test_percentile_set_from_values() {
        let values = vec![(0.05, 100.0), (0.50, 200.0), (0.95, 300.0)];
        let set = PercentileSet::from_values(&values).unwrap();

        assert_eq!(set.p5, 100.0);
        assert_eq!(set.p50, 200.0);
        assert_eq!(set.p95, 300.0);
    }

    #[test]
    fn test_percentile_set_missing_value() {
        let values = vec![(0.05, 100.0), (0.50, 200.0)]; // Missing P95
        assert!(PercentileSet::from_values(&values).is_none());
    }

    #[test]
    fn test_percentile_set_or_default() {
        let values = vec![(0.50, 200.0)]; // Only P50
        let set = PercentileSet::from_values_or_default(&values);

        assert_eq!(set.p5, 0.0);
        assert_eq!(set.p50, 200.0);
        assert_eq!(set.p95, 0.0);
    }
}
