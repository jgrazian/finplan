//! Analysis configuration data for persistence.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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

    /// Configured result charts (persisted per scenario)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chart_configs: Vec<ChartConfigData>,
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

/// Type of chart to render in results panel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    /// 1D scatter/line plot (single parameter on X-axis)
    #[default]
    Scatter1D,
    /// 2D heatmap (two parameters on X and Y axes)
    Heatmap2D,
}

impl ChartType {
    /// Get display name for the chart type
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Scatter1D => "1D Scatter",
            Self::Heatmap2D => "2D Heatmap",
        }
    }
}

/// Configuration for a single chart in the results panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartConfigData {
    /// Chart type (1D scatter or 2D heatmap)
    pub chart_type: ChartType,

    /// Parameter dimension index for X-axis
    pub x_param_index: usize,

    /// Parameter dimension index for Y-axis (only for 2D heatmaps)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y_param_index: Option<usize>,

    /// Metric to display
    pub metric: AnalysisMetricData,

    /// Fixed values for non-displayed dimensions (dimension index -> step index)
    /// Uses midpoint if not specified for a dimension
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub fixed_values: HashMap<usize, usize>,
}

impl Default for ChartConfigData {
    fn default() -> Self {
        Self {
            chart_type: ChartType::Scatter1D,
            x_param_index: 0,
            y_param_index: None,
            metric: AnalysisMetricData::SuccessRate,
            fixed_values: HashMap::new(),
        }
    }
}

impl ChartConfigData {
    /// Create a default 1D chart for a given parameter
    pub fn new_1d(x_param: usize, metric: AnalysisMetricData) -> Self {
        Self {
            chart_type: ChartType::Scatter1D,
            x_param_index: x_param,
            y_param_index: None,
            metric,
            fixed_values: HashMap::new(),
        }
    }

    /// Create a default 2D heatmap for two parameters
    pub fn new_2d(x_param: usize, y_param: usize, metric: AnalysisMetricData) -> Self {
        Self {
            chart_type: ChartType::Heatmap2D,
            x_param_index: x_param,
            y_param_index: Some(y_param),
            metric,
            fixed_values: HashMap::new(),
        }
    }
}
