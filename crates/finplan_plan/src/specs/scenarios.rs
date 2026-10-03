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

impl UpdateScenario {
    /// A name, when given, is not blank.
    pub fn check_name(&self) -> PlanResult<()> {
        if self
            .name
            .as_deref()
            .is_some_and(|name| name.trim().is_empty())
        {
            return Err(PlanError::invalid("scenario name cannot be empty"));
        }
        Ok(())
    }

    /// The horizon, when given, is one the table's CHECK accepts.
    pub fn check_duration(&self) -> PlanResult<()> {
        if self.duration_years.is_some_and(|y| !(1..=120).contains(&y)) {
            return Err(PlanError::invalid(
                "duration_years must be between 1 and 120",
            ));
        }
        Ok(())
    }

    /// The start and birth dates, when given, as stored: valid and
    /// normalized. The start date is checked first.
    pub fn dates(&self) -> PlanResult<(Option<String>, Option<String>)> {
        let start_date = self
            .start_date
            .as_deref()
            .map(|d| validate_date(d, "start_date"))
            .transpose()?;
        let birth_date = self
            .birth_date
            .as_deref()
            .map(|d| validate_date(d, "birth_date"))
            .transpose()?;
        Ok((start_date, birth_date))
    }
}
