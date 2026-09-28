//! Paths and steps: the shape rules a suggestion's courses of action follow,
//! and walking a path's steps in order.
//!
//! A path is a sequence of steps, each a batch of [`Change`]s. Step k reads
//! the plan as steps 1..k-1 left it; an entity a step creates stays in scope
//! for the rest of its path, across requests too (steps applied one at a
//! time), through the [`Created`] ids stored with the suggestion. The walking
//! itself is `suggest::resolve_steps` / `suggest::apply_steps_sql`; this
//! module holds the shape rules and the in-memory walk the routes share.

use std::collections::HashSet;

use super::preview;
use crate::compile::{self, rows::ScenarioGraph};
use crate::db::Db;
use crate::error::ApiResult;
use crate::suggest::rules::Kind;
use crate::suggest::{self, Change, ChangeProblem, ChangeTarget, Created, DiffLine};

use super::suggestions::{MAX_PATHS, MAX_STEPS};

const MAX_KEY: usize = 32;
const MAX_LABEL: usize = 80;
const MAX_REASONING: usize = 4_000;

// ── shape ───────────────────────────────────────────────────────────────────

/// The parts of a path its shape rules read, from whichever type wrote it.
pub(crate) struct PathShape<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub reasoning: Option<&'a str>,
    pub recommended: bool,
    pub steps: Vec<StepShape<'a>>,
}

pub(crate) struct StepShape<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub reasoning: Option<&'a str>,
    pub changes: &'a [Change],
}

/// Whether `key` may name a path or step: 1-32 of `a-z`, `0-9`, `-`.
pub(crate) fn valid_key(key: &str) -> bool {
    (1..=MAX_KEY).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn one_line(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && text.chars().count() <= MAX_LABEL && !text.contains('\n')
}

/// Every way a set of paths breaks the shape rules, whoever wrote it. A read
/// note offers no paths; a fix or stress note at least one; at most
/// [`MAX_PATHS`], at most one recommended, no two making the same changes.
/// Each path has a unique key, a one-line label and 1 to [`MAX_STEPS`] steps;
/// each step a key unique in its path, a one-line title and 1 to
/// `max_changes` changes. Empty when the set is sound.
pub(crate) fn shape_problems(
    kind: Kind,
    paths: &[PathShape<'_>],
    max_changes: usize,
) -> Vec<String> {
    let mut problems = Vec::new();
    match kind {
        Kind::Read if !paths.is_empty() => problems.push("a read note offers no paths".into()),
        Kind::Fix | Kind::Stress if paths.is_empty() => problems.push(
            "fix and stress notes need at least one path; make it a check or read note otherwise"
                .into(),
        ),
        _ => {}
    }
    if paths.len() > MAX_PATHS {
        problems.push(format!("a note offers at most {MAX_PATHS} paths"));
    }
    if paths.iter().filter(|p| p.recommended).count() > 1 {
        problems.push("at most one path is recommended".into());
    }
    let mut keys = HashSet::new();
    let mut batches: Vec<(String, &str)> = Vec::new();
    for path in paths {
        let key = path.key;
        if !valid_key(key) {
            problems.push(format!(
                "path key \"{key}\" must be 1 to {MAX_KEY} of a-z, 0-9 and -"
            ));
        } else if !keys.insert(key) {
            problems.push(format!("path key \"{key}\" is used twice"));
        }
        if !one_line(path.label) {
            problems.push(format!(
                "path \"{key}\": the label is one line of 1 to {MAX_LABEL} characters"
            ));
        }
        if path
            .reasoning
            .is_some_and(|r| r.trim().chars().count() > MAX_REASONING)
        {
            problems.push(format!(
                "path \"{key}\": reasoning is at most {MAX_REASONING} characters"
            ));
        }
        if path.steps.is_empty() || path.steps.len() > MAX_STEPS {
            problems.push(format!("path \"{key}\" takes 1 to {MAX_STEPS} steps"));
        }
        let mut step_keys = HashSet::new();
        for step in &path.steps {
            let at = format!("path \"{key}\", step \"{}\"", step.key);
            if !valid_key(step.key) {
                problems.push(format!(
                    "{at}: a step key is 1 to {MAX_KEY} of a-z, 0-9 and -"
                ));
            } else if !step_keys.insert(step.key) {
                problems.push(format!("{at}: the step key is used twice in the path"));
            }
            if !one_line(step.title) {
                problems.push(format!(
                    "{at}: the title is one line of 1 to {MAX_LABEL} characters"
                ));
            }
            if step
                .reasoning
                .is_some_and(|r| r.trim().chars().count() > MAX_REASONING)
            {
                problems.push(format!(
                    "{at}: reasoning is at most {MAX_REASONING} characters"
                ));
            }
            if step.changes.is_empty() || step.changes.len() > max_changes {
                problems.push(format!("{at}: a step carries 1 to {max_changes} changes"));
            }
        }
        // The same edits, in whatever steps and order, are the same course
        // of action.
        let mut batch: Vec<String> = path
            .steps
            .iter()
            .flat_map(|s| s.changes.iter())
            .map(|c| serde_json::to_string(c).unwrap_or_default())
            .collect();
        batch.sort();
        let batch = batch.join("\u{1f}");
        if let Some((_, twin)) = batches.iter().find(|(b, _)| *b == batch) {
            problems.push(format!(
                "paths \"{twin}\" and \"{key}\" make the same changes; offer genuinely different courses of action"
            ));
        } else {
            batches.push((batch, key));
        }
    }
    problems
}

// ── walking a path ──────────────────────────────────────────────────────────

/// One step to walk: its key and its changes as written.
pub(crate) struct WalkStep<'a> {
    pub key: &'a str,
    pub changes: &'a [Change],
}

/// A path walked in memory: each step's diff, read after the steps before it.
pub(crate) struct Walked {
    pub diffs: Vec<Vec<DiffLine>>,
}

/// Why a step of a walk could not be taken.
#[derive(Debug, Clone)]
pub(crate) struct StepFailure {
    pub step: String,
    pub problems: Vec<ChangeProblem>,
}

/// Walk `steps` in order from `graph` (see [`suggest::resolve_steps`]), with
/// what earlier-applied steps created (`seeded`) in scope. Return profiles the
/// steps name that the graph lacks (a snapshot keeps only its run's) are
/// loaded read-only first, so the steps resolve and diffs read them by name.
/// The outer error is the server's; the inner one names the step that failed
/// and why.
pub(crate) async fn walk(
    db: &Db,
    user_id: &str,
    mut graph: ScenarioGraph,
    steps: &[WalkStep<'_>],
    seeded: &Created,
) -> ApiResult<Result<Walked, StepFailure>> {
    let named = suggest::profiles_named(steps.iter().flat_map(|s| s.changes.iter()));
    preview::load_profiles(db, user_id, &mut graph, named.into_iter().collect()).await?;
    let batches: Vec<Vec<Change>> = steps.iter().map(|s| s.changes.to_vec()).collect();
    let stepped = match suggest::resolve_steps(&graph, &batches, seeded)? {
        Ok(stepped) => stepped,
        Err(failed) => {
            return Ok(Err(StepFailure {
                step: steps
                    .get(failed.step)
                    .map_or_else(String::new, |s| s.key.to_string()),
                problems: failed.problems,
            }));
        }
    };
    // The plan every step leaves must compile; a failure is the last step's.
    if let Err(err) = compile::compile(&stepped.graph)
        && let Some(last) = steps.last()
    {
        let change = last.changes.len().saturating_sub(1);
        let target = last
            .changes
            .last()
            .map_or(ChangeTarget::NewEvent(String::new()), |c| c.target.clone());
        return Ok(Err(StepFailure {
            step: last.key.to_string(),
            problems: vec![suggest::plan_problem(err, change, target)?],
        }));
    }
    Ok(Ok(Walked {
        diffs: stepped
            .steps
            .iter()
            .map(|step| step.diff(std::iter::empty()))
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn change(value: serde_json::Value) -> Change {
        serde_json::from_value(value).expect("a change")
    }

    fn step<'a>(key: &'a str, changes: &'a [Change]) -> StepShape<'a> {
        StepShape {
            key,
            title: "Do it",
            reasoning: None,
            changes,
        }
    }

    fn path<'a>(key: &'a str, recommended: bool, steps: Vec<StepShape<'a>>) -> PathShape<'a> {
        PathShape {
            key,
            label: "A way",
            reasoning: None,
            recommended,
            steps,
        }
    }

    #[test]
    fn keys_follow_the_rules() {
        assert!(valid_key("a"));
        assert!(valid_key("spend-less-2"));
        assert!(!valid_key(""));
        assert!(!valid_key("Spend"));
        assert!(!valid_key("a b"));
        assert!(!valid_key(&"a".repeat(33)));
    }

    #[test]
    fn shapes_are_checked() {
        let one = [change(
            json!({"op": "remove", "target": {"event": 5}, "path": "/effects/0"}),
        )];
        let two = [change(
            json!({"op": "remove", "target": {"event": 6}, "path": "/effects/0"}),
        )];
        let ok = [
            path("a", true, vec![step("a", &one)]),
            path("b", false, vec![step("a", &two)]),
        ];
        assert!(shape_problems(Kind::Fix, &ok, 20).is_empty());

        assert_eq!(shape_problems(Kind::Fix, &[], 20).len(), 1);
        assert_eq!(shape_problems(Kind::Read, &ok, 20).len(), 1);
        assert!(shape_problems(Kind::Check, &[], 20).is_empty());

        let twins = [
            path("a", false, vec![step("a", &one)]),
            path("b", false, vec![step("x", &one)]),
        ];
        assert!(shape_problems(Kind::Fix, &twins, 20)[0].contains("same changes"));

        let two_recommended = [
            path("a", true, vec![step("a", &one)]),
            path("b", true, vec![step("a", &two)]),
        ];
        assert!(shape_problems(Kind::Fix, &two_recommended, 20)[0].contains("recommended"));

        let dup_keys = [
            path("a", false, vec![step("a", &one)]),
            path("a", false, vec![step("a", &two)]),
        ];
        assert!(shape_problems(Kind::Fix, &dup_keys, 20)[0].contains("used twice"));

        let dup_steps = [path("a", false, vec![step("s", &one), step("s", &two)])];
        assert!(shape_problems(Kind::Fix, &dup_steps, 20)[0].contains("used twice"));

        let five: Vec<_> = (0..5).map(|_| step("s", &one)).collect();
        let long = [path("a", false, five)];
        assert!(
            shape_problems(Kind::Fix, &long, 20)
                .iter()
                .any(|p| p.contains("1 to 4 steps"))
        );

        let empty: [Change; 0] = [];
        let no_changes = [path("a", false, vec![step("a", &empty)])];
        assert!(shape_problems(Kind::Fix, &no_changes, 20)[0].contains("1 to 20 changes"));
        assert!(
            shape_problems(Kind::Fix, &ok, 0)
                .iter()
                .any(|p| p.contains("1 to 0"))
        );

        let mut bad_label = path("a", false, vec![step("a", &one)]);
        bad_label.label = "two\nlines";
        assert!(shape_problems(Kind::Fix, &[bad_label], 20)[0].contains("label"));

        let many: Vec<_> = ["a", "b", "c", "d", "e"]
            .into_iter()
            .map(|k| path(k, false, vec![step("a", &one)]))
            .collect();
        assert!(
            shape_problems(Kind::Fix, &many, 20)
                .iter()
                .any(|p| p.contains("at most 4 paths"))
        );
    }
}
