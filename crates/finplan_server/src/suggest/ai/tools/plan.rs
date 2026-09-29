//! The plan group: `preview_changes`, `preview_paths`, `validate_changes` and
//! `preflight`.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{ToolEnv, ToolOutput};
use crate::api::suggestions::MAX_STEPS;
use crate::observability::AiToolOutcome;
use crate::suggest::Change;
use crate::suggest::ai::prompt::change_schema;

/// Most changes in one step.
pub const MAX_CHANGES: usize = 12;
/// Paths one `preview_paths` call takes.
const MIN_PATHS: usize = 2;
const MAX_PATHS: usize = 4;

/// A path's steps as one batch, keyed for "was exactly this previewed".
pub fn batch_key(steps: &[Vec<Change>]) -> String {
    let batch: Vec<&Change> = steps.iter().flatten().collect();
    serde_json::to_string(&batch).unwrap_or_default()
}

pub(super) fn empty_schema() -> Value {
    json!({"type": "object", "properties": {}})
}

fn steps_schema() -> Value {
    json!({
        "type": "array",
        "description": "The path's steps in order, each a list of changes.",
        "items": {"type": "array", "items": change_schema(), "minItems": 1},
        "minItems": 1,
        "maxItems": MAX_STEPS
    })
}

pub(super) fn preview_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"steps": steps_schema()},
        "required": ["steps"]
    })
}

pub(super) fn preview_paths_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "paths": {
                "type": "array",
                "description": "Two to four paths of one note.",
                "minItems": MIN_PATHS,
                "maxItems": MAX_PATHS,
                "items": {
                    "type": "object",
                    "properties": {
                        "key": {"type": "string", "description": "The path's key, echoed back."},
                        "steps": steps_schema()
                    },
                    "required": ["steps"]
                }
            }
        },
        "required": ["paths"]
    })
}

/// A path to preview: its steps in order. A bare `changes` list reads as a
/// path of one step.
#[derive(Debug, Deserialize)]
struct PreviewInput {
    #[serde(default)]
    steps: Vec<Vec<Change>>,
    #[serde(default)]
    changes: Vec<Change>,
}

#[derive(Debug, Deserialize)]
struct PathsInput {
    #[serde(default)]
    paths: Vec<PathInput>,
}

#[derive(Debug, Deserialize)]
struct PathInput {
    #[serde(default)]
    key: Option<String>,
    #[serde(flatten)]
    path: PreviewInput,
}

impl PreviewInput {
    /// The path's steps, when they are within the caps.
    fn into_steps(self) -> Result<Vec<Vec<Change>>, String> {
        let steps = if self.steps.is_empty() && !self.changes.is_empty() {
            vec![self.changes]
        } else {
            self.steps
        };
        if steps.is_empty()
            || steps.len() > MAX_STEPS
            || steps.iter().any(|s| s.is_empty() || s.len() > MAX_CHANGES)
        {
            return Err(format!(
                "send 1 to {MAX_STEPS} steps, each of 1 to {MAX_CHANGES} changes"
            ));
        }
        Ok(steps)
    }
}

const BUDGET_SPENT: &str = "The preview budget for this review is used up. Submit notes with an estimate instead, or stop.";

fn problems_of(preview: &Value) -> usize {
    preview
        .get("problems")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

pub(super) async fn preview(input: &Value, env: &ToolEnv<'_>) -> ToolOutput {
    if env.previews_left == 0 {
        return ToolOutput::error(AiToolOutcome::BudgetExhausted, BUDGET_SPENT);
    }
    let steps = match serde_json::from_value::<PreviewInput>(input.clone())
        .map_err(|e| format!("invalid changes: {e}"))
        .and_then(PreviewInput::into_steps)
    {
        Ok(steps) => steps,
        Err(message) => return ToolOutput::error(AiToolOutcome::Invalid, message),
    };
    let key = batch_key(&steps);
    let changes: Vec<Change> = steps.into_iter().flatten().collect();
    // A simulation was attempted: it counts, whatever it comes back with.
    let mut out = match env.host.preview(changes).await {
        Ok(preview) => {
            let problems = problems_of(&preview);
            let mut out = ToolOutput::ok(preview.to_string());
            out.outcome = if problems == 0 {
                AiToolOutcome::Ok
            } else {
                AiToolOutcome::Problems
            };
            out.problems = problems;
            out.paired = preview.get("paired").and_then(Value::as_bool);
            if problems == 0 {
                out.previewed.push((key, preview));
            }
            out
        }
        Err(message) => ToolOutput::error(AiToolOutcome::Error, message),
    };
    out.previews_spent = 1;
    out
}

pub(super) async fn preview_paths(input: &Value, env: &ToolEnv<'_>) -> ToolOutput {
    let parsed = match serde_json::from_value::<PathsInput>(input.clone()) {
        Ok(p) => p,
        Err(e) => {
            return ToolOutput::error(AiToolOutcome::Invalid, format!("invalid paths: {e}"));
        }
    };
    if !(MIN_PATHS..=MAX_PATHS).contains(&parsed.paths.len()) {
        return ToolOutput::error(
            AiToolOutcome::Invalid,
            format!("send {MIN_PATHS} to {MAX_PATHS} paths; for one path use preview_changes"),
        );
    }
    let mut paths = Vec::with_capacity(parsed.paths.len());
    for (i, p) in parsed.paths.into_iter().enumerate() {
        match p.path.into_steps() {
            Ok(steps) => paths.push((p.key.unwrap_or_else(|| format!("path-{}", i + 1)), steps)),
            Err(message) => {
                return ToolOutput::error(
                    AiToolOutcome::Invalid,
                    format!("path {}: {message}", i + 1),
                );
            }
        }
    }
    let wanted = paths.len() as u32;
    if env.previews_left == 0 {
        return ToolOutput::error(AiToolOutcome::BudgetExhausted, BUDGET_SPENT);
    }
    if env.previews_left < wanted {
        return ToolOutput::error(
            AiToolOutcome::BudgetExhausted,
            format!(
                "{wanted} paths need {wanted} previews and {} are left. Preview the most important ones with preview_changes, or stop.",
                env.previews_left
            ),
        );
    }

    let mut out = ToolOutput::ok(String::new());
    let mut results = Vec::with_capacity(paths.len());
    let mut all_paired = true;
    let mut any_error = false;
    for (key, steps) in paths {
        let batch = batch_key(&steps);
        let changes: Vec<Change> = steps.into_iter().flatten().collect();
        out.previews_spent += 1;
        match env.host.preview(changes).await {
            Ok(preview) => {
                let problems = problems_of(&preview);
                out.problems += problems;
                all_paired &= preview.get("paired").and_then(Value::as_bool) == Some(true);
                if problems == 0 {
                    out.previewed.push((batch, preview.clone()));
                }
                results.push(json!({"path": key, "preview": preview}));
            }
            Err(message) => {
                any_error = true;
                results.push(json!({"path": key, "error": message}));
            }
        }
    }
    out.paired = Some(all_paired);
    out.outcome = if any_error {
        AiToolOutcome::Error
    } else if out.problems > 0 {
        AiToolOutcome::Problems
    } else {
        AiToolOutcome::Ok
    };
    out.text = json!({"paths": results}).to_string();
    out
}

pub(super) fn validate(input: &Value, env: &ToolEnv<'_>) -> ToolOutput {
    let steps = match serde_json::from_value::<PreviewInput>(input.clone())
        .map_err(|e| format!("invalid changes: {e}"))
        .and_then(PreviewInput::into_steps)
    {
        Ok(steps) => steps,
        Err(message) => return ToolOutput::error(AiToolOutcome::Invalid, message),
    };
    match env.host.resolve_steps(&steps) {
        Ok(diffs) => {
            let steps: Vec<Value> = diffs
                .iter()
                .enumerate()
                .map(|(i, diff)| json!({"step": i, "diff": diff}))
                .collect();
            ToolOutput::ok(json!({"valid": true, "steps": steps}).to_string())
        }
        Err((step, problems)) => {
            let mut out = ToolOutput::ok(
                json!({"valid": false, "failed_step": step, "problems": problems}).to_string(),
            );
            out.outcome = AiToolOutcome::Problems;
            out.problems = problems.len();
            out
        }
    }
}

pub(super) fn preflight(env: &ToolEnv<'_>) -> ToolOutput {
    match env.host.preflight() {
        Ok(report) => ToolOutput::ok(report.to_string()),
        Err(message) => ToolOutput::error(AiToolOutcome::Error, message),
    }
}
