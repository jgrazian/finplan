//! Analysis configuration data for persistence.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Persisted analysis configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisConfigData {
    /// Monte Carlo iterations per sweep point
    #[serde(default = "default_mc_iterations")]
    pub mc_iterations: usize,

    /// Default number of steps for new sweep parameters
    #[serde(default = "default_steps")]
    pub default_steps: usize,

    /// Sweep parameters configuration
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sweep_parameters: Vec<SweepParameterData>,

    /// Selected metrics to compute
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub selected_metrics: HashSet<AnalysisMetricData>,
}

fn default_mc_iterations() -> usize {
    500
}

fn default_steps() -> usize {
    6
}

/// Sweep parameter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepParameterData {
    /// Event name being swept (resolved to EventId at runtime)
    #[serde(alias = "name")]
    pub event_name: String,

    /// Type of sweep
    pub sweep_type: SweepTypeData,

    /// Minimum value
    pub min_value: f64,

    /// Maximum value
    pub max_value: f64,

    /// Number of steps
    pub step_count: usize,
}

/// Type of parameter being swept
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SweepTypeData {
    /// Age trigger (years)
    TriggerAge,
    /// Date trigger (year)
    TriggerDate,
    /// Effect amount (dollars)
    EffectValue,
    /// Repeating event start age
    RepeatingStartAge,
    /// Repeating event end age
    RepeatingEndAge,
}

impl SweepTypeData {
    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::TriggerAge => "Age",
            Self::TriggerDate => "Year",
            Self::EffectValue => "Amount",
            Self::RepeatingStartAge => "Start Age",
            Self::RepeatingEndAge => "End Age",
        }
    }
}

/// Analysis metric type for persistence
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMetricData {
    SuccessRate,
    P5FinalNetWorth,
    P50FinalNetWorth,
    P95FinalNetWorth,
    LifetimeTaxes,
    #[serde(rename = "net_worth_at_age")]
    NetWorthAtAge {
        age: u8,
    },
    MaxDrawdown,
}

impl AnalysisMetricData {
    pub fn label(&self) -> String {
        match self {
            Self::SuccessRate => "Success Rate".to_string(),
            Self::NetWorthAtAge { age } => format!("Net Worth at {}", age),
            Self::P5FinalNetWorth => "P5 Final Net Worth".to_string(),
            Self::P50FinalNetWorth => "P50 Final Net Worth".to_string(),
            Self::P95FinalNetWorth => "P95 Final Net Worth".to_string(),
            Self::LifetimeTaxes => "Lifetime Taxes".to_string(),
            Self::MaxDrawdown => "Max Drawdown".to_string(),
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            Self::SuccessRate => "Success %",
            Self::NetWorthAtAge { .. } => "Net Worth",
            Self::P5FinalNetWorth => "P5",
            Self::P50FinalNetWorth => "P50",
            Self::P95FinalNetWorth => "P95",
            Self::LifetimeTaxes => "Taxes",
            Self::MaxDrawdown => "Drawdown",
        }
    }
}
