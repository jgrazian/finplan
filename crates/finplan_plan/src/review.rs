//! The Review tab's notes as the web reads them, and the pure review of a plan
//! that needs no server.
//!
//! [`Suggestion`] is the wire shape of one note, whoever wrote it; the server
//! stores them as rows, and a local plan gets them from [`local_review`], which
//! runs the rule-based checks (`crate::rules`) over the plan's graph and one
//! run's results and writes each finding up the way the server's
//! `POST /scenarios/{id}/review` does, minus what only a server has: stored
//! ids and history, simulated checks, and the model-written pass.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::compile;
use crate::error::PlanResult;
use crate::graph::ScenarioGraph;
use crate::results::RunResults;
use crate::results::funding::FundingDiagnostics;
use crate::results::view::Results;
use crate::rules::{self, Evidence, Kind, ReviewCheck, Section};
use crate::suggest::{self, Change, Created, DiffLine};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PreviewStats {
    pub success_rate: f64,
    pub funding_success_rate: Option<f64>,
    /// Final net worth in today's dollars, over all iterations.
    pub real_final: Option<RealFinal>,
    pub funding: Option<FundingDiagnostics>,
    /// The median after-tax ending balance over all iterations, nominal:
    /// tax-deferred balances count at one minus the plan's deferred tax rate.
    /// What a Roth conversion is judged on, where success rarely moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after_tax_final: Option<f64>,
    /// Lifetime tax, early-withdrawal penalties included, on the median path,
    /// nominal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lifetime_taxes: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
#[ts(export)]
pub struct RealFinal {
    pub p5: f64,
    pub p10: f64,
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
    pub p90: f64,
    pub p95: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SuggestionStatus {
    Open,
    Applied,
    Dismissed,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SuggestionSource {
    Rules,
    Ai,
}

/// What the author expects the changes to do, before anything is simulated.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionEstimate {
    #[serde(default)]
    pub success_rate: Option<f64>,
    #[serde(default)]
    pub funding_success_rate: Option<f64>,
}

/// The changes simulated against the suggestion's run: the last preview.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionCheck {
    pub iterations: usize,
    pub paired: bool,
    pub base: PreviewStats,
    pub edited: PreviewStats,
}

/// One step of a path: a self-contained batch of changes the user could stop
/// after.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionStep {
    /// Unique within its path: 1-32 of `a-z`, `0-9`, `-`.
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    pub changes: Vec<Change>,
    /// `changes` as the server renders them, read after the path's earlier
    /// steps.
    #[serde(default)]
    pub diff: Vec<DiffLine>,
    #[serde(default)]
    pub applied: bool,
    /// When it was applied (UTC, `YYYY-MM-DD HH:MM:SS` like `resolved_at`);
    /// steps applied in one request share it.
    #[serde(default)]
    pub applied_at: Option<String>,
}

/// One course of action: ordered steps, and what all of them together are
/// expected (`estimate`) or simulated (`check`) to do.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionPath {
    /// Unique within the suggestion: 1-32 of `a-z`, `0-9`, `-`.
    pub key: String,
    pub label: String,
    /// Why this path rather than the others; null when the note's own
    /// reasoning covers it.
    #[serde(default)]
    pub reasoning: Option<String>,
    /// At most one path is; with none, clients treat the first as default.
    #[serde(default)]
    pub recommended: bool,
    /// 1 to [`MAX_STEPS`], in the order they apply.
    pub steps: Vec<SuggestionStep>,
    #[serde(default)]
    pub estimate: Option<SuggestionEstimate>,
    /// The last preview of every step together. A check from an older shape
    /// of the stats is dropped when read, not fatal: previewing again
    /// rewrites it.
    #[serde(default, deserialize_with = "lenient")]
    pub check: Option<SuggestionCheck>,
}

fn lenient<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: for<'a> Deserialize<'a>,
{
    let value = Option::<serde_json::Value>::deserialize(de)?;
    Ok(value.and_then(|v| serde_json::from_value(v).ok()))
}

impl SuggestionPath {
    /// Every change of every step, in order: the path as one batch.
    pub fn all_changes(&self) -> Vec<Change> {
        self.steps
            .iter()
            .flat_map(|s| s.changes.iter().cloned())
            .collect()
    }

    /// The path a client shows first: the recommended one, else the first.
    pub fn default_of(paths: &[SuggestionPath]) -> Option<&SuggestionPath> {
        paths
            .iter()
            .find(|p| p.recommended)
            .or_else(|| paths.first())
    }
}

/// Where a note of a draft groups on the Review board (design 2c): the
/// accounts and holdings, the events and parameters, or what the user still has
/// to confirm. A hint from the drafting agent; null on every other note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DraftColumn {
    Portfolio,
    Plan,
    ToConfirm,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Suggestion {
    pub id: i64,
    pub scenario_id: i64,
    /// The run whose inputs and results the suggestion was written against;
    /// null on a note written for a draft, which has no run yet.
    pub run_id: Option<i64>,
    pub source: SuggestionSource,
    /// The rule that wrote it; null for model-written suggestions.
    pub rule: Option<String>,
    pub kind: Kind,
    pub section: Section,
    pub title: String,
    /// The note's lead: one sentence on what it found and why it matters,
    /// shown under the title. Null on notes stored before authors wrote one.
    pub summary: Option<String>,
    /// The working behind it — figures, assumptions, what to check — which
    /// the Review tab keeps behind a disclosure.
    pub reasoning: String,
    pub evidence: Vec<Evidence>,
    /// The courses of action, in the author's order; empty on a read note.
    pub paths: Vec<SuggestionPath>,
    /// The path being followed, once any of its steps is applied; the other
    /// paths are then closed. `status` turns `applied` when every step of it
    /// is.
    pub applied_path: Option<String>,
    pub status: SuggestionStatus,
    pub created_at: String,
    pub resolved_at: Option<String>,
    /// The note whose "Chat about this" thread the model wrote this one from
    /// (see `suggestion_chat`); null otherwise, or once that note is gone.
    pub parent_id: Option<i64>,
    /// A draft note's own key, which a question's `blocks` names; null
    /// otherwise.
    pub note_key: Option<String>,
    /// The keys of the drafting agent's questions this note still waits on.
    /// While any is unanswered the note cannot be applied. Empty once they are
    /// answered, and on every other note.
    pub blocked_by: Vec<String>,
    /// Where the note groups on a draft's Review board.
    pub column: Option<DraftColumn>,
    /// The drafting agent applied this note itself, as a plain fact read from a
    /// document or answered by the user: it is `applied` and shows as "Added".
    pub auto_added: bool,
}

// ── fingerprints ────────────────────────────────────────────────────────────

/// A unit enum's wire tag, as stored in its TEXT column.
fn tag<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// What a suggestion is about, independent of the numbers it quotes: the
/// author (rule, or "ai"), the kind, and either the fields its changes touch
/// (every step of every path together) or — for a note without changes — its
/// title with every figure blanked. "USAA empties in 97 of 99 failures" and
/// "… 12 of 14 …" are one note.
pub fn fingerprint<'a>(
    rule: Option<&str>,
    kind: Kind,
    changes: impl IntoIterator<Item = &'a Change>,
    title: &str,
) -> String {
    let mut touched: Vec<String> = changes
        .into_iter()
        .map(|c| {
            format!(
                "{}{}{}",
                target_key(&c.target),
                c.created_signature(),
                c.path
            )
        })
        .collect();
    touched.sort();
    touched.dedup();

    let mut hash = Sha256::new();
    hash.update(rule.unwrap_or("ai"));
    hash.update([0]);
    hash.update(tag(&kind));
    hash.update([0]);
    if touched.is_empty() {
        hash.update(blank_figures(title));
    } else {
        for key in touched {
            hash.update(key);
            hash.update([0]);
        }
    }
    format!("{:x}", hash.finalize())
}

/// A change's target as the fingerprint reads it. An entity the batch creates
/// (`new_event`, `new_asset`, …) is named by its kind alone: its key is the
/// author's own label, not something the note is about.
fn target_key(target: &crate::suggest::ChangeTarget) -> String {
    match serde_json::to_value(target) {
        Ok(serde_json::Value::Object(map)) if map.len() == 1 => {
            let (kind, id) = map.into_iter().next().expect("one entry");
            if kind.starts_with("new_") {
                kind
            } else {
                format!("{{\"{kind}\":{id}}}")
            }
        }
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

/// Each run of digits (with the separators inside a figure) becomes `#`.
pub fn blank_figures(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            while chars
                .peek()
                .is_some_and(|n| n.is_ascii_digit() || *n == ',' || *n == '.')
            {
                chars.next();
            }
            out.push('#');
        } else {
            out.push(c);
        }
    }
    out
}

// ── local review ────────────────────────────────────────────────────────────

/// A plan's rule-based review: `Review` (what `GET /scenarios/{id}/review`
/// serves) without its `ai` field.
///
/// The mapping for a client: a local plan's Review tab renders
/// `{ ...localReview, ai: null }` as a `Review`. Every note is `open`, from
/// source `rules`, with ids numbered from 1 in board order; ids are only
/// stable within one review, so a local store keys what the user did to a
/// note (dismissed, applied) by [`suggestion_fingerprint`], and passes the
/// silenced ones back in on the next review.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LocalReview {
    pub run_id: i64,
    pub reviewed_at: String,
    pub suggestions: Vec<Suggestion>,
    /// The standard checks, each rule's counted over `suggestions`.
    pub checks: Vec<ReviewCheck>,
}

/// What a note is about, independent of the figures it quotes: the key a local
/// store records a dismissal under, and the one [`local_review`] silences by.
#[must_use]
pub fn suggestion_fingerprint(note: &Suggestion) -> String {
    let changes: Vec<Change> = note
        .paths
        .iter()
        .flat_map(SuggestionPath::all_changes)
        .collect();
    fingerprint(note.rule.as_deref(), note.kind, &changes, &note.title)
}

/// Review a finished run of `graph`'s plan with the rule-based checks: no
/// server, no clock (`reviewed_at` is whatever the caller stamps it with).
///
/// `graph` is the plan the run was made from (its snapshot), `run` its
/// projected results, and `silenced` the fingerprints of notes the user has
/// set aside. Each note's courses of action are walked in memory against the
/// graph, so their diffs are rendered and a path that no longer fits the plan
/// is left out, as the server's review does. Simulated checks (`check`) and
/// estimates are not computed here; a client may preview a path on its own
/// runner.
pub fn local_review(
    graph: &ScenarioGraph,
    run: &RunResults,
    run_id: i64,
    reviewed_at: &str,
    silenced: &HashSet<String>,
) -> PlanResult<LocalReview> {
    let results = run.results(run_id, graph.scenario.id, None)?;
    review_results(graph, &results, run_id, reviewed_at, silenced)
}

/// [`local_review`] from results already projected.
pub fn review_results(
    graph: &ScenarioGraph,
    results: &Results,
    run_id: i64,
    reviewed_at: &str,
    silenced: &HashSet<String>,
) -> PlanResult<LocalReview> {
    let mut seen = HashSet::new();
    let mut suggestions = Vec::new();
    for draft in rules::review(graph, results) {
        let print = fingerprint(
            Some(draft.rule),
            draft.kind,
            draft.paths.iter().flat_map(|p| p.changes()),
            &draft.title,
        );
        if silenced.contains(&print) || !seen.insert(print) {
            continue;
        }
        let had_paths = !draft.paths.is_empty();
        let mut paths = Vec::with_capacity(draft.paths.len());
        for path in draft.paths {
            let batches: Vec<Vec<Change>> = path.steps.iter().map(|s| s.changes.clone()).collect();
            let Ok(stepped) = suggest::resolve_steps(graph, &batches, &Created::new())? else {
                // Rules only write changes that resolve against the graph they
                // read; one that does not is a rule bug, and the path is left out.
                continue;
            };
            if compile::compile(&stepped.graph).is_err() {
                continue;
            }
            paths.push(SuggestionPath {
                key: path.key.to_string(),
                label: path.label,
                reasoning: path.reasoning,
                recommended: path.recommended,
                steps: path
                    .steps
                    .into_iter()
                    .zip(&stepped.steps)
                    .map(|(step, walked)| SuggestionStep {
                        key: step.key.to_string(),
                        title: step.title,
                        reasoning: step.reasoning,
                        changes: step.changes,
                        diff: walked.diff(std::iter::empty()),
                        applied: false,
                        applied_at: None,
                    })
                    .collect(),
                estimate: None,
                check: None,
            });
        }
        if had_paths && paths.is_empty() {
            continue;
        }
        suggestions.push(Suggestion {
            id: suggestions.len() as i64 + 1,
            scenario_id: graph.scenario.id,
            run_id: Some(run_id),
            source: SuggestionSource::Rules,
            rule: Some(draft.rule.to_string()),
            kind: draft.kind,
            section: draft.section,
            title: draft.title,
            summary: Some(draft.summary),
            reasoning: draft.reasoning,
            evidence: draft.evidence,
            paths,
            applied_path: None,
            status: SuggestionStatus::Open,
            created_at: reviewed_at.to_string(),
            resolved_at: None,
            parent_id: None,
            note_key: None,
            blocked_by: Vec::new(),
            column: None,
            auto_added: false,
        });
    }
    Ok(LocalReview {
        run_id,
        reviewed_at: reviewed_at.to_string(),
        checks: rules::review_checks(suggestions.iter().filter_map(|s| s.rule.as_deref())),
        suggestions,
    })
}
