//! Scenario request bodies, and the date check they share.

use serde::Deserialize;
use ts_rs::TS;

use crate::error::{PlanError, PlanResult};

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateScenario {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub start_date: String,
    #[serde(default)]
    pub birth_date: Option<String>,
    #[serde(default = "default_duration")]
    pub duration_years: i64,
    #[serde(default)]
    pub inflation_profile_id: Option<i64>,
    #[serde(default)]
    pub tax_config_id: Option<i64>,
}

fn default_duration() -> i64 {
    30
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateScenario {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub birth_date: Option<String>,
    #[serde(default)]
    pub duration_years: Option<i64>,
    #[serde(default)]
    pub inflation_profile_id: Option<i64>,
    #[serde(default)]
    pub tax_config_id: Option<i64>,
    #[serde(default)]
    pub collect_ledger: Option<bool>,
}

pub fn validate_date(text: &str, field: &str) -> PlanResult<String> {
    text.parse::<jiff::civil::Date>()
        .map(|d| d.to_string())
        .map_err(|e| PlanError::invalid(format!("invalid {field} '{text}': {e}")))
}
