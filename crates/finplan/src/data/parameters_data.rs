use finplan_core::model::{InflationProfile, TaxConfig};
use serde::{Deserialize, Serialize};

/// Simulation parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParametersData {
    /// Birth date for age-based calculations (YYYY-MM-DD format)
    pub birth_date: String,

    /// Simulation start date (YYYY-MM-DD format)
    pub start_date: String,

    /// Number of years to simulate
    #[serde(default = "default_duration")]
    pub duration_years: usize,

    /// Inflation model
    #[serde(default)]
    pub inflation: InflationData,

    /// Tax configuration
    #[serde(default)]
    pub tax_config: TaxConfigData,
}

fn default_duration() -> usize {
    30
}

/// Inflation profile data
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type")]
pub enum InflationData {
    #[default]
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
    /// Use historical US inflation
    USHistorical {
        #[serde(default)]
        distribution: DistributionType,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DistributionType {
    Fixed,
    #[default]
    Normal,
    LogNormal,
}

impl InflationData {
    pub fn to_inflation_profile(&self) -> InflationProfile {
        match self {
            InflationData::None => InflationProfile::None,
            InflationData::Fixed { rate } => InflationProfile::Fixed(*rate),
            InflationData::Normal { mean, std_dev } => InflationProfile::Normal {
                mean: *mean,
                std_dev: *std_dev,
            },
            InflationData::LogNormal { mean, std_dev } => InflationProfile::LogNormal {
                mean: *mean,
                std_dev: *std_dev,
            },
            InflationData::USHistorical { distribution } => match distribution {
                DistributionType::Fixed => InflationProfile::US_HISTORICAL_FIXED,
                DistributionType::Normal => InflationProfile::US_HISTORICAL_NORMAL,
                DistributionType::LogNormal => InflationProfile::US_HISTORICAL_LOG_NORMAL,
            },
        }
    }
}

/// Tax configuration data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxConfigData {
    /// State income tax rate (e.g., 0.05 for 5%)
    #[serde(default = "default_state_rate")]
    pub state_rate: f64,

    /// Long-term capital gains rate (e.g., 0.15 for 15%)
    #[serde(default = "default_cap_gains_rate")]
    pub capital_gains_rate: f64,

    /// Federal tax brackets preset
    #[serde(default)]
    pub federal_brackets: FederalBracketsPreset,
}

fn default_state_rate() -> f64 {
    0.05
}

fn default_cap_gains_rate() -> f64 {
    0.15
}

impl Default for TaxConfigData {
    fn default() -> Self {
        Self {
            state_rate: default_state_rate(),
            capital_gains_rate: default_cap_gains_rate(),
            federal_brackets: FederalBracketsPreset::default(),
        }
    }
}

/// Federal tax brackets preset
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FederalBracketsPreset {
    /// 2024 Single filer brackets
    #[default]
    Single2024,
    /// 2024 Married filing jointly brackets
    MarriedJoint2024,
    /// Custom brackets
    Custom {
        brackets: Vec<TaxBracketData>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxBracketData {
    pub threshold: f64,
    pub rate: f64,
}

impl TaxConfigData {
    pub fn to_tax_config(&self) -> TaxConfig {
        let federal_brackets = match &self.federal_brackets {
            FederalBracketsPreset::Single2024 => TaxConfig::default().federal_brackets,
            FederalBracketsPreset::MarriedJoint2024 => {
                // 2024 married filing jointly brackets
                vec![
                    finplan_core::model::TaxBracket {
                        threshold: 0.0,
                        rate: 0.10,
                    },
                    finplan_core::model::TaxBracket {
                        threshold: 23_200.0,
                        rate: 0.12,
                    },
                    finplan_core::model::TaxBracket {
                        threshold: 94_300.0,
                        rate: 0.22,
                    },
                    finplan_core::model::TaxBracket {
                        threshold: 201_050.0,
                        rate: 0.24,
                    },
                    finplan_core::model::TaxBracket {
                        threshold: 383_900.0,
                        rate: 0.32,
                    },
                    finplan_core::model::TaxBracket {
                        threshold: 487_450.0,
                        rate: 0.35,
                    },
                    finplan_core::model::TaxBracket {
                        threshold: 731_200.0,
                        rate: 0.37,
                    },
                ]
            }
            FederalBracketsPreset::Custom { brackets } => brackets
                .iter()
                .map(|b| finplan_core::model::TaxBracket {
                    threshold: b.threshold,
                    rate: b.rate,
                })
                .collect(),
        };

        TaxConfig {
            federal_brackets,
            state_rate: self.state_rate,
            capital_gains_rate: self.capital_gains_rate,
            early_withdrawal_penalty_rate: 0.10,
        }
    }
}

impl Default for ParametersData {
    fn default() -> Self {
        Self {
            birth_date: "1985-01-01".to_string(),
            start_date: "2025-01-01".to_string(),
            duration_years: 30,
            inflation: InflationData::default(),
            tax_config: TaxConfigData::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameters_serialization() {
        let params = ParametersData {
            birth_date: "1985-06-15".to_string(),
            start_date: "2025-01-01".to_string(),
            duration_years: 40,
            inflation: InflationData::USHistorical {
                distribution: DistributionType::Normal,
            },
            tax_config: TaxConfigData {
                state_rate: 0.05,
                capital_gains_rate: 0.15,
                federal_brackets: FederalBracketsPreset::Single2024,
            },
        };

        let yaml = serde_saphyr::to_string(&params).unwrap();
        println!("Parameters YAML:\n{}", yaml);

        let deserialized: ParametersData = serde_saphyr::from_str(&yaml).unwrap();
        assert_eq!(deserialized.duration_years, 40);
    }
}
