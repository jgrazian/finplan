//! `inspect_path`, `cash_flow_breakdown` and `failure_profile`: what a stored
//! run shows about its bad cases.
//!
//! The run stores the percentile paths it was started with (10, 50 and 90 by
//! default): cash flows, balances and warnings for each. `inspect_path` serves
//! the nearest stored one to the rank asked for, through the host, and says
//! which it was. Per-iteration data for the other paths is not stored, so
//! `failure_profile` is built from the aggregates every run keeps
//! (`funding_diagnostics`). `cash_flow_breakdown` sums a stored path's ledger
//! by the event (or, for rows no event wrote, the account) behind each flow.

use serde_json::{Value, json};

use super::{ToolEnv, ToolOutput, unavailable};
use crate::observability::AiToolOutcome;

/// Which stored path to look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRank {
    /// The lowest stored percentile.
    Worst,
    P10,
    P25,
    Median,
}

impl PathRank {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "worst" => Some(Self::Worst),
            "p10" => Some(Self::P10),
            "p25" => Some(Self::P25),
            "median" => Some(Self::Median),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Worst => "worst",
            Self::P10 => "p10",
            Self::P25 => "p25",
            Self::Median => "median",
        }
    }

    /// The percentile to ask for; `None` for the worst.
    pub fn percentile(self) -> Option<f64> {
        match self {
            Self::Worst => None,
            Self::P10 => Some(0.10),
            Self::P25 => Some(0.25),
            Self::Median => Some(0.50),
        }
    }
}

pub(super) fn inspect_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "rank": {"type": "string", "enum": ["worst", "p10", "p25", "median"], "description": "worst is the lowest stored percentile. The nearest stored path is returned."},
            "years": {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2, "description": "[first, last] calendar year to show; default the whole run (long runs are thinned)."}
        },
        "required": ["rank"]
    })
}

/// A path tool's input: which stored path, and optionally which years.
fn path_input(input: &Value) -> Result<(PathRank, Option<(i64, i64)>), ToolOutput> {
    let Some(rank) = input
        .get("rank")
        .and_then(Value::as_str)
        .and_then(PathRank::parse)
    else {
        return Err(ToolOutput::error(
            AiToolOutcome::Invalid,
            "rank must be worst, p10, p25 or median",
        ));
    };
    let years = match input.get("years") {
        None | Some(Value::Null) => None,
        Some(Value::Array(pair)) => match (
            pair.first().and_then(Value::as_i64),
            pair.get(1).and_then(Value::as_i64),
        ) {
            (Some(from), Some(to)) if pair.len() == 2 && from <= to => Some((from, to)),
            _ => {
                return Err(ToolOutput::error(
                    AiToolOutcome::Invalid,
                    "years must be [first, last] with first <= last",
                ));
            }
        },
        Some(_) => {
            return Err(ToolOutput::error(
                AiToolOutcome::Invalid,
                "years must be [first, last]",
            ));
        }
    };
    Ok((rank, years))
}

pub(super) async fn inspect_path(input: &Value, env: &ToolEnv<'_>) -> ToolOutput {
    let (rank, years) = match path_input(input) {
        Ok(parsed) => parsed,
        Err(refused) => return refused,
    };
    match env.host.inspect_path(rank, years).await {
        Ok(text) => ToolOutput::ok(text),
        Err(message) => ToolOutput::error(AiToolOutcome::Error, message),
    }
}

pub(super) async fn cash_flow_breakdown(input: &Value, env: &ToolEnv<'_>) -> ToolOutput {
    let (rank, years) = match path_input(input) {
        Ok(parsed) => parsed,
        Err(refused) => return refused,
    };
    match env.host.cash_flow_breakdown(rank, years).await {
        Ok(text) => ToolOutput::ok(text),
        Err(message) => ToolOutput::error(AiToolOutcome::Error, message),
    }
}

pub(super) fn failure_profile(env: &ToolEnv<'_>) -> ToolOutput {
    match env.failure_profile {
        Some(profile) => ToolOutput::ok(profile.to_string()),
        None => ToolOutput::error(AiToolOutcome::Error, unavailable("failure_profile")),
    }
}
