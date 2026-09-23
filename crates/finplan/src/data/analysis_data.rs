//! Analysis configuration data for persistence.

use finplan_core::{
    model::{ParameterId, ParameterValue},
    optimization::OptimizableParameter,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

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
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_sweep_parameters"
    )]
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

/// A named parameter and inclusive bounds of its declared type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepParameterData {
    pub parameter_name: String,
    pub min_value: ParameterValue,
    pub max_value: ParameterValue,
    pub step_count: usize,
}

impl SweepParameterData {
    pub fn parameter(&self, parameter_id: ParameterId) -> OptimizableParameter {
        OptimizableParameter {
            parameter_id,
            min_value: self.min_value,
            max_value: self.max_value,
        }
    }

    pub fn validate(&self, current: ParameterValue) -> Result<(), String> {
        let parameter = self.parameter(ParameterId(0));
        let (min, max) = parameter.bounds();
        if std::mem::discriminant(&current) != std::mem::discriminant(&self.min_value)
            || parameter.value_at(min).is_none()
            || parameter.value_at(max).is_none()
            || !(max - min).is_finite()
            || min >= max
        {
            return Err(
                "Enter valid bounds of the parameter's type, with minimum less than maximum".into(),
            );
        }
        if self.step_count < 2 {
            return Err("Use at least 2 steps".into());
        }
        if parameter.is_discrete() && self.step_count as f64 > max - min + 1.0 {
            return Err(
                "Steps cannot exceed the number of distinct days or months in the range".into(),
            );
        }
        Ok(())
    }

    pub fn format_coordinate(&self, coordinate: f64) -> String {
        self.parameter(ParameterId(0))
            .value_at(coordinate)
            .map(format_sweep_value)
            .unwrap_or_else(|| coordinate.to_string())
    }
}

pub fn format_sweep_value(value: ParameterValue) -> String {
    match value {
        ParameterValue::Money(value) => crate::util::format::format_compact_currency(value),
        ParameterValue::Rate(value) => format!("{:.2}%", value * 100.0),
        ParameterValue::Date(date) => date.to_string(),
        ParameterValue::Age(age) => format!("{}y {}m", age.years, age.months),
    }
}

// Old event sweeps cannot be mapped to named inputs reliably. Ignore those
// selections on load while preserving the scenario and its other analysis settings.
fn deserialize_sweep_parameters<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<SweepParameterData>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SavedSweep {
        Named(SweepParameterData),
        Legacy {
            #[serde(alias = "name")]
            event_name: String,
            sweep_type: String,
        },
    }
    Ok(Vec::<SavedSweep>::deserialize(deserializer)?
        .into_iter()
        .filter_map(|entry| match entry {
            SavedSweep::Named(parameter) => Some(parameter),
            SavedSweep::Legacy {
                event_name,
                sweep_type,
            } => {
                tracing::debug!(
                    event_name,
                    sweep_type,
                    "Ignoring legacy event sweep; choose a named parameter"
                );
                None
            }
        })
        .collect())
}

/// Analysis metric type for persistence
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMetricData {
    SuccessRate,
    P5FinalNetWorth,
    P25FinalNetWorth,
    P50FinalNetWorth,
    P75FinalNetWorth,
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
            Self::P25FinalNetWorth => "P25 Final Net Worth".to_string(),
            Self::P50FinalNetWorth => "P50 Final Net Worth".to_string(),
            Self::P75FinalNetWorth => "P75 Final Net Worth".to_string(),
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
            Self::P25FinalNetWorth => "P25",
            Self::P50FinalNetWorth => "P50",
            Self::P75FinalNetWorth => "P75",
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

/// Color scheme for heatmaps (viridis family)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    /// Viridis: purple -> teal -> green -> yellow (default, perceptually uniform)
    #[default]
    Viridis,
    /// Magma: dark purple -> magenta -> pink -> light yellow
    Magma,
    /// Inferno: dark purple -> red/orange -> yellow
    Inferno,
    /// Plasma: blue -> purple -> orange -> yellow
    Plasma,
    /// Cividis: dark blue -> gray/tan -> yellow (colorblind-friendly)
    Cividis,
    /// Rocket: dark blue -> magenta -> pink/cream
    Rocket,
    /// Mako: dark -> purple -> teal/cyan -> light
    Mako,
    /// Turbo: purple -> blue -> cyan -> green -> yellow -> orange -> red
    Turbo,
}

impl ColorScheme {
    /// Get display name for the color scheme
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Viridis => "Viridis",
            Self::Magma => "Magma",
            Self::Inferno => "Inferno",
            Self::Plasma => "Plasma",
            Self::Cividis => "Cividis",
            Self::Rocket => "Rocket",
            Self::Mako => "Mako",
            Self::Turbo => "Turbo",
        }
    }

    /// Get all available color schemes
    pub fn all() -> &'static [ColorScheme] {
        &[
            Self::Viridis,
            Self::Magma,
            Self::Inferno,
            Self::Plasma,
            Self::Cividis,
            Self::Rocket,
            Self::Mako,
            Self::Turbo,
        ]
    }

    /// Get the next color scheme in the list
    pub fn next(&self) -> Self {
        match self {
            Self::Viridis => Self::Magma,
            Self::Magma => Self::Inferno,
            Self::Inferno => Self::Plasma,
            Self::Plasma => Self::Cividis,
            Self::Cividis => Self::Rocket,
            Self::Rocket => Self::Mako,
            Self::Mako => Self::Turbo,
            Self::Turbo => Self::Viridis,
        }
    }

    /// Get the previous color scheme in the list
    pub fn prev(&self) -> Self {
        match self {
            Self::Viridis => Self::Turbo,
            Self::Magma => Self::Viridis,
            Self::Inferno => Self::Magma,
            Self::Plasma => Self::Inferno,
            Self::Cividis => Self::Plasma,
            Self::Rocket => Self::Cividis,
            Self::Mako => Self::Rocket,
            Self::Turbo => Self::Mako,
        }
    }
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

    /// Color scheme for heatmaps
    #[serde(default)]
    pub color_scheme: ColorScheme,

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
            color_scheme: ColorScheme::default(),
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
            color_scheme: ColorScheme::default(),
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
            color_scheme: ColorScheme::default(),
            fixed_values: HashMap::new(),
        }
    }
}

// ========== Cache Types for Sweep Results Persistence ==========

/// Fingerprint for cache validation - captures the config state that affects results
///
/// Note: Selected metrics are NOT included in the fingerprint because metrics can
/// be computed on-demand from the raw sweep data without re-running simulations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweepConfigFingerprint {
    /// Hash of sweep parameters configuration
    pub parameters_hash: String,
    /// Number of Monte Carlo iterations per sweep point
    pub mc_iterations: usize,
}

impl SweepConfigFingerprint {
    /// Create a fingerprint from the current analysis configuration
    pub fn from_config(config: &AnalysisConfigData) -> Self {
        Self {
            parameters_hash: Self::hash_parameters(&config.sweep_parameters),
            mc_iterations: config.mc_iterations,
        }
    }

    /// Hash the sweep parameters to detect changes
    fn hash_parameters(params: &[SweepParameterData]) -> String {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();

        for param in params {
            param.parameter_name.hash(&mut hasher);
            // Typed bounds include the calendar origin and distinguish Money from Rate.
            format!("{:?}", param.min_value).hash(&mut hasher);
            format!("{:?}", param.max_value).hash(&mut hasher);
            param.step_count.hash(&mut hasher);
        }

        format!("{:x}", hasher.finish())
    }
}

/// Cached sweep results with validation fingerprint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSweepResults {
    /// Fingerprint for cache validation
    pub fingerprint: SweepConfigFingerprint,
    /// The cached sweep results
    pub results: finplan_core::analysis::SweepResults,
    /// When the cache was created (ISO 8601 timestamp)
    pub created_at: String,
}

#[cfg(test)]
mod named_sweep_tests {
    use super::*;
    use crate::{data::named_parameters::NamedParameterData, state::AnalysisState};
    use jiff::civil::date;

    #[test]
    fn old_event_sweeps_are_removed_without_losing_analysis_settings() {
        let yaml = "mc_iterations: 25\ndefault_steps: 7\nsweep_parameters:\n  - event_name: Salary\n    sweep_type: effect_value\n    min_value: 100\n    max_value: 200\n    step_count: 3\n  - name: Retirement\n    sweep_type: trigger_age\n    min_value: 60\n    max_value: 70\n    step_count: 6\n";
        let config: AnalysisConfigData = serde_saphyr::from_str(yaml).unwrap();
        assert!(config.sweep_parameters.is_empty());
        assert_eq!(config.mc_iterations, 25);
        assert_eq!(config.default_steps, 7);
    }

    #[test]
    fn scenario_loading_keeps_only_existing_variables_with_matching_bounds() {
        let sweep = SweepParameterData {
            parameter_name: "Spend".into(),
            min_value: ParameterValue::Money(100.0),
            max_value: ParameterValue::Money(200.0),
            step_count: 3,
        };
        let config = AnalysisConfigData {
            sweep_parameters: vec![sweep],
            ..Default::default()
        };
        let mut state = AnalysisState::new();
        state.load_from_config(&config, &[]);
        assert!(state.sweep_parameters.is_empty());
        state.load_from_config(
            &config,
            &[NamedParameterData {
                name: "Spend".into(),
                value: ParameterValue::Rate(0.1),
            }],
        );
        assert!(state.sweep_parameters.is_empty());
        state.load_from_config(
            &config,
            &[NamedParameterData {
                name: "Spend".into(),
                value: ParameterValue::Money(150.0),
            }],
        );
        assert_eq!(state.sweep_parameters, config.sweep_parameters);
    }

    #[test]
    fn fingerprints_include_named_target_type_and_calendar_origin() {
        let mut config = AnalysisConfigData {
            sweep_parameters: vec![SweepParameterData {
                parameter_name: "Start".into(),
                min_value: ParameterValue::Date(date(2025, 1, 1)),
                max_value: ParameterValue::Date(date(2025, 1, 3)),
                step_count: 3,
            }],
            ..Default::default()
        };
        let original = SweepConfigFingerprint::from_config(&config);
        config.sweep_parameters[0].min_value = ParameterValue::Date(date(2026, 1, 1));
        config.sweep_parameters[0].max_value = ParameterValue::Date(date(2026, 1, 3));
        assert_ne!(original, SweepConfigFingerprint::from_config(&config));
        config.sweep_parameters[0].min_value = ParameterValue::Money(1.0);
        config.sweep_parameters[0].max_value = ParameterValue::Money(2.0);
        let money = SweepConfigFingerprint::from_config(&config);
        config.sweep_parameters[0].min_value = ParameterValue::Rate(1.0);
        config.sweep_parameters[0].max_value = ParameterValue::Rate(2.0);
        assert_ne!(money, SweepConfigFingerprint::from_config(&config));
    }
}
