use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{
    events_data::EventData,
    parameters_data::ParametersData,
    portfolio_data::{AssetTag, PortfolioData},
    profiles_data::{ProfileData, ReturnProfileTag},
};

/// Top-level application data containing all simulations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppData {
    /// The currently active scenario name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_scenario: Option<String>,
    pub simulations: HashMap<String, SimulationData>,
}

/// A complete simulation configuration in human-readable format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationData {
    /// Portfolio with accounts
    pub portfolios: PortfolioData,

    /// Return profiles (named) - used in Parametric mode
    #[serde(default)]
    pub profiles: Vec<ProfileData>,

    /// Asset to return profile mappings - used in Parametric mode
    #[serde(default)]
    pub assets: HashMap<AssetTag, ReturnProfileTag>,

    /// Asset to return profile mappings for Historical mode
    /// Maps asset tickers to Bootstrap profile names (e.g., "S&P 500", "US Small Cap")
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub historical_assets: HashMap<AssetTag, ReturnProfileTag>,

    /// Events (income, expenses, transfers, etc.)
    #[serde(default)]
    pub events: Vec<EventData>,

    /// Simulation parameters (dates, duration, inflation, taxes)
    #[serde(default)]
    pub parameters: ParametersData,
}

impl Default for SimulationData {
    fn default() -> Self {
        Self {
            portfolios: PortfolioData {
                name: "My Portfolio".to_string(),
                description: None,
                accounts: vec![],
            },
            profiles: vec![],
            assets: HashMap::new(),
            historical_assets: HashMap::new(),
            events: vec![],
            parameters: ParametersData::default(),
        }
    }
}

impl AppData {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from YAML string
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_saphyr::Error> {
        serde_saphyr::from_str(yaml)
    }

    /// Save to YAML string
    pub fn to_yaml(&self) -> Result<String, serde_saphyr::ser::Error> {
        serde_saphyr::to_string(self)
    }
}

impl SimulationData {
    /// Load from YAML string
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_saphyr::Error> {
        serde_saphyr::from_str(yaml)
    }

    /// Save to YAML string
    pub fn to_yaml(&self) -> Result<String, serde_saphyr::ser::Error> {
        serde_saphyr::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use crate::data::{
        events_data::{AccountTag, AmountData, EffectData, EventTag, IntervalData, TriggerData},
        parameters_data::{DistributionType, InflationData},
        portfolio_data::{AccountData, AccountType, AssetAccount, AssetValue, Property},
    };

    use super::*;

    #[test]
    fn test_complete_simulation_serialization() {
        let sim_data = SimulationData {
            portfolios: PortfolioData {
                name: "Retirement Plan".to_string(),
                description: Some("My retirement planning scenario".to_string()),
                accounts: vec![
                    AccountData {
                        name: "Checking".to_string(),
                        description: None,
                        account_type: AccountType::Checking(Property {
                            value: 5000.0,
                            return_profile: Some(ReturnProfileTag("HYSA".to_string())),
                        }),
                    },
                    AccountData {
                        name: "Brokerage".to_string(),
                        description: Some("Taxable investment account".to_string()),
                        account_type: AccountType::Brokerage(AssetAccount {
                            assets: vec![AssetValue {
                                asset: AssetTag("VTSAX".to_string()),
                                value: 100000.0,
                            }],
                        }),
                    },
                    AccountData {
                        name: "401k".to_string(),
                        description: None,
                        account_type: AccountType::Traditional401k(AssetAccount {
                            assets: vec![AssetValue {
                                asset: AssetTag("VTSAX".to_string()),
                                value: 450000.0,
                            }],
                        }),
                    },
                ],
            },
            profiles: vec![
                ProfileData {
                    name: ReturnProfileTag("S&P 500".to_string()),
                    description: Some("Historical S&P 500 returns".to_string()),
                    profile: crate::data::profiles_data::ReturnProfileData::Normal {
                        mean: 0.0957,
                        std_dev: 0.1652,
                    },
                },
                ProfileData {
                    name: ReturnProfileTag("HYSA".to_string()),
                    description: Some("High yield savings".to_string()),
                    profile: crate::data::profiles_data::ReturnProfileData::Fixed { rate: 0.045 },
                },
            ],
            assets: HashMap::from([(
                AssetTag("VTSAX".to_string()),
                ReturnProfileTag("S&P 500".to_string()),
            )]),
            historical_assets: HashMap::new(),
            events: vec![
                EventData {
                    name: EventTag("Monthly Salary".to_string()),
                    description: Some("Regular monthly income".to_string()),
                    trigger: TriggerData::Repeating {
                        interval: IntervalData::Monthly,
                        start: None,
                        end: Some(Box::new(TriggerData::Age {
                            years: 65,
                            months: None,
                        })),
                    },
                    effects: vec![EffectData::Income {
                        to: AccountTag("Checking".to_string()),
                        amount: AmountData::fixed(8000.0),
                        gross: true,
                        taxable: true,
                    }],
                    once: false,
                    enabled: true,
                },
                EventData {
                    name: EventTag("Living Expenses".to_string()),
                    description: None,
                    trigger: TriggerData::Repeating {
                        interval: IntervalData::Monthly,
                        start: None,
                        end: None,
                    },
                    effects: vec![EffectData::Expense {
                        from: AccountTag("Checking".to_string()),
                        amount: AmountData::fixed(5000.0),
                    }],
                    once: false,
                    enabled: true,
                },
            ],
            parameters: ParametersData {
                birth_date: "1985-06-15".to_string(),
                start_date: "2025-01-01".to_string(),
                duration_years: 40,
                inflation: InflationData::USHistorical {
                    distribution: DistributionType::Normal,
                },
                tax_config: Default::default(),
                returns_mode: Default::default(),
                historical_block_size: None,
            },
        };

        let yaml = sim_data.to_yaml().unwrap();
        println!("Complete Simulation YAML:\n{}", yaml);

        // Verify round-trip
        let deserialized = SimulationData::from_yaml(&yaml).unwrap();
        assert_eq!(deserialized.portfolios.accounts.len(), 3);
        assert_eq!(deserialized.events.len(), 2);
        assert_eq!(deserialized.profiles.len(), 2);
    }

    #[test]
    fn test_app_data_with_multiple_simulations() {
        let mut app_data = AppData::new();

        app_data.simulations.insert(
            "retirement".to_string(),
            SimulationData {
                portfolios: PortfolioData {
                    name: "Retirement".to_string(),
                    description: None,
                    accounts: vec![],
                },
                ..Default::default()
            },
        );

        app_data.simulations.insert(
            "early_retirement".to_string(),
            SimulationData {
                portfolios: PortfolioData {
                    name: "Early Retirement".to_string(),
                    description: None,
                    accounts: vec![],
                },
                ..Default::default()
            },
        );

        let yaml = app_data.to_yaml().unwrap();
        println!("App Data with multiple simulations:\n{}", yaml);

        let deserialized = AppData::from_yaml(&yaml).unwrap();
        assert_eq!(deserialized.simulations.len(), 2);
    }
}
