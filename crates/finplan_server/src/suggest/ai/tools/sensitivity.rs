//! `sensitivity`: which of the plan's named parameters move its success rate
//! most ("spending matters three times as much as the retirement age").
//!
//! It runs the app's own sensitivity ranking (`api::analysis::ai_sensitivity`)
//! over the plan the loop is working on: each parameter is moved down and up
//! on its own, two simulations apiece, plus one of the plan as it stands. Like
//! `goal_seek` it is paid from the loop's preview budget, [`PREVIEW_COST`]
//! previews a call, and a session may make [`MAX_SENSITIVITIES`] calls. A call
//! refused before anything is simulated (an unknown parameter, a plan with
//! none) costs nothing.

use serde::Deserialize;
use serde_json::{Value, json};

use super::goal_seek::Metric;
use super::{ToolEnv, ToolOutput};
use crate::observability::AiToolOutcome;

/// Rankings one session may run.
pub const MAX_SENSITIVITIES: u32 = 1;
/// Previews one ranking is charged: about as much simulation as a goal seek.
pub const PREVIEW_COST: u32 = 4;
/// Most parameters one ranking moves; a plan with more names the ones to try.
pub const MAX_PARAMETERS: usize = 8;

/// A ranking as the model asks for it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensitivityRequest {
    /// Named plan parameters, by id (`parameter:5`) or by name. Empty: every
    /// one, when the plan has at most [`MAX_PARAMETERS`].
    #[serde(default)]
    pub parameters: Vec<String>,
    /// How far amounts and rates move each way, as a fraction of their value.
    /// Default 0.2. Ages always move five years and dates one.
    #[serde(default)]
    pub fraction: Option<f64>,
    /// What the ranking is by. Default `funding_success_rate`.
    #[serde(default)]
    pub metric: Option<Metric>,
}

/// Why a ranking produced nothing.
#[derive(Debug, Clone)]
pub enum SensitivityError {
    /// Turned away before anything was simulated: not charged.
    Refused(String),
    /// The simulations ran, or started to, and failed: charged.
    Failed(String),
}

pub(super) fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "parameters": {
                "type": "array",
                "items": {"type": "string"},
                "maxItems": MAX_PARAMETERS,
                "description": "Named plan parameters to move, by id (`parameter:5`) or name as the plan lists them. Default: all of them, when the plan has at most 8."
            },
            "fraction": {
                "type": "number",
                "minimum": 0.01,
                "maximum": 1,
                "description": "How far each amount or rate moves down and up, as a fraction of its value: 0.2 is ±20%. Default 0.2. Ages always move ±5 years and dates ±1 year."
            },
            "metric": {
                "type": "string",
                "enum": ["success_rate", "funding_success_rate"],
                "description": "What the ranking is by. Default funding_success_rate."
            }
        }
    })
}

pub(super) async fn run(input: &Value, env: &ToolEnv<'_>) -> ToolOutput {
    let request: SensitivityRequest = match serde_json::from_value(input.clone()) {
        Ok(request) => request,
        Err(e) => {
            return ToolOutput::error(AiToolOutcome::Invalid, format!("invalid sensitivity: {e}"));
        }
    };
    if request
        .fraction
        .is_some_and(|f| !f.is_finite() || !(0.01..=1.0).contains(&f))
    {
        return ToolOutput::error(
            AiToolOutcome::Invalid,
            "fraction is between 0.01 and 1: 0.2 for ±20%",
        );
    }
    if request.parameters.len() > MAX_PARAMETERS {
        return ToolOutput::error(
            AiToolOutcome::Invalid,
            format!("name at most {MAX_PARAMETERS} parameters"),
        );
    }
    if env.sensitivities_left == 0 {
        return ToolOutput::error(
            AiToolOutcome::BudgetExhausted,
            format!(
                "sensitivity may be used {MAX_SENSITIVITIES} time in one session and it is spent. \
                 Use its ranking, goal_seek or previews."
            ),
        );
    }
    if env.previews_left < PREVIEW_COST {
        return ToolOutput::error(
            AiToolOutcome::BudgetExhausted,
            format!(
                "a sensitivity ranking costs {PREVIEW_COST} previews and {} are left. Use preview_changes on the paths that matter, or stop.",
                env.previews_left
            ),
        );
    }
    match env.host.sensitivity(request).await {
        Ok(ranking) => {
            let mut out = ToolOutput::ok(ranking.to_string());
            out.previews_spent = PREVIEW_COST;
            out.sensitivities_spent = 1;
            out
        }
        Err(SensitivityError::Refused(message)) => {
            ToolOutput::error(AiToolOutcome::Invalid, message)
        }
        Err(SensitivityError::Failed(message)) => {
            let mut out = ToolOutput::error(AiToolOutcome::Error, message);
            out.previews_spent = PREVIEW_COST;
            out.sensitivities_spent = 1;
            out
        }
    }
}
