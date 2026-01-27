use finplan_core::model::{HistoricalReturns, ReturnProfile};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ReturnProfileTag(pub String);

/// YAML-friendly representation of a return profile
/// Uses explicit field names to avoid serde_saphyr issues with tagged newtype variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfileData {
    None,
    Fixed {
        rate: f64,
    },
    Normal {
        mean: f64,
        std_dev: f64,
    },
    LogNormal {
        mean: f64,
        std_dev: f64,
    },
    /// Student's t distribution for fat-tailed returns
    StudentT {
        mean: f64,
        scale: f64,
        df: f64,
    },
    /// Regime-switching model with bull/bear market states (Normal distributions)
    RegimeSwitching {
        bull_mean: f64,
        bull_std_dev: f64,
        bear_mean: f64,
        bear_std_dev: f64,
        bull_to_bear_prob: f64,
        bear_to_bull_prob: f64,
    },
    /// Bootstrap from historical data
    Bootstrap {
        /// Preset key identifying the historical data (e.g., "sp500", "us_small_cap")
        preset: String,
    },
}

impl ReturnProfileData {
    pub fn to_return_profile(&self) -> ReturnProfile {
        self.to_return_profile_with_block_size(None)
    }

    /// Convert to ReturnProfile with optional block size for Bootstrap profiles
    pub fn to_return_profile_with_block_size(&self, block_size: Option<usize>) -> ReturnProfile {
        match self {
            ReturnProfileData::None => ReturnProfile::None,
            ReturnProfileData::Fixed { rate } => ReturnProfile::Fixed(*rate),
            ReturnProfileData::Normal { mean, std_dev } => ReturnProfile::Normal {
                mean: *mean,
                std_dev: *std_dev,
            },
            ReturnProfileData::LogNormal { mean, std_dev } => ReturnProfile::LogNormal {
                mean: *mean,
                std_dev: *std_dev,
            },
            ReturnProfileData::StudentT { mean, scale, df } => ReturnProfile::StudentT {
                mean: *mean,
                scale: *scale,
                df: *df,
            },
            ReturnProfileData::RegimeSwitching {
                bull_mean,
                bull_std_dev,
                bear_mean,
                bear_std_dev,
                bull_to_bear_prob,
                bear_to_bull_prob,
            } => ReturnProfile::RegimeSwitching {
                bull: Box::new(ReturnProfile::Normal {
                    mean: *bull_mean,
                    std_dev: *bull_std_dev,
                }),
                bear: Box::new(ReturnProfile::Normal {
                    mean: *bear_mean,
                    std_dev: *bear_std_dev,
                }),
                bull_to_bear_prob: *bull_to_bear_prob,
                bear_to_bull_prob: *bear_to_bull_prob,
            },
            ReturnProfileData::Bootstrap { preset } => {
                let history = Self::get_historical_returns(preset);
                ReturnProfile::Bootstrap {
                    history,
                    block_size,
                }
            }
        }
    }

    /// Get HistoricalReturns for a preset key
    pub fn get_historical_returns(preset: &str) -> HistoricalReturns {
        match preset {
            "sp500" => HistoricalReturns::sp500(),
            "us_small_cap" => HistoricalReturns::us_small_cap(),
            "us_tbills" => HistoricalReturns::us_tbills(),
            "us_long_bonds" => HistoricalReturns::us_long_bonds(),
            "intl_developed" => HistoricalReturns::intl_developed(),
            "emerging_markets" => HistoricalReturns::emerging_markets(),
            "reits" => HistoricalReturns::reits(),
            "gold" => HistoricalReturns::gold(),
            "us_agg_bonds" => HistoricalReturns::us_agg_bonds(),
            "us_corporate_bonds" => HistoricalReturns::us_corporate_bonds(),
            "tips" => HistoricalReturns::tips(),
            _ => HistoricalReturns::sp500(), // Default fallback
        }
    }
}

impl From<&ReturnProfile> for ReturnProfileData {
    fn from(profile: &ReturnProfile) -> Self {
        match profile {
            ReturnProfile::None => ReturnProfileData::None,
            ReturnProfile::Fixed(rate) => ReturnProfileData::Fixed { rate: *rate },
            ReturnProfile::Normal { mean, std_dev } => ReturnProfileData::Normal {
                mean: *mean,
                std_dev: *std_dev,
            },
            ReturnProfile::LogNormal { mean, std_dev } => ReturnProfileData::LogNormal {
                mean: *mean,
                std_dev: *std_dev,
            },
            ReturnProfile::StudentT { mean, scale, df } => ReturnProfileData::StudentT {
                mean: *mean,
                scale: *scale,
                df: *df,
            },
            ReturnProfile::RegimeSwitching {
                bull,
                bear,
                bull_to_bear_prob,
                bear_to_bull_prob,
            } => {
                // Extract mean/std_dev from nested profiles
                // Falls back to defaults if not Normal distribution
                let (bull_mean, bull_std_dev) = match bull.as_ref() {
                    ReturnProfile::Normal { mean, std_dev } => (*mean, *std_dev),
                    ReturnProfile::StudentT { mean, scale, .. } => (*mean, *scale),
                    _ => (0.15, 0.12), // Default bull market params
                };
                let (bear_mean, bear_std_dev) = match bear.as_ref() {
                    ReturnProfile::Normal { mean, std_dev } => (*mean, *std_dev),
                    ReturnProfile::StudentT { mean, scale, .. } => (*mean, *scale),
                    _ => (-0.08, 0.25), // Default bear market params
                };
                ReturnProfileData::RegimeSwitching {
                    bull_mean,
                    bull_std_dev,
                    bear_mean,
                    bear_std_dev,
                    bull_to_bear_prob: *bull_to_bear_prob,
                    bear_to_bull_prob: *bear_to_bull_prob,
                }
            }
            ReturnProfile::Bootstrap { history, .. } => {
                // Map back to preset key based on name
                let preset = Self::preset_key_from_name(&history.name).unwrap_or("sp500");
                ReturnProfileData::Bootstrap {
                    preset: preset.to_string(),
                }
            }
        }
    }
}

impl ReturnProfileData {
    /// Map historical returns name back to preset key
    fn preset_key_from_name(name: &str) -> Option<&'static str> {
        match name {
            "S&P 500" => Some("sp500"),
            "US Small Cap" => Some("us_small_cap"),
            "US T-Bills" => Some("us_tbills"),
            "US Long-Term Bonds" => Some("us_long_bonds"),
            "International Developed" => Some("intl_developed"),
            "Emerging Markets" => Some("emerging_markets"),
            "REITs" => Some("reits"),
            "Gold" => Some("gold"),
            "US Aggregate Bonds" => Some("us_agg_bonds"),
            "US Corporate Bonds" => Some("us_corporate_bonds"),
            "TIPS" => Some("tips"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileData {
    pub name: ReturnProfileTag,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(flatten)]
    pub profile: ReturnProfileData,
}
