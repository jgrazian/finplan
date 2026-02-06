use std::borrow::Cow;
use std::collections::HashMap;

use jiff::civil::Date;
use rand::{Rng, distr::Distribution};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

// Note: std::collections::HashMap is used in from_profiles for return_profiles parameter

use crate::error::{LookupError, MarketError};
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
        let partial_rate = n_day_rate(yearly_rate, f64::from(remaining_days));
        Some(value * (1.0 + partial_rate))
    } else {
        Some(value)
    }
}

/// Convert a yearly rate to an n-day rate using compound interest
#[must_use]
#[inline]
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
    #[must_use]
    pub fn new(
        inflation_values: &[f64],
        returns: FxHashMap<ReturnProfileId, Vec<f64>>,
        assets: FxHashMap<AssetId, (f64, ReturnProfileId)>,
    ) -> Self {
        let mut inflation_rates = Vec::with_capacity(inflation_values.len());
        let mut cumulative = 1.0;

        for r in inflation_values {
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

            for r in rp_values {
                returns_rates.push(Rate {
                    incremental: r,
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

    /// Generate market data from profiles.
    ///
    /// For `RegimeSwitching` return profiles and `Bootstrap` inflation profiles,
    /// this properly maintains state across years, modeling the clustering of
    /// good and bad market periods and inflation persistence.
    pub fn from_profiles<R: Rng + ?Sized>(
        rng: &mut R,
        num_years: usize,
        inflation_profile: &InflationProfile,
        return_profiles: &HashMap<ReturnProfileId, ReturnProfile>,
        assets: &FxHashMap<AssetId, (f64, ReturnProfileId)>,
    ) -> Result<Self, MarketError> {
        // Use sample_sequence for inflation to support block bootstrap
        let inflation_values = inflation_profile.sample_sequence(rng, num_years)?;

        let mut returns: FxHashMap<ReturnProfileId, Vec<f64>> = FxHashMap::default();
        // Sort profile IDs for deterministic iteration order
        // HashMap iteration is non-deterministic across process invocations,
        // which would cause different random returns per profile with the same seed
        let mut profile_ids: Vec<_> = return_profiles.keys().copied().collect();
        profile_ids.sort_by_key(|id| id.0);
        for rp_id in profile_ids {
            let rp = &return_profiles[&rp_id];
            // Use sample_sequence for proper regime-switching support
            let rp_returns = rp.sample_sequence(rng, num_years)?;
            returns.insert(rp_id, rp_returns);
        }

        Ok(Self::new(&inflation_values, returns, assets.clone()))
    }

    pub fn get_asset_value(
        &self,
        start_date: Date,
        eval_date: Date,
        asset_id: AssetId,
    ) -> Result<f64, MarketError> {
        let (initial_value, return_profile_id) = *self
            .assets
            .get(&asset_id)
            .ok_or(LookupError::AssetIdNotFound(asset_id))?;
        let returns = self
            .returns
            .get(&return_profile_id)
            .ok_or(LookupError::ReturnProfileNotFound(return_profile_id))?;
        apply_rates_to_value(returns, start_date, eval_date, initial_value)
            .ok_or(MarketError::InsufficientRateData)
    }

    /// Calculate the inflation-adjusted value of a cash amount.
    /// Returns the future nominal value needed to have the same purchasing power.
    pub fn get_inflation_adjusted_value(
        &self,
        start_date: Date,
        eval_date: Date,
        cash_amount: f64,
    ) -> Result<f64, MarketError> {
        apply_rates_to_value(&self.inflation_values, start_date, eval_date, cash_amount)
            .ok_or(MarketError::InsufficientRateData)
    }

    /// Calculate the value of an amount after applying returns from a specific profile.
    pub fn get_return_on_value(
        &self,
        start_date: Date,
        eval_date: Date,
        initial_value: f64,
        return_profile_id: ReturnProfileId,
    ) -> Result<f64, MarketError> {
        let returns = self
            .returns
            .get(&return_profile_id)
            .ok_or(LookupError::ReturnProfileNotFound(return_profile_id))?;
        apply_rates_to_value(returns, start_date, eval_date, initial_value)
            .ok_or(MarketError::InsufficientRateData)
    }

    /// Get the return multiplier for a period (used for cash compounding).
    /// Returns (1 + `n_day_rate`) for the given number of days at the `year_index` rate.
    pub fn get_period_multiplier(
        &self,
        year_index: usize,
        days: i64,
        return_profile_id: ReturnProfileId,
    ) -> Result<f64, MarketError> {
        if days <= 0 {
            return Ok(1.0);
        }
        let returns = self
            .returns
            .get(&return_profile_id)
            .ok_or(LookupError::ReturnProfileNotFound(return_profile_id))?;
        if year_index >= returns.len() {
            return Err(MarketError::InsufficientRateData);
        }
        let yearly_rate = returns[year_index].incremental;
        let period_rate = n_day_rate(yearly_rate, days as f64);
        Ok(1.0 + period_rate)
    }

    /// Get cumulative inflation factors for each year of the simulation.
    ///
    /// Returns a vector where index i represents the cumulative inflation from
    /// the start of the simulation through year i. The first element (year 0) is 1.0,
    /// representing today's dollars. Each subsequent year is multiplied by (1 + `inflation_rate`).
    ///
    /// These factors can be used to convert nominal future values to real (today's) dollars
    /// by dividing: `real_value` = `nominal_value` / `cumulative_inflation`[`year_index`]
    #[must_use]
    pub fn get_cumulative_inflation_factors(&self) -> Vec<f64> {
        // Build cumulative factors: [1.0, 1.0*(1+r0), 1.0*(1+r0)*(1+r1), ...]
        // Note: inflation_values stores Rate { incremental, cumulative } where
        // cumulative is the product BEFORE applying this year's rate
        let mut factors = Vec::with_capacity(self.inflation_values.len() + 1);
        factors.push(1.0); // Year 0 = today's dollars

        let mut cumulative = 1.0;
        for rate in &self.inflation_values {
            cumulative *= 1.0 + rate.incremental;
            factors.push(cumulative);
        }

        factors
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type")]
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
    /// Bootstrap sampling from historical inflation data.
    /// Non-parametric approach that samples directly from observed historical inflation rates.
    /// - `history`: Historical inflation data to sample from
    /// - `block_size`: Optional block size for block bootstrap (preserves autocorrelation).
    ///   If None or 1, uses i.i.d. sampling with replacement.
    Bootstrap {
        history: HistoricalInflation,
        #[serde(default)]
        block_size: Option<usize>,
    },
}

impl InflationProfile {
    // US CPI Inflation (All Urban Consumers)
    // Source: FRED (CPIAUCSL)
    // Data: 1948-2025 (78 years)
    // Arithmetic mean: 0.0347, Geometric mean: 0.0343
    // Std dev: 0.0279
    pub const US_HISTORICAL_FIXED: InflationProfile = InflationProfile::Fixed(0.0343436);
    pub const US_HISTORICAL_NORMAL: InflationProfile = InflationProfile::Normal {
        mean: 0.0347068,
        std_dev: 0.0279436,
    };
    pub const US_HISTORICAL_LOG_NORMAL: InflationProfile = InflationProfile::LogNormal {
        mean: 0.0347068,
        std_dev: 0.0279436,
    };

    /// Create a bootstrap inflation profile from US historical CPI data.
    #[must_use]
    pub fn us_historical_bootstrap(block_size: Option<usize>) -> Self {
        InflationProfile::Bootstrap {
            history: HistoricalInflation::us_cpi(),
            block_size,
        }
    }

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
            InflationProfile::Bootstrap { history, .. } => {
                history.sample(rng).ok_or(MarketError::EmptyHistoricalData)
            }
        }
    }

    /// Sample a sequence of inflation rates.
    ///
    /// For `Bootstrap` profiles, this uses block bootstrap if `block_size` > 1,
    /// otherwise i.i.d. sampling. Other profile types sample independently each year.
    pub fn sample_sequence<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        n: usize,
    ) -> Result<Vec<f64>, MarketError> {
        if let InflationProfile::Bootstrap {
            history,
            block_size,
        } = self
        {
            let bs = block_size.unwrap_or(1);
            if bs > 1 {
                history
                    .block_bootstrap(rng, n, bs)
                    .ok_or(MarketError::EmptyHistoricalData)
            } else {
                history
                    .sample_years(rng, n)
                    .ok_or(MarketError::EmptyHistoricalData)
            }
        } else {
            let mut results = Vec::with_capacity(n);
            for _ in 0..n {
                results.push(self.sample(rng)?);
            }
            Ok(results)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfile {
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
    /// Student's t distribution for fat-tailed returns.
    /// Better captures extreme market events than Normal distribution.
    /// - `mean`: Expected return (location parameter)
    /// - `scale`: Scale parameter (similar to `std_dev` but adjusted for df)
    /// - `df`: Degrees of freedom (lower = fatter tails, typically 4-6 for equities)
    StudentT {
        mean: f64,
        scale: f64,
        df: f64,
    },
    /// Markov regime-switching model with bull/bear market states.
    /// Captures the tendency of markets to cluster good and bad years.
    /// - `bull`: Return profile during bull markets (typically higher returns, lower volatility)
    /// - `bear`: Return profile during bear markets (typically lower/negative returns, higher volatility)
    /// - `bull_to_bear_prob`: Annual probability of transitioning from bull to bear (e.g., 0.12 = ~8yr bull cycles)
    /// - `bear_to_bull_prob`: Annual probability of transitioning from bear to bull (e.g., 0.50 = ~2yr bear cycles)
    RegimeSwitching {
        bull: Box<ReturnProfile>,
        bear: Box<ReturnProfile>,
        bull_to_bear_prob: f64,
        bear_to_bull_prob: f64,
    },
    /// Bootstrap sampling from historical return data.
    /// Non-parametric approach that samples directly from observed historical returns.
    /// - `preset`: Preset key identifying the historical data source (e.g., "sp500", `us_small_cap`)
    /// - `block_size`: Optional block size for block bootstrap (preserves autocorrelation).
    ///   If None or 1, uses i.i.d. sampling with replacement.
    Bootstrap {
        history: HistoricalReturns,
        #[serde(default)]
        block_size: Option<usize>,
    },
}

impl ReturnProfile {
    /// Sample a single return from this profile.
    ///
    /// For `RegimeSwitching`, this samples statelessly using steady-state regime probabilities.
    /// Use `sample_sequence()` for proper stateful regime-switching simulation.
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
            ReturnProfile::StudentT { mean, scale, df } => rand_distr::StudentT::new(*df)
                .map(|d| mean + scale * d.sample(rng))
                .map_err(|_| MarketError::InvalidDistributionParameters {
                    profile_type: "StudentT return",
                    mean: *mean,
                    std_dev: *scale,
                    reason: "degrees of freedom must be positive and finite",
                }),
            ReturnProfile::RegimeSwitching {
                bull,
                bear,
                bull_to_bear_prob,
                bear_to_bull_prob,
            } => {
                // For stateless sampling, use steady-state regime probabilities
                // P(bull) = bear_to_bull / (bull_to_bear + bear_to_bull)
                let total_prob = bull_to_bear_prob + bear_to_bull_prob;
                if total_prob <= 0.0 {
                    return Err(MarketError::InvalidDistributionParameters {
                        profile_type: "RegimeSwitching return",
                        mean: 0.0,
                        std_dev: 0.0,
                        reason: "transition probabilities must be positive",
                    });
                }
                let bull_steady_state = bear_to_bull_prob / total_prob;
                if rng.random::<f64>() < bull_steady_state {
                    bull.sample(rng)
                } else {
                    bear.sample(rng)
                }
            }
            ReturnProfile::Bootstrap { history, .. } => {
                history
                    .sample(rng)
                    .ok_or(MarketError::InvalidDistributionParameters {
                        profile_type: "Bootstrap return",
                        mean: 0.0,
                        std_dev: 0.0,
                        reason: "historical returns data is empty or unknown preset",
                    })
            }
        }
    }

    /// Sample a sequence of returns with proper regime state tracking.
    ///
    /// For `RegimeSwitching`, this maintains regime state across years,
    /// properly modeling the tendency of markets to cluster good and bad years.
    /// For other profile types, this is equivalent to calling `sample()` n times.
    pub fn sample_sequence<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        num_years: usize,
    ) -> Result<Vec<f64>, MarketError> {
        match self {
            ReturnProfile::RegimeSwitching {
                bull,
                bear,
                bull_to_bear_prob,
                bear_to_bull_prob,
            } => {
                let mut returns = Vec::with_capacity(num_years);
                let mut in_bull = true; // Start in bull market

                for _ in 0..num_years {
                    // Sample return from current regime
                    let ret = if in_bull {
                        bull.sample(rng)?
                    } else {
                        bear.sample(rng)?
                    };
                    returns.push(ret);

                    // Transition regime for next year
                    let transition_prob = if in_bull {
                        *bull_to_bear_prob
                    } else {
                        *bear_to_bull_prob
                    };
                    if rng.random::<f64>() < transition_prob {
                        in_bull = !in_bull;
                    }
                }

                Ok(returns)
            }
            ReturnProfile::Bootstrap {
                history,
                block_size,
            } => {
                // Use block bootstrap if block_size > 1, otherwise i.i.d.
                let result = match block_size {
                    Some(bs) if *bs > 1 => history.block_bootstrap(rng, num_years, *bs),
                    _ => history.sample_years(rng, num_years),
                };
                result.ok_or(MarketError::InvalidDistributionParameters {
                    profile_type: "Bootstrap return",
                    mean: 0.0,
                    std_dev: 0.0,
                    reason: "historical returns data is empty",
                })
            }
            // For non-regime-switching profiles, just sample independently
            _ => {
                let mut returns = Vec::with_capacity(num_years);
                for _ in 0..num_years {
                    returns.push(self.sample(rng)?);
                }
                Ok(returns)
            }
        }
    }
}

// Auto-generated by scripts/fetch_historical_returns.py
// Generated: 2026-01-26T16:13:37.329914
//
// Data Sources:
//   - Robert Shiller, Yale University (S&P 500 since 1871)
//   - Kenneth French Data Library, Dartmouth (Fama-French factors since 1926)
//   - Yahoo Finance (ETF data for recent history)
impl ReturnProfile {
    // US Large Cap Stocks (S&P 500 Total Return)
    // Source: Robert Shiller, Yale University
    // Data: 1927-2023 (97 years)
    // Arithmetic mean: 0.1147, Geometric mean: 0.0991
    // Std dev: 0.1815, Skewness: -0.27, Kurtosis: 0.11
    pub const SP_500_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.0990829);
    pub const SP_500_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.11471,
        std_dev: 0.18146,
    };
    pub const SP_500_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.11471,
        std_dev: 0.18146,
    };
    pub const SP_500_HISTORICAL_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
        mean: 0.11471,
        scale: 0.140558,
        df: 5.0,
    };

    // US Small Cap Stocks (Market + SMB Factor)
    // Source: Kenneth French Data Library, Dartmouth
    // Data: 1927-2024 (98 years)
    // Arithmetic mean: 0.1477, Geometric mean: 0.1120
    // Std dev: 0.2780, Skewness: 0.09, Kurtosis: 0.32
    pub const US_SMALL_CAP_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.112028);
    pub const US_SMALL_CAP_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.147749,
        std_dev: 0.278003,
    };
    pub const US_SMALL_CAP_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.147749,
        std_dev: 0.278003,
    };
    pub const US_SMALL_CAP_HISTORICAL_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
        mean: 0.147749,
        scale: 0.21534,
        df: 5.0,
    };

    // US 3-Month Treasury Bills
    // Source: FRED (TB3MS)
    // Data: 1934-2025 (92 years)
    // Arithmetic mean: 0.0342, Geometric mean: 0.0337
    // Std dev: 0.0305, Skewness: 0.96, Kurtosis: 0.78
    pub const US_TBILLS_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.0337398);
    pub const US_TBILLS_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.0341782,
        std_dev: 0.0305423,
    };
    pub const US_TBILLS_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.0341782,
        std_dev: 0.0305423,
    };

    // US Long-Term Government Bonds (estimated from yields)
    // Source: Robert Shiller, Yale University (estimated)
    // Data: 1927-2023 (97 years)
    // Arithmetic mean: 0.0477, Geometric mean: 0.0455
    // Std dev: 0.0701, Skewness: 1.52, Kurtosis: 4.22
    pub const US_LONG_BOND_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.0455233);
    pub const US_LONG_BOND_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.047717,
        std_dev: 0.0700793,
    };
    pub const US_LONG_BOND_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.047717,
        std_dev: 0.0700793,
    };
    pub const US_LONG_BOND_HISTORICAL_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
        mean: 0.047717,
        scale: 0.0542832,
        df: 5.0,
    };

    // Developed Markets ex-US (Fama-French)
    // Source: Kenneth French Data Library, Dartmouth
    // Data: 1991-2024 (34 years)
    // Arithmetic mean: 0.0778, Geometric mean: 0.0603
    // Std dev: 0.1883, Skewness: -0.47, Kurtosis: 0.24
    pub const INTL_DEVELOPED_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.0602527);
    pub const INTL_DEVELOPED_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.0778324,
        std_dev: 0.188273,
    };
    pub const INTL_DEVELOPED_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.0778324,
        std_dev: 0.188273,
    };
    pub const INTL_DEVELOPED_HISTORICAL_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
        mean: 0.0778324,
        scale: 0.145836,
        df: 5.0,
    };

    // Emerging Markets (Fama-French)
    // Source: Kenneth French Data Library, Dartmouth
    // Data: 1991-2024 (33 years)
    // Arithmetic mean: 0.1073, Geometric mean: 0.0507
    // Std dev: 0.3475, Skewness: 0.54, Kurtosis: 0.99
    pub const EMERGING_MARKETS_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.0507225);
    pub const EMERGING_MARKETS_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.107264,
        std_dev: 0.347473,
    };
    pub const EMERGING_MARKETS_HISTORICAL_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
        mean: 0.107264,
        scale: 0.269151,
        df: 5.0,
    };

    // US Real Estate Investment Trusts (via VNQ)
    // Source: Yahoo Finance
    // Data: 2005-2026 (22 years)
    // Arithmetic mean: 0.0828, Geometric mean: 0.0642
    // Std dev: 0.1959, Skewness: -0.44, Kurtosis: 0.18
    pub const REITS_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.0642145);
    pub const REITS_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.082752,
        std_dev: 0.195905,
    };
    pub const REITS_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.082752,
        std_dev: 0.195905,
    };
    pub const REITS_HISTORICAL_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
        mean: 0.082752,
        scale: 0.151748,
        df: 5.0,
    };

    // Gold (via GC=F futures)
    // Source: Yahoo Finance
    // Data: 2001-2026 (26 years)
    // Arithmetic mean: 0.1317, Geometric mean: 0.1189
    // Std dev: 0.1734, Skewness: 0.45, Kurtosis: 2.63
    pub const GOLD_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.118905);
    pub const GOLD_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.131744,
        std_dev: 0.173436,
    };
    pub const GOLD_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.131744,
        std_dev: 0.173436,
    };
    pub const GOLD_HISTORICAL_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
        mean: 0.131744,
        scale: 0.134343,
        df: 5.0,
    };

    // US Investment Grade Bonds (Bloomberg Aggregate via AGG)
    // Source: Yahoo Finance
    // Data: 2004-2026 (23 years)
    // Arithmetic mean: 0.0312, Geometric mean: 0.0301
    // Std dev: 0.0469, Skewness: -1.88, Kurtosis: 5.44
    pub const US_AGG_BOND_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.0301011);
    pub const US_AGG_BOND_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.0311818,
        std_dev: 0.0468972,
    };
    pub const US_AGG_BOND_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.0311818,
        std_dev: 0.0468972,
    };

    // US Investment Grade Corporate Bonds (via LQD)
    // Source: Yahoo Finance
    // Data: 2003-2026 (24 years)
    // Arithmetic mean: 0.0441, Geometric mean: 0.0418
    // Std dev: 0.0698, Skewness: -1.28, Kurtosis: 3.54
    pub const US_CORPORATE_BOND_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.0417664);
    pub const US_CORPORATE_BOND_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.0441447,
        std_dev: 0.0697513,
    };
    pub const US_CORPORATE_BOND_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.0441447,
        std_dev: 0.0697513,
    };
    pub const US_CORPORATE_BOND_HISTORICAL_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
        mean: 0.0441447,
        scale: 0.0540291,
        df: 5.0,
    };

    // US Treasury Inflation-Protected Securities (via TIP)
    // Source: Yahoo Finance
    // Data: 2004-2026 (23 years)
    // Arithmetic mean: 0.0359, Geometric mean: 0.0341
    // Std dev: 0.0607, Skewness: -0.85, Kurtosis: 1.17
    pub const TIPS_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.0341326);
    pub const TIPS_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.0358924,
        std_dev: 0.0606518,
    };
    pub const TIPS_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {
        mean: 0.0358924,
        std_dev: 0.0606518,
    };
    pub const TIPS_HISTORICAL_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
        mean: 0.0358924,
        scale: 0.0469807,
        df: 5.0,
    };

    // =========================================================================
    // Regime-Switching Presets
    // =========================================================================
    // These are functions (not const) because Box allocation isn't const.
    // Based on historical analysis of S&P 500 bull/bear market cycles:
    // - Bull markets: avg duration ~8 years, higher returns (~15%), lower vol (~12%)
    // - Bear markets: avg duration ~2 years, lower returns (~-8%), higher vol (~25%)

    /// S&P 500 regime-switching model with Normal distributions for each regime.
    /// Bull: 15% mean, 12% `std_dev` | Bear: -8% mean, 25% `std_dev`
    /// Transition: ~12% bull->bear (8yr cycles), ~50% bear->bull (2yr cycles)
    #[must_use]
    pub fn sp500_regime_switching_normal() -> ReturnProfile {
        ReturnProfile::RegimeSwitching {
            bull: Box::new(ReturnProfile::Normal {
                mean: 0.15,
                std_dev: 0.12,
            }),
            bear: Box::new(ReturnProfile::Normal {
                mean: -0.08,
                std_dev: 0.25,
            }),
            bull_to_bear_prob: 0.12,
            bear_to_bull_prob: 0.50,
        }
    }

    /// S&P 500 regime-switching model with Student's t distributions for fat tails.
    /// Same parameters as normal but with df=5 for each regime.
    #[must_use]
    pub fn sp500_regime_switching_student_t() -> ReturnProfile {
        ReturnProfile::RegimeSwitching {
            bull: Box::new(ReturnProfile::StudentT {
                mean: 0.15,
                scale: 0.12 * (3.0_f64 / 5.0).sqrt(), // Adjust for df=5
                df: 5.0,
            }),
            bear: Box::new(ReturnProfile::StudentT {
                mean: -0.08,
                scale: 0.25 * (3.0_f64 / 5.0).sqrt(), // Adjust for df=5
                df: 5.0,
            }),
            bull_to_bear_prob: 0.12,
            bear_to_bull_prob: 0.50,
        }
    }

    /// Create a custom regime-switching profile.
    #[must_use]
    pub fn regime_switching(
        bull: ReturnProfile,
        bear: ReturnProfile,
        bull_to_bear_prob: f64,
        bear_to_bull_prob: f64,
    ) -> ReturnProfile {
        ReturnProfile::RegimeSwitching {
            bull: Box::new(bull),
            bear: Box::new(bear),
            bull_to_bear_prob,
            bear_to_bull_prob,
        }
    }

    // =========================================================================
    // Bootstrap Presets
    // =========================================================================
    // Non-parametric sampling from historical return data.
    // Preserves the actual distribution shape including fat tails and skewness.

    /// Bootstrap from S&P 500 historical returns (1927-2023, 97 years).
    /// Uses i.i.d. sampling with replacement.
    #[must_use]
    pub fn sp500_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::sp500(),
            block_size: None,
        }
    }

    /// Bootstrap from S&P 500 with 5-year blocks (preserves momentum/mean-reversion).
    #[must_use]
    pub fn sp500_bootstrap_block5() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::sp500(),
            block_size: Some(5),
        }
    }

    /// Bootstrap from US Small Cap historical returns (1927-2024, 98 years).
    #[must_use]
    pub fn us_small_cap_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::us_small_cap(),
            block_size: None,
        }
    }

    /// Bootstrap from US T-Bills historical returns (1934-2025, 92 years).
    #[must_use]
    pub fn us_tbills_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::us_tbills(),
            block_size: None,
        }
    }

    /// Bootstrap from US Long-Term Bonds historical returns (1927-2023, 97 years).
    #[must_use]
    pub fn us_long_bonds_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::us_long_bonds(),
            block_size: None,
        }
    }

    /// Bootstrap from International Developed Markets (1991-2024, 34 years).
    #[must_use]
    pub fn intl_developed_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::intl_developed(),
            block_size: None,
        }
    }

    /// Bootstrap from Emerging Markets (1991-2024, 33 years).
    #[must_use]
    pub fn emerging_markets_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::emerging_markets(),
            block_size: None,
        }
    }

    /// Bootstrap from US REITs (2005-2026, 22 years).
    #[must_use]
    pub fn reits_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::reits(),
            block_size: None,
        }
    }

    /// Bootstrap from Gold (2001-2026, 26 years).
    #[must_use]
    pub fn gold_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::gold(),
            block_size: None,
        }
    }

    /// Bootstrap from US Aggregate Bonds (2004-2026, 23 years).
    #[must_use]
    pub fn us_agg_bonds_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::us_agg_bonds(),
            block_size: None,
        }
    }

    /// Bootstrap from US Corporate Bonds (2003-2026, 24 years).
    #[must_use]
    pub fn us_corporate_bonds_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::us_corporate_bonds(),
            block_size: None,
        }
    }

    /// Bootstrap from US TIPS (2004-2026, 23 years).
    #[must_use]
    pub fn tips_bootstrap() -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history: HistoricalReturns::tips(),
            block_size: None,
        }
    }

    /// Create a custom bootstrap profile from historical data.
    #[must_use]
    pub fn bootstrap(history: HistoricalReturns, block_size: Option<usize>) -> ReturnProfile {
        ReturnProfile::Bootstrap {
            history,
            block_size,
        }
    }
}

/// Historical return series for bootstrap sampling.
///
/// Enables non-parametric simulation by sampling directly from historical data.
/// Supports both i.i.d. sampling and block bootstrap (preserves autocorrelation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalReturns {
    /// Asset/index name for display purposes
    pub name: Cow<'static, str>,
    /// Starting year of the data series
    pub start_year: i16,
    /// Annual returns (index 0 = `start_year`)
    pub returns: Cow<'static, [f64]>,
}

impl HistoricalReturns {
    /// Create a new historical returns series.
    #[must_use]
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        start_year: i16,
        returns: impl Into<Cow<'static, [f64]>>,
    ) -> Self {
        Self {
            name: name.into(),
            start_year,
            returns: returns.into(),
        }
    }

    /// Number of years of historical data available.
    #[must_use]
    pub fn len(&self) -> usize {
        self.returns.len()
    }

    /// Returns true if the historical data is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.returns.is_empty()
    }

    /// Sample a random year's return (i.i.d. with replacement).
    pub fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Option<f64> {
        if self.returns.is_empty() {
            return None;
        }
        let idx = rng.random_range(0..self.returns.len());
        Some(self.returns[idx])
    }

    /// Sample n years with replacement (i.i.d. bootstrap).
    pub fn sample_years<R: Rng + ?Sized>(&self, rng: &mut R, n: usize) -> Option<Vec<f64>> {
        if self.returns.is_empty() {
            return None;
        }
        Some(
            (0..n)
                .map(|_| self.returns[rng.random_range(0..self.returns.len())])
                .collect(),
        )
    }

    /// Block bootstrap: sample contiguous blocks to preserve autocorrelation.
    ///
    /// This is useful for capturing momentum/mean-reversion effects in returns.
    /// Blocks wrap around at the end of the series (circular bootstrap).
    ///
    /// # Arguments
    /// * `rng` - Random number generator
    /// * `n` - Number of years to sample
    /// * `block_size` - Size of contiguous blocks to sample
    pub fn block_bootstrap<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        n: usize,
        block_size: usize,
    ) -> Option<Vec<f64>> {
        if self.returns.is_empty() || block_size == 0 {
            return None;
        }
        let mut result = Vec::with_capacity(n);
        while result.len() < n {
            let start = rng.random_range(0..self.returns.len());
            for i in 0..block_size {
                if result.len() >= n {
                    break;
                }
                // Circular wrap for blocks that extend past the end
                let idx = (start + i) % self.returns.len();
                result.push(self.returns[idx]);
            }
        }
        Some(result)
    }

    /// Compute basic statistics of the historical returns.
    pub fn statistics(&self) -> Option<HistoricalStatistics> {
        if self.returns.is_empty() {
            return None;
        }
        let n = self.returns.len() as f64;
        let arithmetic_mean = self.returns.iter().sum::<f64>() / n;

        // Geometric mean: (product of (1+r))^(1/n) - 1
        let product: f64 = self.returns.iter().map(|r| 1.0 + r).product();
        let geometric_mean = product.powf(1.0 / n) - 1.0;

        let variance = self
            .returns
            .iter()
            .map(|r| (r - arithmetic_mean).powi(2))
            .sum::<f64>()
            / n;
        let std_dev = variance.sqrt();

        let min = self.returns.iter().copied().fold(f64::INFINITY, f64::min);
        let max = self
            .returns
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);

        Some(HistoricalStatistics {
            arithmetic_mean,
            geometric_mean,
            std_dev,
            min,
            max,
            years: self.returns.len(),
        })
    }

    // =========================================================================
    // Preset Historical Data Constructors
    // =========================================================================

    /// S&P 500 Total Return (1927-2023, 97 years)
    #[must_use]
    pub fn sp500() -> Self {
        Self::new("S&P 500", 1927, historical_returns::SP_500_ANNUAL_RETURNS)
    }

    /// US Small Cap Stocks (1927-2024, 98 years)
    #[must_use]
    pub fn us_small_cap() -> Self {
        Self::new(
            "US Small Cap",
            1927,
            historical_returns::US_SMALL_CAP_ANNUAL_RETURNS,
        )
    }

    /// US 3-Month Treasury Bills (1934-2025, 92 years)
    #[must_use]
    pub fn us_tbills() -> Self {
        Self::new(
            "US T-Bills",
            1934,
            historical_returns::US_TBILLS_ANNUAL_RETURNS,
        )
    }

    /// US Long-Term Government Bonds (1927-2023, 97 years)
    #[must_use]
    pub fn us_long_bonds() -> Self {
        Self::new(
            "US Long Bonds",
            1927,
            historical_returns::US_LONG_BOND_ANNUAL_RETURNS,
        )
    }

    /// Developed Markets ex-US (1991-2024, 34 years)
    #[must_use]
    pub fn intl_developed() -> Self {
        Self::new(
            "Intl Developed",
            1991,
            historical_returns::INTL_DEVELOPED_ANNUAL_RETURNS,
        )
    }

    /// Emerging Markets (1991-2024, 33 years)
    #[must_use]
    pub fn emerging_markets() -> Self {
        Self::new(
            "Emerging Markets",
            1991,
            historical_returns::EMERGING_MARKETS_ANNUAL_RETURNS,
        )
    }

    /// US REITs (2005-2026, 22 years)
    #[must_use]
    pub fn reits() -> Self {
        Self::new("REITs", 2005, historical_returns::REITS_ANNUAL_RETURNS)
    }

    /// Gold (2001-2026, 26 years)
    #[must_use]
    pub fn gold() -> Self {
        Self::new("Gold", 2001, historical_returns::GOLD_ANNUAL_RETURNS)
    }

    /// US Aggregate Bonds (2004-2026, 23 years)
    #[must_use]
    pub fn us_agg_bonds() -> Self {
        Self::new(
            "US Agg Bonds",
            2004,
            historical_returns::US_AGG_BOND_ANNUAL_RETURNS,
        )
    }

    /// US Corporate Bonds (2003-2026, 24 years)
    #[must_use]
    pub fn us_corporate_bonds() -> Self {
        Self::new(
            "US Corporate Bonds",
            2003,
            historical_returns::US_CORPORATE_BOND_ANNUAL_RETURNS,
        )
    }

    /// US TIPS (2004-2026, 23 years)
    #[must_use]
    pub fn tips() -> Self {
        Self::new("TIPS", 2004, historical_returns::TIPS_ANNUAL_RETURNS)
    }
}

/// Basic statistics for historical returns.
#[derive(Debug, Clone, Copy)]
pub struct HistoricalStatistics {
    pub arithmetic_mean: f64,
    pub geometric_mean: f64,
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
    pub years: usize,
}

/// Historical inflation series for bootstrap sampling.
///
/// Enables non-parametric simulation by sampling directly from historical inflation data.
/// Supports both i.i.d. sampling and block bootstrap (preserves autocorrelation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalInflation {
    /// Name/source for display purposes
    pub name: Cow<'static, str>,
    /// Starting year of the data series
    pub start_year: i16,
    /// Annual inflation rates (index 0 = `start_year`)
    pub rates: Cow<'static, [f64]>,
}

impl HistoricalInflation {
    /// Create a new historical inflation series.
    #[must_use]
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        start_year: i16,
        rates: impl Into<Cow<'static, [f64]>>,
    ) -> Self {
        Self {
            name: name.into(),
            start_year,
            rates: rates.into(),
        }
    }

    /// Number of years of historical data available.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rates.len()
    }

    /// Returns true if the historical data is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rates.is_empty()
    }

    /// Sample a random year's inflation rate (i.i.d. with replacement).
    pub fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Option<f64> {
        if self.rates.is_empty() {
            return None;
        }
        let idx = rng.random_range(0..self.rates.len());
        Some(self.rates[idx])
    }

    /// Sample n years with replacement (i.i.d. bootstrap).
    pub fn sample_years<R: Rng + ?Sized>(&self, rng: &mut R, n: usize) -> Option<Vec<f64>> {
        if self.rates.is_empty() {
            return None;
        }
        Some(
            (0..n)
                .map(|_| self.rates[rng.random_range(0..self.rates.len())])
                .collect(),
        )
    }

    /// Block bootstrap: sample contiguous blocks to preserve autocorrelation.
    ///
    /// This is useful for capturing inflation persistence effects.
    /// Blocks wrap around at the end of the series (circular bootstrap).
    pub fn block_bootstrap<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        n: usize,
        block_size: usize,
    ) -> Option<Vec<f64>> {
        if self.rates.is_empty() || block_size == 0 {
            return None;
        }
        let mut result = Vec::with_capacity(n);
        while result.len() < n {
            let start = rng.random_range(0..self.rates.len());
            for i in 0..block_size {
                if result.len() >= n {
                    break;
                }
                // Circular wrap for blocks that extend past the end
                let idx = (start + i) % self.rates.len();
                result.push(self.rates[idx]);
            }
        }
        Some(result)
    }

    /// Compute basic statistics of the historical inflation.
    pub fn statistics(&self) -> Option<HistoricalStatistics> {
        if self.rates.is_empty() {
            return None;
        }
        let n = self.rates.len() as f64;
        let arithmetic_mean = self.rates.iter().sum::<f64>() / n;

        // Geometric mean: (product of (1+r))^(1/n) - 1
        let product: f64 = self.rates.iter().map(|r| 1.0 + r).product();
        let geometric_mean = product.powf(1.0 / n) - 1.0;

        let variance = self
            .rates
            .iter()
            .map(|r| (r - arithmetic_mean).powi(2))
            .sum::<f64>()
            / n;
        let std_dev = variance.sqrt();

        let min = self.rates.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = self.rates.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        Some(HistoricalStatistics {
            arithmetic_mean,
            geometric_mean,
            std_dev,
            min,
            max,
            years: self.rates.len(),
        })
    }

    // =========================================================================
    // Preset Historical Data Constructors
    // =========================================================================

    /// US CPI Inflation (All Urban Consumers) (1948-2025, 78 years)
    /// Source: FRED CPIAUCSL
    #[must_use]
    pub fn us_cpi() -> Self {
        Self::new("US CPI", 1948, historical_inflation::US_CPI_ANNUAL_RATES)
    }
}

/// Multi-asset historical returns for correlated bootstrap sampling.
///
/// When bootstrapping multiple assets, sampling the same historical year
/// across all assets preserves the cross-asset correlations from that period.
#[allow(dead_code)] // Available for future integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAssetHistory {
    /// Asset names in order (defines column indices)
    pub names: Vec<String>,
    /// Starting year of the aligned data series
    pub start_year: i16,
    /// Returns matrix: returns[`year_index`][asset_index]
    /// All rows must have the same length as `names`
    pub returns: Vec<Vec<f64>>,
}

#[allow(dead_code)] // Available for future integration
impl MultiAssetHistory {
    /// Create a new multi-asset history with validation.
    pub fn new(
        names: Vec<String>,
        start_year: i16,
        returns: Vec<Vec<f64>>,
    ) -> Result<Self, &'static str> {
        let n_assets = names.len();
        if n_assets == 0 {
            return Err("At least one asset required");
        }
        for row in &returns {
            if row.len() != n_assets {
                return Err("All return rows must have same length as names");
            }
        }
        Ok(Self {
            names,
            start_year,
            returns,
        })
    }

    /// Number of years of historical data.
    #[must_use]
    pub fn len(&self) -> usize {
        self.returns.len()
    }

    /// Returns true if no historical data is available.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.returns.is_empty()
    }

    /// Number of assets in the series.
    #[must_use]
    pub fn num_assets(&self) -> usize {
        self.names.len()
    }

    /// Sample a single year's returns for all assets (preserves correlation).
    pub fn sample_year<R: Rng + ?Sized>(&self, rng: &mut R) -> Option<Vec<f64>> {
        if self.returns.is_empty() {
            return None;
        }
        let idx = rng.random_range(0..self.returns.len());
        Some(self.returns[idx].clone())
    }

    /// Sample n years of returns for all assets (preserves within-year correlation).
    pub fn sample_years<R: Rng + ?Sized>(&self, rng: &mut R, n: usize) -> Option<Vec<Vec<f64>>> {
        if self.returns.is_empty() {
            return None;
        }
        Some(
            (0..n)
                .map(|_| self.returns[rng.random_range(0..self.returns.len())].clone())
                .collect(),
        )
    }

    /// Block bootstrap for multiple assets (preserves both autocorrelation and cross-correlation).
    pub fn block_bootstrap<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        n: usize,
        block_size: usize,
    ) -> Option<Vec<Vec<f64>>> {
        if self.returns.is_empty() || block_size == 0 {
            return None;
        }
        let mut result = Vec::with_capacity(n);
        while result.len() < n {
            let start = rng.random_range(0..self.returns.len());
            for i in 0..block_size {
                if result.len() >= n {
                    break;
                }
                let idx = (start + i) % self.returns.len();
                result.push(self.returns[idx].clone());
            }
        }
        Some(result)
    }
}

/// Historical annual returns for bootstrap sampling
pub mod historical_returns {
    /// US Large Cap Stocks (S&P 500 Total Return)
    /// Source: Robert Shiller, Yale University
    /// Annual returns 1927-2023 (97 years)
    pub const SP_500_ANNUAL_RETURNS: &[f64] = &[
        0.1071, 0.3490, 0.4533, -0.0803, -0.1985, -0.3873, -0.0936, 0.5318, -0.0791, 0.5231,
        0.3292, -0.2964, 0.1507, 0.0431, -0.0719, -0.0786, 0.1817, 0.2250, 0.1815, 0.3760, -0.1054,
        0.0309, 0.1032, 0.1677, 0.3240, 0.1990, 0.1397, 0.0222, 0.4375, 0.2781, 0.0684, -0.0571,
        0.3839, 0.0780, 0.0587, 0.1897, -0.0266, 0.2045, 0.1562, 0.1168, -0.0634, 0.1558, 0.1052,
        -0.0765, 0.0667, 0.1332, 0.1763, -0.1457, -0.2023, 0.3722, 0.1162, -0.0793, 0.1570, 0.1623,
        0.2494, -0.0613, 0.2736, 0.1987, 0.0727, 0.2477, 0.3002, -0.0181, 0.1715, 0.2260, -0.0102,
        0.3080, 0.0737, 0.1147, 0.0084, 0.3421, 0.2645, 0.2720, 0.3087, 0.1532, -0.0498, -0.1304,
        -0.1972, 0.2807, 0.0606, 0.1004, 0.1316, -0.0085, -0.3455, 0.3176, 0.1609, 0.0348, 0.1586,
        0.2504, 0.1332, -0.0327, 0.2052, 0.2449, -0.0461, 0.2756, 0.1710, 0.2212, -0.1180,
    ];

    /// US Small Cap Stocks (Market + SMB Factor)
    /// Source: Kenneth French Data Library, Dartmouth
    /// Annual returns 1927-2024 (98 years)
    pub const US_SMALL_CAP_ANNUAL_RETURNS: &[f64] = &[
        0.3036, 0.4285, -0.4551, -0.3426, -0.4070, -0.0361, 1.0140, 0.2907, 0.5564, 0.5019,
        -0.4884, 0.3753, 0.0842, -0.0646, -0.1436, 0.2149, 0.6202, 0.3947, 0.6470, -0.1002,
        -0.0354, -0.0737, 0.2399, 0.3080, 0.1543, 0.0672, -0.0045, 0.4811, 0.1899, 0.0745, -0.1267,
        0.5950, 0.1802, -0.0189, 0.2803, -0.1823, 0.1494, 0.1492, 0.3629, -0.0609, 0.7850, 0.3837,
        -0.2490, -0.1171, 0.2186, 0.0475, -0.4268, -0.2846, 0.5334, 0.4164, 0.1961, 0.2254, 0.4431,
        0.3849, 0.0346, 0.2967, 0.3648, -0.0410, 0.3257, 0.0687, -0.0919, 0.2347, 0.1612, -0.1996,
        0.5089, 0.1745, 0.1769, -0.0061, 0.2700, 0.1656, 0.2346, -0.0069, 0.3903, -0.1590, 0.0660,
        -0.1714, 0.5801, 0.1628, 0.0366, 0.1549, -0.0177, -0.3419, 0.3758, 0.3131, -0.0519, 0.1514,
        0.4282, 0.0411, -0.0367, 0.2010, 0.1702, -0.0819, 0.2414, 0.3751, 0.2013, -0.2691, 0.2316,
        0.1368,
    ];

    /// US 3-Month Treasury Bills
    /// Source: FRED (TB3MS)
    /// Annual returns 1934-2025 (92 years)
    pub const US_TBILLS_ANNUAL_RETURNS: &[f64] = &[
        0.0028, 0.0017, 0.0017, 0.0028, 0.0006, 0.0005, 0.0004, 0.0013, 0.0034, 0.0038, 0.0038,
        0.0038, 0.0038, 0.0060, 0.0104, 0.0112, 0.0120, 0.0152, 0.0172, 0.0189, 0.0094, 0.0172,
        0.0263, 0.0323, 0.0177, 0.0339, 0.0288, 0.0235, 0.0277, 0.0316, 0.0355, 0.0395, 0.0486,
        0.0431, 0.0534, 0.0667, 0.0639, 0.0433, 0.0407, 0.0703, 0.0783, 0.0577, 0.0497, 0.0527,
        0.0719, 0.1007, 0.1143, 0.1402, 0.1061, 0.0861, 0.0952, 0.0748, 0.0598, 0.0577, 0.0667,
        0.0811, 0.0749, 0.0537, 0.0343, 0.0300, 0.0425, 0.0549, 0.0501, 0.0506, 0.0478, 0.0464,
        0.0582, 0.0339, 0.0160, 0.0101, 0.0137, 0.0315, 0.0473, 0.0435, 0.0137, 0.0015, 0.0014,
        0.0005, 0.0009, 0.0006, 0.0003, 0.0005, 0.0032, 0.0093, 0.0194, 0.0206, 0.0037, 0.0004,
        0.0202, 0.0507, 0.0497, 0.0407,
    ];

    /// US Long-Term Government Bonds (estimated from yields)
    /// Source: Robert Shiller, Yale University (estimated)
    /// Annual returns 1927-2023 (97 years)
    pub const US_LONG_BOND_ANNUAL_RETURNS: &[f64] = &[
        0.0503, 0.0239, 0.0342, 0.0462, 0.0185, 0.0338, 0.0581, 0.0526, 0.0491, 0.0322, 0.0297,
        0.0388, 0.0388, 0.0389, 0.0135, -0.0006, 0.0238, 0.0283, 0.0357, 0.0285, 0.0126, 0.0199,
        0.0291, 0.0135, 0.0095, 0.0159, 0.0202, 0.0634, -0.0092, -0.0011, -0.0054, 0.0630, -0.0482,
        0.0607, 0.0599, 0.0338, 0.0349, 0.0253, 0.0342, -0.0084, 0.0372, 0.0049, -0.0255, 0.0125,
        0.1686, 0.0575, 0.0115, 0.0112, 0.0412, 0.1099, 0.0915, -0.0051, 0.0015, -0.0670, -0.0815,
        0.2118, 0.2817, 0.0044, 0.2696, 0.3415, 0.0207, 0.0469, 0.1163, 0.0809, 0.1408, 0.1464,
        0.1610, -0.0378, 0.1108, 0.0771, 0.0712, 0.1506, 0.0228, 0.0250, 0.1412, 0.0827, 0.0938,
        0.0194, 0.0415, 0.0028, 0.0609, 0.1233, 0.0695, 0.0360, 0.0664, 0.1065, -0.0258, 0.0083,
        0.0578, 0.0449, -0.0206, -0.0231, 0.0933, 0.1181, -0.0349, -0.1063, -0.0355,
    ];

    /// Developed Markets ex-US (Fama-French)
    /// Source: Kenneth French Data Library, Dartmouth
    /// Annual returns 1991-2024 (34 years)
    pub const INTL_DEVELOPED_ANNUAL_RETURNS: &[f64] = &[
        0.0945, -0.1537, 0.3019, 0.1000, 0.0842, 0.0586, -0.0049, 0.1680, 0.3664, -0.1644, -0.2111,
        -0.1197, 0.4391, 0.2269, 0.1680, 0.2552, 0.1302, -0.4280, 0.3354, 0.1189, -0.1260, 0.1729,
        0.2302, -0.0452, -0.0076, 0.0329, 0.2712, -0.1400, 0.2188, 0.1060, 0.1215, -0.1504, 0.1606,
        0.0359,
    ];

    /// Emerging Markets (Fama-French)
    /// Source: Kenneth French Data Library, Dartmouth
    /// Annual returns 1991-2024 (33 years)
    pub const EMERGING_MARKETS_ANNUAL_RETURNS: &[f64] = &[
        -0.6837, 0.9595, -0.0735, -0.0367, 0.0100, -0.2101, -0.2418, 0.9092, -0.2908, -0.0264,
        -0.0267, 0.6641, 0.2772, 0.3210, 0.3271, 0.4351, -0.4321, 0.6174, 0.3272, -0.1210, 0.1976,
        0.0432, 0.0213, -0.1138, 0.1656, 0.2911, -0.0875, 0.0745, 0.1037, 0.1414, -0.0707, 0.0995,
        -0.0312,
    ];

    /// US Real Estate Investment Trusts (via VNQ)
    /// Source: Yahoo Finance
    /// Annual returns 2005-2026 (22 years)
    pub const REITS_ANNUAL_RETURNS: &[f64] = &[
        0.1194, 0.3528, -0.1652, -0.3698, 0.3014, 0.2839, 0.0864, 0.1763, 0.0230, 0.3040, 0.0243,
        0.0857, 0.0490, -0.0603, 0.2891, -0.0461, 0.4054, -0.2625, 0.1185, 0.0481, 0.0325, 0.0246,
    ];

    /// Gold (via GC=F futures)
    /// Source: Yahoo Finance
    /// Annual returns 2001-2026 (26 years)
    pub const GOLD_ANNUAL_RETURNS: &[f64] = &[
        0.0246, 0.2472, 0.1959, 0.0524, 0.1819, 0.2284, 0.3144, 0.0583, 0.2395, 0.2976, 0.1018,
        0.0696, -0.2824, -0.0150, -0.1044, 0.0846, 0.1359, -0.0214, 0.1887, 0.2459, -0.0347,
        -0.0043, 0.1334, 0.2748, 0.6452, 0.1672,
    ];

    /// US Investment Grade Bonds (Bloomberg Aggregate via AGG)
    /// Source: Yahoo Finance
    /// Annual returns 2004-2026 (23 years)
    pub const US_AGG_BOND_ANNUAL_RETURNS: &[f64] = &[
        0.0378, 0.0226, 0.0390, 0.0659, 0.0790, 0.0297, 0.0636, 0.0770, 0.0375, -0.0198, 0.0600,
        0.0048, 0.0241, 0.0355, 0.0034, 0.0846, 0.0748, -0.0177, -0.1302, 0.0566, 0.0131, 0.0719,
        0.0039,
    ];

    /// US Investment Grade Corporate Bonds (via LQD)
    /// Source: Yahoo Finance
    /// Annual returns 2003-2026 (24 years)
    pub const US_CORPORATE_BOND_ANNUAL_RETURNS: &[f64] = &[
        0.0911, 0.0571, 0.0116, 0.0422, 0.0373, 0.0242, 0.0845, 0.0932, 0.0974, 0.1024, -0.0200,
        0.0821, -0.0126, 0.0620, 0.0705, -0.0379, 0.1737, 0.1097, -0.0184, -0.1792, 0.0940, 0.0086,
        0.0790, 0.0069,
    ];

    /// US Treasury Inflation-Protected Securities (via TIP)
    /// Source: Yahoo Finance
    /// Annual returns 2004-2026 (23 years)
    pub const TIPS_ANNUAL_RETURNS: &[f64] = &[
        0.0828, 0.0249, 0.0028, 0.1193, 0.0005, 0.0893, 0.0613, 0.1330, 0.0640, -0.0849, 0.0359,
        -0.0175, 0.0468, 0.0292, -0.0143, 0.0835, 0.1084, 0.0568, -0.1226, 0.0380, 0.0165, 0.0676,
        0.0042,
    ];
}

/// Historical annual inflation rates for bootstrap sampling
pub mod historical_inflation {
    /// US CPI Inflation (All Urban Consumers)
    /// Source: FRED (CPIAUCSL)
    /// Annual rates 1948-2025 (78 years)
    /// Arithmetic mean: 3.47%, Geometric mean: 3.43%, Std dev: 2.79%
    pub const US_CPI_ANNUAL_RATES: &[f64] = &[
        0.0273, -0.0183, 0.0580, 0.0596, 0.0091, 0.0060, -0.0037, 0.0037, 0.0283, 0.0304, 0.0176,
        0.0152, 0.0136, 0.0067, 0.0123, 0.0165, 0.0120, 0.0192, 0.0336, 0.0328, 0.0471, 0.0590,
        0.0557, 0.0327, 0.0341, 0.0894, 0.1210, 0.0713, 0.0504, 0.0668, 0.0899, 0.1325, 0.1235,
        0.0891, 0.0383, 0.0379, 0.0404, 0.0379, 0.0119, 0.0433, 0.0441, 0.0464, 0.0625, 0.0298,
        0.0297, 0.0281, 0.0260, 0.0253, 0.0338, 0.0170, 0.0161, 0.0268, 0.0344, 0.0160, 0.0248,
        0.0204, 0.0334, 0.0334, 0.0252, 0.0411, -0.0002, 0.0281, 0.0144, 0.0306, 0.0176, 0.0151,
        0.0065, 0.0064, 0.0205, 0.0213, 0.0200, 0.0232, 0.0132, 0.0716, 0.0641, 0.0332, 0.0287,
        0.0265,
    ];
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

        let market = Market::new(&inflation, returns, assets);

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
        let days = f64::from((eval_date - start_date).get_days());
        let expected_rate = (1.10_f64).powf(days / 365.0) - 1.0;
        let expected_val = 1000.0 * (1.0 + expected_rate);

        let val = market
            .get_asset_value(start_date, eval_date, asset_id)
            .unwrap();
        assert!((val - expected_val).abs() < 1e-6);

        // Before start date
        let val = market.get_asset_value(start_date, date(2023, 12, 31), asset_id);
        assert!(val.is_err());
    }

    #[test]
    fn test_student_t_sampling() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        // Test StudentT sampling works
        let profile = ReturnProfile::StudentT {
            mean: 0.10,
            scale: 0.15,
            df: 5.0,
        };

        // Sample many times and check basic statistics
        let samples: Vec<f64> = (0..10000)
            .map(|_| profile.sample(&mut rng).unwrap())
            .collect();

        let mean: f64 = samples.iter().sum::<f64>() / samples.len() as f64;
        let variance: f64 =
            samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / samples.len() as f64;
        let std_dev = variance.sqrt();

        // Mean should be close to 0.10 (within 2% absolute)
        assert!(
            (mean - 0.10).abs() < 0.02,
            "Mean {mean} too far from expected 0.10"
        );

        // For Student's t with df=5, variance = scale^2 * df/(df-2) = 0.15^2 * 5/3 = 0.0375
        // Expected std_dev = sqrt(0.0375) ≈ 0.1936
        // Allow 20% tolerance for sampling variance
        let expected_std = 0.15 * (5.0_f64 / 3.0).sqrt();
        assert!(
            (std_dev - expected_std).abs() < expected_std * 0.20,
            "Std dev {std_dev} too far from expected {expected_std}"
        );
    }

    #[test]
    fn test_student_t_preset_constants() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(123);

        // Test that the preset constants can be sampled
        let sample = ReturnProfile::SP_500_HISTORICAL_STUDENT_T
            .sample(&mut rng)
            .unwrap();
        assert!(sample.is_finite());

        let sample = ReturnProfile::US_SMALL_CAP_HISTORICAL_STUDENT_T
            .sample(&mut rng)
            .unwrap();
        assert!(sample.is_finite());

        let sample = ReturnProfile::EMERGING_MARKETS_HISTORICAL_STUDENT_T
            .sample(&mut rng)
            .unwrap();
        assert!(sample.is_finite());
    }

    #[test]
    fn test_regime_switching_stateless_sample() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let profile = ReturnProfile::sp500_regime_switching_normal();

        // Sample many times and verify we get a mix of bull and bear returns
        let samples: Vec<f64> = (0..1000)
            .map(|_| profile.sample(&mut rng).unwrap())
            .collect();

        // Should have some positive (bull) and negative (bear) returns
        let positive_count = samples.iter().filter(|&&x| x > 0.0).count();
        let negative_count = samples.iter().filter(|&&x| x < 0.0).count();

        // With steady-state ~80% bull, ~20% bear, we expect mostly positive
        // but with bear's higher volatility some samples can be positive too
        assert!(positive_count > 500, "Expected majority positive returns");
        assert!(negative_count > 50, "Expected some negative returns");
    }

    #[test]
    fn test_regime_switching_sequence_maintains_state() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let profile = ReturnProfile::sp500_regime_switching_normal();

        // Generate a sequence
        let returns = profile.sample_sequence(&mut rng, 100).unwrap();
        assert_eq!(returns.len(), 100);

        // Check that returns cluster (characteristic of regime switching)
        // Count "runs" - consecutive returns of same sign
        let mut runs = 1;
        for i in 1..returns.len() {
            if (returns[i] > 0.0) != (returns[i - 1] > 0.0) {
                runs += 1;
            }
        }

        // With regime switching, runs should be fewer than with independent samples
        // Independent samples would have ~50 runs on average (50% chance of sign change)
        // Regime switching should have fewer due to persistence
        // This is a probabilistic test, but with seed 42 it should be stable
        assert!(
            runs < 45,
            "Expected fewer runs ({runs}) due to regime persistence"
        );
    }

    #[test]
    fn test_regime_switching_with_student_t() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(123);

        let profile = ReturnProfile::sp500_regime_switching_student_t();

        // Should be able to sample
        let sample = profile.sample(&mut rng).unwrap();
        assert!(sample.is_finite());

        // Should be able to generate sequence
        let returns = profile.sample_sequence(&mut rng, 50).unwrap();
        assert_eq!(returns.len(), 50);
        assert!(returns.iter().all(|r| r.is_finite()));
    }

    #[test]
    fn test_regime_switching_custom() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(456);

        // Create a custom regime switching profile
        let profile = ReturnProfile::regime_switching(
            ReturnProfile::Fixed(0.10),  // Bull: always 10%
            ReturnProfile::Fixed(-0.20), // Bear: always -20%
            0.25,                        // 25% chance bull->bear
            0.50,                        // 50% chance bear->bull
        );

        let returns = profile.sample_sequence(&mut rng, 20).unwrap();

        // All returns should be either 0.10 or -0.20
        for r in &returns {
            assert!(
                (*r - 0.10).abs() < 1e-10 || (*r - (-0.20)).abs() < 1e-10,
                "Unexpected return value: {r}"
            );
        }
    }

    #[test]
    fn test_regime_switching_in_market_from_profiles() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(789);

        let rp_id = ReturnProfileId(1);
        let asset_id = AssetId(1);

        let mut return_profiles = HashMap::new();
        return_profiles.insert(rp_id, ReturnProfile::sp500_regime_switching_normal());

        let assets = FxHashMap::from_iter([(asset_id, (1000.0, rp_id))]);

        let market = Market::from_profiles(
            &mut rng,
            30,
            &InflationProfile::Fixed(0.02),
            &return_profiles,
            &assets,
        )
        .unwrap();

        // Should be able to get asset values
        let start = date(2024, 1, 1);
        let value = market.get_asset_value(start, date(2025, 1, 1), asset_id);
        assert!(value.is_ok());
        assert!(value.unwrap().is_finite());
    }

    // =========================================================================
    // Bootstrap Tests
    // =========================================================================

    #[test]
    fn test_historical_returns_statistics() {
        let history = HistoricalReturns::sp500();
        assert_eq!(history.len(), 97);
        assert!(!history.is_empty());
        assert_eq!(history.start_year, 1927);

        let stats = history.statistics().unwrap();
        // S&P 500 historical stats should match the preset constants
        assert!(
            (stats.arithmetic_mean - 0.1147).abs() < 0.01,
            "Arithmetic mean {} should be ~0.1147",
            stats.arithmetic_mean
        );
        assert!(
            (stats.geometric_mean - 0.099).abs() < 0.01,
            "Geometric mean {} should be ~0.099",
            stats.geometric_mean
        );
        assert!(
            (stats.std_dev - 0.18).abs() < 0.02,
            "Std dev {} should be ~0.18",
            stats.std_dev
        );
    }

    #[test]
    fn test_historical_returns_sampling() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let history = HistoricalReturns::sp500();

        // Single sample should be from historical data
        let sample = history.sample(&mut rng).unwrap();
        assert!(
            history.returns.contains(&sample),
            "Sample {} should be from historical data",
            sample
        );

        // Multi-year sample
        let samples = history.sample_years(&mut rng, 30).unwrap();
        assert_eq!(samples.len(), 30);
        for s in &samples {
            assert!(
                history.returns.contains(s),
                "Sample {} should be from historical data",
                s
            );
        }
    }

    #[test]
    fn test_historical_returns_block_bootstrap() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let history = HistoricalReturns::sp500();
        let returns = history.block_bootstrap(&mut rng, 30, 5).unwrap();
        assert_eq!(returns.len(), 30);

        // All samples should be from historical data
        for r in &returns {
            assert!(
                history.returns.contains(r),
                "Sample {} should be from historical data",
                r
            );
        }

        // Block bootstrap should show some consecutive sequences
        // (This is probabilistic but with seed 42 should be stable)
    }

    #[test]
    fn test_bootstrap_profile_sample() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let profile = ReturnProfile::sp500_bootstrap();
        let history = HistoricalReturns::sp500();

        // Sample should be from historical data
        let sample = profile.sample(&mut rng).unwrap();
        assert!(
            history.returns.contains(&sample),
            "Sample {} should be from historical data",
            sample
        );

        // Generate sequence
        let returns = profile.sample_sequence(&mut rng, 50).unwrap();
        assert_eq!(returns.len(), 50);
        for r in &returns {
            assert!(
                history.returns.contains(r),
                "Sample {} should be from historical data",
                r
            );
        }
    }

    #[test]
    fn test_bootstrap_profile_block() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let profile = ReturnProfile::sp500_bootstrap_block5();
        let history = HistoricalReturns::sp500();

        // Generate sequence with block bootstrap
        let returns = profile.sample_sequence(&mut rng, 30).unwrap();
        assert_eq!(returns.len(), 30);
        for r in &returns {
            assert!(
                history.returns.contains(r),
                "Sample {} should be from historical data",
                r
            );
        }
    }

    #[test]
    fn test_bootstrap_all_presets() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(123);

        // Test all preset bootstrap profiles work
        let presets = vec![
            ReturnProfile::sp500_bootstrap(),
            ReturnProfile::sp500_bootstrap_block5(),
            ReturnProfile::us_small_cap_bootstrap(),
            ReturnProfile::us_tbills_bootstrap(),
            ReturnProfile::us_long_bonds_bootstrap(),
            ReturnProfile::intl_developed_bootstrap(),
            ReturnProfile::emerging_markets_bootstrap(),
            ReturnProfile::reits_bootstrap(),
            ReturnProfile::gold_bootstrap(),
            ReturnProfile::us_agg_bonds_bootstrap(),
            ReturnProfile::us_corporate_bonds_bootstrap(),
            ReturnProfile::tips_bootstrap(),
        ];

        for profile in presets {
            let sample = profile.sample(&mut rng).unwrap();
            assert!(sample.is_finite(), "Bootstrap sample should be finite");

            let returns = profile.sample_sequence(&mut rng, 10).unwrap();
            assert_eq!(returns.len(), 10);
            assert!(
                returns.iter().all(|r| r.is_finite()),
                "All returns should be finite"
            );
        }
    }

    #[test]
    fn test_bootstrap_in_market_from_profiles() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(456);

        let rp_id = ReturnProfileId(1);
        let asset_id = AssetId(1);

        let mut return_profiles = HashMap::new();
        return_profiles.insert(rp_id, ReturnProfile::sp500_bootstrap());

        let assets = FxHashMap::from_iter([(asset_id, (1000.0, rp_id))]);

        let market = Market::from_profiles(
            &mut rng,
            30,
            &InflationProfile::Fixed(0.02),
            &return_profiles,
            &assets,
        )
        .unwrap();

        // Should be able to get asset values
        let start = date(2024, 1, 1);
        let value = market.get_asset_value(start, date(2025, 1, 1), asset_id);
        assert!(value.is_ok());
        assert!(value.unwrap().is_finite());
    }

    #[test]
    fn test_bootstrap_custom() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(789);

        // Create custom historical data
        let history = HistoricalReturns::new("Custom", 2020, vec![0.10, 0.20, -0.05, 0.15]);

        let profile = ReturnProfile::bootstrap(history.clone(), Some(2));

        let sample = profile.sample(&mut rng).unwrap();
        assert!(
            history.returns.contains(&sample),
            "Sample {} should be from custom data",
            sample
        );

        let returns = profile.sample_sequence(&mut rng, 20).unwrap();
        assert_eq!(returns.len(), 20);
        for r in &returns {
            assert!(
                history.returns.contains(r),
                "Sample {} should be from custom data",
                r
            );
        }
    }

    #[test]
    fn test_multi_asset_history() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        // Create a simple 3-year, 2-asset history
        let history = MultiAssetHistory::new(
            vec!["Stocks".to_string(), "Bonds".to_string()],
            2020,
            vec![
                vec![0.10, 0.05],  // 2020
                vec![0.20, -0.02], // 2021
                vec![-0.15, 0.08], // 2022
            ],
        )
        .unwrap();

        assert_eq!(history.len(), 3);
        assert_eq!(history.num_assets(), 2);

        // Sample a year - should get both assets from same year
        let year = history.sample_year(&mut rng).unwrap();
        assert_eq!(year.len(), 2);
        // Verify it's one of the actual years
        assert!(
            history.returns.contains(&year),
            "Sampled year {:?} should be from history",
            year
        );

        // Sample multiple years
        let years = history.sample_years(&mut rng, 10).unwrap();
        assert_eq!(years.len(), 10);
        for y in &years {
            assert_eq!(y.len(), 2);
            assert!(
                history.returns.contains(y),
                "Sampled year {:?} should be from history",
                y
            );
        }
    }

    #[test]
    fn test_multi_asset_block_bootstrap() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let history = MultiAssetHistory::new(
            vec!["A".to_string(), "B".to_string()],
            2020,
            vec![
                vec![0.10, 0.05],
                vec![0.20, -0.02],
                vec![-0.15, 0.08],
                vec![0.05, 0.03],
            ],
        )
        .unwrap();

        let samples = history.block_bootstrap(&mut rng, 10, 2).unwrap();
        assert_eq!(samples.len(), 10);
        for s in &samples {
            assert_eq!(s.len(), 2);
            assert!(
                history.returns.contains(s),
                "Block bootstrap sample {:?} should be from history",
                s
            );
        }
    }

    #[test]
    fn test_empty_historical_returns_error() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let profile = ReturnProfile::Bootstrap {
            history: HistoricalReturns::new("Empty", 2020, vec![]),
            block_size: None,
        };

        // Should return error for empty data
        let result = profile.sample(&mut rng);
        assert!(result.is_err());

        let result = profile.sample_sequence(&mut rng, 10);
        assert!(result.is_err());
    }

    /// Test that Market::from_profiles produces deterministic results regardless of
    /// HashMap insertion order. This guards against non-determinism from HashMap
    /// iteration order which varies across process invocations.
    #[test]
    fn test_market_from_profiles_determinism() {
        use rand::SeedableRng;

        let rp1 = ReturnProfileId(1);
        let rp2 = ReturnProfileId(2);
        let rp3 = ReturnProfileId(3);
        let asset1 = AssetId(1);
        let asset2 = AssetId(2);
        let asset3 = AssetId(3);

        // Create profiles - use bootstrap profiles which consume significant RNG
        let profile1 = ReturnProfile::sp500_bootstrap();
        let profile2 = ReturnProfile::us_small_cap_bootstrap();
        let profile3 = ReturnProfile::intl_developed_bootstrap();

        // Build HashMap in order 1, 2, 3
        let mut profiles_order_a = HashMap::new();
        profiles_order_a.insert(rp1, profile1.clone());
        profiles_order_a.insert(rp2, profile2.clone());
        profiles_order_a.insert(rp3, profile3.clone());

        // Build HashMap in order 3, 1, 2 (different insertion order)
        let mut profiles_order_b = HashMap::new();
        profiles_order_b.insert(rp3, profile3.clone());
        profiles_order_b.insert(rp1, profile1.clone());
        profiles_order_b.insert(rp2, profile2.clone());

        // Build HashMap in order 2, 3, 1 (yet another order)
        let mut profiles_order_c = HashMap::new();
        profiles_order_c.insert(rp2, profile2);
        profiles_order_c.insert(rp3, profile3);
        profiles_order_c.insert(rp1, profile1);

        let assets = FxHashMap::from_iter([
            (asset1, (1000.0, rp1)),
            (asset2, (2000.0, rp2)),
            (asset3, (3000.0, rp3)),
        ]);

        let inflation = InflationProfile::us_historical_bootstrap(Some(5));

        // Create markets with same seed but different HashMap insertion orders
        let mut rng_a = rand::rngs::StdRng::seed_from_u64(12345);
        let market_a =
            Market::from_profiles(&mut rng_a, 30, &inflation, &profiles_order_a, &assets)
                .expect("Market A should succeed");

        let mut rng_b = rand::rngs::StdRng::seed_from_u64(12345);
        let market_b =
            Market::from_profiles(&mut rng_b, 30, &inflation, &profiles_order_b, &assets)
                .expect("Market B should succeed");

        let mut rng_c = rand::rngs::StdRng::seed_from_u64(12345);
        let market_c =
            Market::from_profiles(&mut rng_c, 30, &inflation, &profiles_order_c, &assets)
                .expect("Market C should succeed");

        // Verify all markets produce identical results
        let start = date(2024, 1, 1);
        for year in 1..=30 {
            let eval = date(2024 + year, 1, 1);

            // Check asset values are identical
            for &asset_id in &[asset1, asset2, asset3] {
                let val_a = market_a.get_asset_value(start, eval, asset_id).unwrap();
                let val_b = market_b.get_asset_value(start, eval, asset_id).unwrap();
                let val_c = market_c.get_asset_value(start, eval, asset_id).unwrap();

                assert!(
                    (val_a - val_b).abs() < 1e-10,
                    "Year {}: Asset {:?} differs between order A ({}) and B ({})",
                    year,
                    asset_id,
                    val_a,
                    val_b
                );
                assert!(
                    (val_a - val_c).abs() < 1e-10,
                    "Year {}: Asset {:?} differs between order A ({}) and C ({})",
                    year,
                    asset_id,
                    val_a,
                    val_c
                );
            }

            // Check inflation values are identical
            let inf_a = market_a
                .get_inflation_adjusted_value(start, eval, 1000.0)
                .unwrap();
            let inf_b = market_b
                .get_inflation_adjusted_value(start, eval, 1000.0)
                .unwrap();
            let inf_c = market_c
                .get_inflation_adjusted_value(start, eval, 1000.0)
                .unwrap();

            assert!(
                (inf_a - inf_b).abs() < 1e-10,
                "Year {}: Inflation differs between order A ({}) and B ({})",
                year,
                inf_a,
                inf_b
            );
            assert!(
                (inf_a - inf_c).abs() < 1e-10,
                "Year {}: Inflation differs between order A ({}) and C ({})",
                year,
                inf_a,
                inf_c
            );
        }
    }
}
