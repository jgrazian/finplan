use std::collections::HashMap;

use jiff::civil::Date;
use rand::{Rng, distr::Distribution};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

// Note: std::collections::HashMap is used in from_profiles for return_profiles parameter

#[cfg(feature = "ts")]
use ts_rs::TS;

use crate::error::MarketError;
use crate::model::{AssetId, ReturnProfileId};

#[derive(Debug, Clone, Copy)]
struct Rate {
    incremental: f64,
    cumulative: f64,
}

/// Helper to apply a series of rates to a value over a date range.
/// Uses pre-computed cumulative rates for efficiency.
#[inline]
fn apply_rates_to_value(
    rates: &[Rate],
    start_date: Date,
    eval_date: Date,
    initial_value: f64,
) -> Option<f64> {
    if eval_date < start_date {
        return None;
    }

    if eval_date == start_date {
        return Some(initial_value);
    }

    // Calculate complete years directly (O(1) instead of O(years) loop)
    let year_diff = eval_date.year() - start_date.year();

    // Determine complete years by checking if we've passed the anniversary
    let complete_years = if year_diff <= 0 {
        0usize
    } else {
        // Check if eval_date has passed the Nth anniversary of start_date
        // Compare (month, day) to determine if anniversary has passed
        let start_month = start_date.month() as u8;
        let start_day = start_date.day();
        let eval_month = eval_date.month() as u8;
        let eval_day = eval_date.day();

        if (eval_month, eval_day) >= (start_month, start_day) {
            year_diff as usize
        } else {
            (year_diff - 1).max(0) as usize
        }
    };

    // Apply complete years using cumulative rate
    let value = if complete_years == 0 {
        initial_value
    } else if complete_years < rates.len() {
        // rates[N].cumulative = product of (1 + r[i]) for i in 0..N
        initial_value * rates[complete_years].cumulative
    } else if complete_years == rates.len() {
        // Need final cumulative: rates[N-1].cumulative * (1 + rates[N-1].incremental)
        let last_idx = rates.len() - 1;
        initial_value * rates[last_idx].cumulative * (1.0 + rates[last_idx].incremental)
    } else {
        // Not enough rate data
        return None;
    };

    // Calculate remaining days for partial year using one date arithmetic op
    if complete_years >= rates.len() {
        // No partial year rate available
        return if complete_years == rates.len() {
            Some(value)
        } else {
            None
        };
    }

    // Get the anniversary date (start_date + complete_years)
    let anniversary = start_date.saturating_add(jiff::Span::new().years(complete_years as i64));
    let remaining_days = (eval_date - anniversary).get_days();

    if remaining_days > 0 {
        let yearly_rate = rates[complete_years].incremental;
        let partial_rate = n_day_rate(yearly_rate, remaining_days as f64);
        Some(value * (1.0 + partial_rate))
    } else {
        Some(value)
    }
}

/// Convert a yearly rate to an n-day rate using compound interest
pub fn n_day_rate(yearly_rate: f64, n_days: f64) -> f64 {
    (1.0 + yearly_rate).powf(n_days / 365.0) - 1.0
}

#[derive(Debug, Clone)]
pub struct Market {
    inflation_values: Vec<Rate>,
    returns: FxHashMap<ReturnProfileId, Vec<Rate>>,
    assets: FxHashMap<AssetId, (f64, ReturnProfileId)>,
}

impl Market {
    pub fn new(
        inflation_values: Vec<f64>,
        returns: FxHashMap<ReturnProfileId, Vec<f64>>,
        assets: FxHashMap<AssetId, (f64, ReturnProfileId)>,
    ) -> Self {
        let mut inflation_rates = Vec::with_capacity(inflation_values.len());
        let mut cumulative = 1.0;

        for r in &inflation_values {
            inflation_rates.push(Rate {
                incremental: *r,
                cumulative,
            });
            cumulative *= 1.0 + r;
        }

        let mut returns_ = FxHashMap::default();
        for (rp_id, rp_values) in returns {
            let mut returns_rates = Vec::with_capacity(rp_values.len());
            let mut cumulative = 1.0;

            for r in &rp_values {
                returns_rates.push(Rate {
                    incremental: *r,
                    cumulative,
                });
                cumulative *= 1.0 + r;
            }
            returns_.insert(rp_id, returns_rates);
        }

        Self {
            inflation_values: inflation_rates,
            returns: returns_,
            assets,
        }
    }

    /// Generate market data from profiles
    pub fn from_profiles<R: Rng + ?Sized>(
        rng: &mut R,
        num_years: usize,
        inflation_profile: &InflationProfile,
        return_profiles: &HashMap<ReturnProfileId, ReturnProfile>,
        assets: &FxHashMap<AssetId, (f64, ReturnProfileId)>,
    ) -> Result<Self, MarketError> {
        let mut inflation_values = Vec::with_capacity(num_years);
        for _ in 0..num_years {
            inflation_values.push(inflation_profile.sample(rng)?);
        }

        let mut returns: FxHashMap<ReturnProfileId, Vec<f64>> = FxHashMap::default();
        for (rp_id, rp) in return_profiles.iter() {
            let mut rp_returns = Vec::with_capacity(num_years);
            for _ in 0..num_years {
                rp_returns.push(rp.sample(rng)?);
            }
            returns.insert(*rp_id, rp_returns);
        }

        Ok(Self::new(inflation_values, returns, assets.clone()))
    }

    pub fn get_asset_value(
        &self,
        start_date: Date,
        eval_date: Date,
        asset_id: AssetId,
    ) -> Option<f64> {
        let (initial_value, return_profile_id) = *self.assets.get(&asset_id)?;
        let returns = self.returns.get(&return_profile_id)?;
        apply_rates_to_value(returns, start_date, eval_date, initial_value)
    }

    /// Calculate the inflation-adjusted value of a cash amount.
    /// Returns the future nominal value needed to have the same purchasing power.
    pub fn get_inflation_adjusted_value(
        &self,
        start_date: Date,
        eval_date: Date,
        cash_amount: f64,
    ) -> Option<f64> {
        apply_rates_to_value(&self.inflation_values, start_date, eval_date, cash_amount)
    }

    /// Calculate the value of an amount after applying returns from a specific profile.
    pub fn get_return_on_value(
        &self,
        start_date: Date,
        eval_date: Date,
        initial_value: f64,
        return_profile_id: ReturnProfileId,
    ) -> Option<f64> {
        let returns = self.returns.get(&return_profile_id)?;
        apply_rates_to_value(returns, start_date, eval_date, initial_value)
    }

    /// Get the return multiplier for a period (used for cash compounding).
    /// Returns (1 + n_day_rate) for the given number of days at the year_index rate.
    pub fn get_period_multiplier(
        &self,
        year_index: usize,
        days: i64,
        return_profile_id: ReturnProfileId,
    ) -> Option<f64> {
        if days <= 0 {
            return Some(1.0);
        }
        let returns = self.returns.get(&return_profile_id)?;
        if year_index >= returns.len() {
            return None;
        }
        let yearly_rate = returns[year_index].incremental;
        let period_rate = n_day_rate(yearly_rate, days as f64);
        Some(1.0 + period_rate)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(tag = "type")]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum InflationProfile {
    #[default]
    None,
    Fixed(f64),
    Normal {
        mean: f64,
        std_dev: f64,
    },
    LogNormal {
        mean: f64,
        std_dev: f64,
    },
}

impl InflationProfile {
    pub const US_HISTORICAL_FIXED: InflationProfile = InflationProfile::Fixed(0.035432);
    pub const US_HISTORICAL_NORMAL: InflationProfile = InflationProfile::Normal {
        mean: 0.035432,
        std_dev: 0.027807,
    };
    pub const US_HISTORICAL_LOG_NORMAL: InflationProfile = InflationProfile::LogNormal {
        mean: 0.035432,
        std_dev: 0.026317,
    };

    pub fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Result<f64, MarketError> {
        match self {
            InflationProfile::None => Ok(0.0),
            InflationProfile::Fixed(rate) => Ok(*rate),
            InflationProfile::Normal { mean, std_dev } => rand_distr::Normal::new(*mean, *std_dev)
                .map(|d| d.sample(rng))
                .map_err(|_| MarketError::InvalidDistributionParameters {
                    profile_type: "Normal inflation",
                    mean: *mean,
                    std_dev: *std_dev,
                    reason: "std_dev must be non-negative and finite",
                }),
            InflationProfile::LogNormal { mean, std_dev } => {
                rand_distr::LogNormal::new(*mean, *std_dev)
                    .map(|d| d.sample(rng) - 1.0)
                    .map_err(|_| MarketError::InvalidDistributionParameters {
                        profile_type: "LogNormal inflation",
                        mean: *mean,
                        std_dev: *std_dev,
                        reason: "std_dev must be positive and finite",
                    })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum ReturnProfile {
    None,
    Fixed(f64),
    Normal { mean: f64, std_dev: f64 },
    LogNormal { mean: f64, std_dev: f64 },
}

impl ReturnProfile {
    pub const SP_500_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.095668);
    pub const SP_500_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.095668,
        std_dev: 0.165234,
    };
    pub const SP_500_HISTORICAL_LOG_NORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.079088,
        std_dev: 0.161832,
    };

    pub fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Result<f64, MarketError> {
        match self {
            ReturnProfile::None => Ok(0.0),
            ReturnProfile::Fixed(rate) => Ok(*rate),
            ReturnProfile::Normal { mean, std_dev } => rand_distr::Normal::new(*mean, *std_dev)
                .map(|d| d.sample(rng))
                .map_err(|_| MarketError::InvalidDistributionParameters {
                    profile_type: "Normal return",
                    mean: *mean,
                    std_dev: *std_dev,
                    reason: "std_dev must be non-negative and finite",
                }),
            ReturnProfile::LogNormal { mean, std_dev } => {
                rand_distr::LogNormal::new(*mean, *std_dev)
                    .map(|d| d.sample(rng) - 1.0)
                    .map_err(|_| MarketError::InvalidDistributionParameters {
                        profile_type: "LogNormal return",
                        mean: *mean,
                        std_dev: *std_dev,
                        reason: "std_dev must be positive and finite",
                    })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssetId, ReturnProfileId};
    use jiff::civil::date;

    #[test]
    fn test_get_asset_value() {
        let asset_id = AssetId(1);
        let rp_id = ReturnProfileId(1);

        let assets = FxHashMap::from_iter([(asset_id, (1000.0, rp_id))]);
        let returns = FxHashMap::from_iter([(rp_id, vec![0.10, 0.05])]);
        let inflation = vec![0.02, 0.02];

        let market = Market::new(inflation, returns, assets);

        let start_date = date(2024, 1, 1);

        // Exact start date
        let val = market
            .get_asset_value(start_date, date(2024, 1, 1), asset_id)
            .unwrap();
        assert!((val - 1000.0).abs() < 1e-6);

        // One full year
        let val = market
            .get_asset_value(start_date, date(2025, 1, 1), asset_id)
            .unwrap();
        assert!((val - 1100.0).abs() < 1e-6);

        // Two full years
        let val = market
            .get_asset_value(start_date, date(2026, 1, 1), asset_id)
            .unwrap();
        // 1100 * 1.05 = 1155
        assert!((val - 1155.0).abs() < 1e-6);

        // Partial year (6 months approx)
        // n_day_rate implementation: (1.0 + yearly_rate).powf(n_days / 365.0) - 1.0
        let eval_date = date(2024, 7, 2); // 183 days after Jan 1
        let days = (eval_date - start_date).get_days() as f64;
        let expected_rate = (1.10_f64).powf(days / 365.0) - 1.0;
        let expected_val = 1000.0 * (1.0 + expected_rate);

        let val = market
            .get_asset_value(start_date, eval_date, asset_id)
            .unwrap();
        assert!((val - expected_val).abs() < 1e-6);

        // Before start date
        let val = market.get_asset_value(start_date, date(2023, 12, 31), asset_id);
        assert!(val.is_none());
    }
}
