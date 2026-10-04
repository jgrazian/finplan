//! Scenario request bodies, and the date check they share.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{PlanError, PlanResult};
use crate::specs::WithdrawalStrategy;

/// Whether a scenario is a plan or an AI-guided draft still being written.
/// Drafts never appear in the scenario list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(type_name = "TEXT", rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ScenarioStatus {
    Draft,
    Active,
}

/// When cash runs short, sell investments in this order (spec 20). A plan
/// without one records a shortfall instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct FundingPolicySpec {
    pub strategy: WithdrawalStrategy,
    /// `BracketFilling` only: the highest marginal rate to fill to. Absent
    /// means 12%.
    #[serde(default)]
    pub bracket_ceiling: Option<f64>,
    /// Investment accounts never sold to cover a deficit.
    #[serde(default)]
    pub exclude_accounts: Vec<i64>,
}

/// `PUT /scenarios/{id}/funding`: set the policy, or turn it off with null.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SetFunding {
    pub funding: Option<FundingPolicySpec>,
    /// Drawdown's "Apply to plan": also give every `Strategy`-sourced Sweep
    /// the policy's strategy, so the plan has one strategy everywhere. Ignored
    /// when `funding` is null.
    #[serde(default)]
    #[ts(optional)]
    pub align_sweeps: Option<bool>,
}

impl FundingPolicySpec {
    /// The ceiling, when given, is only for `BracketFilling` and is a rate the
    /// table's CHECK accepts.
    pub fn check(&self) -> PlanResult<()> {
        let Some(rate) = self.bracket_ceiling else {
            return Ok(());
        };
        if !matches!(self.strategy, WithdrawalStrategy::BracketFilling) {
            return Err(PlanError::invalid(
                "bracket_ceiling only applies to the BracketFilling strategy",
            ));
        }
        if !(0.0..1.0).contains(&rate) {
            return Err(PlanError::invalid(
                "bracket_ceiling must be a rate from 0 up to (not including) 1",
            ));
        }
        Ok(())
    }
}

/// A scenario as `GET /scenarios/{id}` returns it.
#[derive(Debug, Serialize, TS)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
#[ts(export)]
pub struct Scenario {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub start_date: String,
    pub birth_date: Option<String>,
    pub duration_years: i64,
    pub inflation_profile_id: Option<i64>,
    pub tax_config_id: Option<i64>,
    pub collect_ledger: bool,
    pub status: ScenarioStatus,
    pub created_at: String,
    pub updated_at: String,
    /// When this scenario last produced results, and what they said. Carried
    /// on the row so a list of scenarios can be shown with its own history
    /// without a request per scenario.
    pub last_run_at: Option<String>,
    pub last_success_rate: Option<f64>,
    /// The plan's funding policy; null = off. The server selects it as one
    /// JSON column (`SCENARIO_COLUMNS`).
    #[cfg_attr(feature = "sqlx", sqlx(json(nullable)))]
    pub funding: Option<FundingPolicySpec>,
}

/// What lowering a scenario for the engine produced, without running it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CompileReport {
    pub ok: bool,
    pub accounts: usize,
    pub assets: usize,
    pub events: usize,
    pub return_profiles: usize,
    pub duration_years: usize,
}

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
