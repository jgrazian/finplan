//! The plan group: `preview_changes`, `preview_paths`, `validate_changes` and
//! `preflight`.

use serde_json::{Value, json};

use super::{ToolEnv, ToolOutput};
use crate::api::suggestions::MAX_STEPS;
use crate::observability::AiToolOutcome;
use crate::suggest::ai::prompt::change_schema;
use finplan_plan::suggest::Change;

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
        "description": "The path's steps in order, each a list of changes. A note's step object ({key, title, changes}) is accepted too.",
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

/// Why a call's steps could not be read: a fixed kind for logs, and the
/// message the model reads.
struct BadSteps {
    kind: &'static str,
    message: String,
}

impl BadSteps {
    fn shape(message: String) -> Self {
        Self {
            kind: "shape",
            message: format!("{message}. {STEPS_SHAPE}"),
        }
    }

    fn into_output(self) -> ToolOutput {
        let mut out = ToolOutput::error(AiToolOutcome::Invalid, self.message);
        out.problems = 1;
        out.problem_kinds = vec![self.kind];
        out
    }
}

const STEPS_SHAPE: &str = "Send {\"steps\": [[change, ...], ...]}: the path's steps in order, each a list of changes. A note's step objects ({\"key\", \"title\", \"changes\"}) are accepted as they are.";

/// A path's steps, read from `{"steps": [...]}` (each step a list of changes,
/// or a note's step object with `changes`) or a bare `{"changes": [...]}` as a
/// path of one step, and held to the caps. A change that does not parse is
/// named by its step and position, so the model can fix just that one.
fn read_steps(input: &Value) -> Result<Vec<Vec<Change>>, BadSteps> {
    let steps = match (input.get("steps"), input.get("changes")) {
        (Some(Value::Array(steps)), _) if !steps.is_empty() => steps.clone(),
        (None | Some(Value::Array(_)), Some(changes)) => vec![changes.clone()],
        (Some(Value::Array(_)) | None, None) => {
            return Err(BadSteps::shape("no steps were sent".into()));
        }
        (Some(other), _) => {
            return Err(BadSteps::shape(format!(
                "`steps` must be a list, not {}",
                json_kind(other)
            )));
        }
    };
    if steps.len() > MAX_STEPS {
        return Err(BadSteps {
            kind: "limits",
            message: format!("send 1 to {MAX_STEPS} steps; {} were sent", steps.len()),
        });
    }
    let mut out = Vec::with_capacity(steps.len());
    for (i, step) in steps.iter().enumerate() {
        let n = i + 1;
        let changes = match step {
            Value::Array(changes) => changes,
            Value::Object(obj) => match obj.get("changes") {
                Some(Value::Array(changes)) => changes,
                _ => {
                    return Err(BadSteps::shape(format!(
                        "step {n} is an object without a `changes` list"
                    )));
                }
            },
            other => {
                return Err(BadSteps::shape(format!(
                    "step {n} is {}, not a list of changes",
                    json_kind(other)
                )));
            }
        };
        if changes.is_empty() || changes.len() > MAX_CHANGES {
            return Err(BadSteps {
                kind: "limits",
                message: format!(
                    "step {n} has {} changes; each step takes 1 to {MAX_CHANGES}",
                    changes.len()
                ),
            });
        }
        let mut parsed = Vec::with_capacity(changes.len());
        for (j, change) in changes.iter().enumerate() {
            match serde_json::from_value::<Change>(change.clone()) {
                Ok(c) => parsed.push(c),
                Err(e) => {
                    return Err(BadSteps {
                        kind: "parse",
                        message: format!(
                            "step {n}, change {}: {e}. A change is {{\"op\": \"replace\" | \"add\" | \"remove\", \"target\": {{\"event\": 12}}, \"path\": \"/amount\", \"expect\": <current value>, \"value\": <new value>}}; the target has exactly one key and an existing id is an integer.",
                            j + 1
                        ),
                    });
                }
            }
        }
        out.push(parsed);
    }
    Ok(out)
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
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
    let steps = match read_steps(input) {
        Ok(steps) => steps,
        Err(bad) => return bad.into_output(),
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
    let parsed = match input.get("paths") {
        Some(Value::Array(paths)) => paths,
        _ => {
            return BadSteps::shape(
                "send {\"paths\": [{\"key\": ..., \"steps\": [...]}, ...]}".into(),
            )
            .into_output();
        }
    };
    if !(MIN_PATHS..=MAX_PATHS).contains(&parsed.len()) {
        let mut out = ToolOutput::error(
            AiToolOutcome::Invalid,
            format!("send {MIN_PATHS} to {MAX_PATHS} paths; for one path use preview_changes"),
        );
        out.problems = 1;
        out.problem_kinds = vec!["limits"];
        return out;
    }
    let mut paths = Vec::with_capacity(parsed.len());
    for (i, p) in parsed.iter().enumerate() {
        let key = p
            .get("key")
            .and_then(Value::as_str)
            .map_or_else(|| format!("path-{}", i + 1), str::to_owned);
        match read_steps(p) {
            Ok(steps) => paths.push((key, steps)),
            Err(mut bad) => {
                bad.message = format!("path {} ({key}): {}", i + 1, bad.message);
                return bad.into_output();
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
    let steps = match read_steps(input) {
        Ok(steps) => steps,
        Err(bad) => return bad.into_output(),
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
            for problem in &problems {
                let kind = problem.log_kind();
                if !out.problem_kinds.contains(&kind) {
                    out.problem_kinds.push(kind);
                }
            }
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
