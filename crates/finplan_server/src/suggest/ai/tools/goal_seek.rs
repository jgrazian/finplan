//! `goal_seek`: search one plan parameter for the value that reaches a target
//! on a success metric ("retire at 43 reaches 90%").
//!
//! It runs the app's own goal seek (`api::analysis::ai_goal_seek`) over the
//! plan the loop is working on, so it costs several simulations. That is paid
//! from the loop's preview budget, [`PREVIEW_COST`] previews a call, and a
//! session may make [`MAX_GOAL_SEEKS`] calls at most. It never touches the
//! person's monthly goal-seek quota: that meters the analysis screen, not a
//! model's tool calls.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{ToolEnv, ToolOutput};
use crate::observability::AiToolOutcome;

/// Goal seeks one session (a review pass, or a whole drafting job) may run.
pub const MAX_GOAL_SEEKS: u32 = 2;
/// Previews one goal seek is charged: it runs a dozen or so simulations.
pub const PREVIEW_COST: u32 = 4;

/// The outcome measure the target is written against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Fraction of iterations ending with positive net worth.
    SuccessRate,
    /// Fraction that met every cash need on time. Stricter.
    FundingSuccessRate,
}

impl Metric {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SuccessRate => "success_rate",
            Self::FundingSuccessRate => "funding_success_rate",
        }
    }
}

/// Which end of the values that reach the target is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// The smallest value that reaches it: the earliest retirement age, the
    /// least contribution that funds the plan.
    Smallest,
    /// The largest value that still reaches it: the most spending.
    Largest,
}

/// A goal seek as the model asks for it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalSeekRequest {
    /// A named plan parameter, by id (`parameter:5`) or by name.
    pub parameter: String,
    pub metric: Metric,
    /// The metric to reach, as a fraction (`0.9` for 90%).
    pub target: f64,
    /// Default: the smallest for an age parameter, the largest otherwise.
    #[serde(default)]
    pub direction: Option<Direction>,
    /// The range to search, in the parameter's own units (dollars, a fraction,
    /// years of age, calendar days). Default: the parameter's suggested range.
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

pub(super) fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "parameter": {
                "type": "string",
                "description": "A named plan parameter: its id (`parameter:5`) or its name, as the plan lists them."
            },
            "metric": {"type": "string", "enum": ["success_rate", "funding_success_rate"]},
            "target": {
                "type": "number",
                "description": "The value the metric must reach, as a fraction: 0.9 for 90%.",
                "exclusiveMinimum": 0,
                "maximum": 1
            },
            "direction": {
                "type": "string",
                "enum": ["smallest", "largest"],
                "description": "smallest: the lowest value that reaches the target (earliest retirement age, least saving). largest: the highest value that still reaches it (most spending). Default: smallest for an age, largest otherwise."
            },
            "min": {"type": "number", "description": "Lower end of the range to search, in the parameter's units (age in years). Default: the parameter's own suggested range."},
            "max": {"type": "number", "description": "Upper end of the range to search."}
        },
        "required": ["parameter", "metric", "target"]
    })
}

pub(super) async fn run(input: &Value, env: &ToolEnv<'_>) -> ToolOutput {
    let request: GoalSeekRequest = match serde_json::from_value(input.clone()) {
        Ok(request) => request,
        Err(e) => {
            return ToolOutput::error(AiToolOutcome::Invalid, format!("invalid goal seek: {e}"));
        }
    };
    if !request.target.is_finite() || request.target <= 0.0 || request.target > 1.0 {
        return ToolOutput::error(
            AiToolOutcome::Invalid,
            "target is a fraction above 0 and at most 1: 0.9 for 90%",
        );
    }
    if env.goal_seeks_left == 0 {
        return ToolOutput::error(
            AiToolOutcome::BudgetExhausted,
            format!(
                "goal_seek may be used {MAX_GOAL_SEEKS} times in one session and both are spent. \
                 Use the results you have, or preview paths."
            ),
        );
    }
    if env.previews_left < PREVIEW_COST {
        return ToolOutput::error(
            AiToolOutcome::BudgetExhausted,
            format!(
                "a goal seek costs {PREVIEW_COST} previews and {} are left. Use preview_changes on the paths that matter, or stop.",
                env.previews_left
            ),
        );
    }
    // A simulation was attempted: it counts, whatever it comes back with.
    let mut out = match env.host.goal_seek(request).await {
        Ok(found) => ToolOutput::ok(found.to_string()),
        Err(message) => ToolOutput::error(AiToolOutcome::Error, message),
    };
    out.previews_spent = PREVIEW_COST;
    out.goal_seeks_spent = 1;
    out
}
