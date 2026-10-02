//! What a running model loop has done so far — a chat turn's or a review
//! pass's — for whoever is watching it: the tools it called, what it said on
//! the way, and its retries.
//!
//! In memory only, keyed by what the loop is working on (a chat thread, a
//! scenario's review). A restart fails every running turn and pass anyway,
//! so there is nothing worth keeping across one.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use ts_rs::TS;

use super::review_ai::PassMetrics;
use crate::observability::{AiMotive, AiRetryReason, AiTool, AiToolOutcome};
use crate::suggest::ai::{Observer, TurnReport};

/// Most steps a loop keeps; older ones fall off the front.
const MAX_STEPS: usize = 40;
/// Longest narration kept, in characters.
const MAX_NARRATION: usize = 280;

/// One thing a running model loop has done, oldest first: what a chat thread
/// or a review shows in place of a bare "Thinking…".
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum AiStep {
    /// A model request; `done` once its reply is in.
    Thinking { done: bool },
    /// A tool call, by the name the model called it by.
    Tool {
        name: String,
        done: bool,
        /// The model was handed an error, which it may well recover from.
        failed: bool,
    },
    /// What the model said alongside its tool calls, on the way to its answer.
    Narration { text: String },
    /// A request sent again after the provider failed it (`rate_limit`,
    /// `timeout`, `network`, …).
    Retry { reason: String },
}

/// One loop's log: the generation that started it, and its steps.
type Entry = (u64, Vec<AiStep>);

/// Each running loop's steps, by what it works on. A loop's entry carries
/// the generation that started it, so a superseded loop winding down cannot
/// clear the log of the one that replaced it.
pub struct Activity<K> {
    entries: Arc<Mutex<HashMap<K, Entry>>>,
    generations: Arc<AtomicU64>,
}

impl<K> Clone for Activity<K> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            generations: self.generations.clone(),
        }
    }
}

impl<K> Default for Activity<K> {
    fn default() -> Self {
        Self {
            entries: Arc::default(),
            generations: Arc::default(),
        }
    }
}

impl<K: Copy + Eq + Hash> Activity<K> {
    /// Start `key`'s log afresh; it is dropped with the returned guard,
    /// however the loop ends — finished, failed, or aborted.
    pub fn begin(&self, key: K) -> ActivityGuard<K> {
        let generation = self.generations.fetch_add(1, Ordering::Relaxed);
        self.lock().insert(key, (generation, Vec::new()));
        ActivityGuard {
            activity: self.clone(),
            key,
            generation,
        }
    }

    /// `key`'s steps so far; empty when nothing is running for it.
    pub fn steps(&self, key: K) -> Vec<AiStep> {
        self.lock()
            .get(&key)
            .map(|(_, steps)| steps.clone())
            .unwrap_or_default()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<K, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Change `key`'s log, if a loop is running for it.
    fn with(&self, key: K, change: impl FnOnce(&mut Vec<AiStep>)) {
        if let Some((_, steps)) = self.lock().get_mut(&key) {
            change(steps);
        }
    }

    fn push(&self, key: K, step: AiStep) {
        self.with(key, |steps| {
            steps.push(step);
            let over = steps.len().saturating_sub(MAX_STEPS);
            steps.drain(..over);
        });
    }
}

pub struct ActivityGuard<K: Copy + Eq + Hash> {
    activity: Activity<K>,
    key: K,
    generation: u64,
}

impl<K: Copy + Eq + Hash> Drop for ActivityGuard<K> {
    fn drop(&mut self) {
        let mut entries = self.activity.lock();
        if entries
            .get(&self.key)
            .is_some_and(|(g, _)| *g == self.generation)
        {
            entries.remove(&self.key);
        }
    }
}

/// A loop's observer: the review metrics, plus its activity log.
pub(super) struct ActivityObserver<'a, K> {
    metrics: &'a PassMetrics,
    activity: &'a Activity<K>,
    key: K,
}

impl<'a, K> ActivityObserver<'a, K> {
    pub(super) fn new(metrics: &'a PassMetrics, activity: &'a Activity<K>, key: K) -> Self {
        Self {
            metrics,
            activity,
            key,
        }
    }
}

impl<K: Copy + Eq + Hash + Send + Sync> Observer for ActivityObserver<'_, K> {
    fn turn(&self, turn: &TurnReport) {
        self.metrics.turn(turn);
        self.activity.with(self.key, |steps| {
            if let Some(AiStep::Thinking { done }) = steps
                .iter_mut()
                .rev()
                .find(|s| matches!(s, AiStep::Thinking { .. }))
            {
                *done = true;
            }
        });
    }
    fn tool(&self, tool: AiTool, outcome: AiToolOutcome, seconds: f64) {
        self.metrics.tool(tool, outcome, seconds);
    }
    fn retry(&self, reason: AiRetryReason) {
        self.metrics.retry(reason);
        self.activity.push(
            self.key,
            AiStep::Retry {
                reason: reason.as_str().to_owned(),
            },
        );
    }
    fn submission(&self, accepted: bool) {
        self.metrics.submission(accepted);
    }
    fn motive(&self, motive: AiMotive, accepted: bool) {
        self.metrics.motive(motive, accepted);
    }
    fn thinking(&self) {
        self.activity
            .push(self.key, AiStep::Thinking { done: false });
    }
    fn tool_started(&self, name: &str) {
        self.activity.push(
            self.key,
            AiStep::Tool {
                name: name.to_owned(),
                done: false,
                failed: false,
            },
        );
    }
    fn tool_finished(&self, name: &str, is_error: bool) {
        self.activity.with(self.key, |steps| {
            let open = steps
                .iter_mut()
                .rev()
                .find(|s| matches!(s, AiStep::Tool { name: n, done: false, .. } if n == name));
            if let Some(AiStep::Tool { done, failed, .. }) = open {
                *done = true;
                *failed = is_error;
            }
        });
    }
    fn narration(&self, text: &str) {
        let text = text.trim();
        let text = if text.chars().count() > MAX_NARRATION {
            text.chars().take(MAX_NARRATION - 1).collect::<String>() + "…"
        } else {
            text.to_owned()
        };
        if !text.is_empty() {
            self.activity.push(self.key, AiStep::Narration { text });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::Telemetry;

    #[test]
    fn a_loop_logs_what_it_does_and_forgets_it_when_it_ends() {
        let activity = Activity::<i64>::default();
        let metrics = PassMetrics::new(&Telemetry::new(1), "test/model");
        let observer = ActivityObserver::new(&metrics, &activity, 7);
        let guard = activity.begin(7);
        observer.thinking();
        observer.narration(&format!("  {}  ", "x".repeat(400)));
        observer.tool_started("preview_changes");
        observer.retry(AiRetryReason::RateLimit);
        observer.tool_finished("preview_changes", true);

        let steps = activity.steps(7);
        assert_eq!(steps[0], AiStep::Thinking { done: false });
        let AiStep::Narration { text } = &steps[1] else {
            panic!("{steps:?}")
        };
        assert_eq!(text.chars().count(), MAX_NARRATION);
        assert!(text.ends_with('…'));
        assert_eq!(
            steps[2],
            AiStep::Tool {
                name: "preview_changes".into(),
                done: true,
                failed: true
            }
        );
        assert_eq!(
            steps[3],
            AiStep::Retry {
                reason: AiRetryReason::RateLimit.as_str().into()
            }
        );
        assert!(activity.steps(8).is_empty(), "keys are separate");

        drop(guard);
        assert!(activity.steps(7).is_empty());
        // Nothing running: reports go nowhere rather than starting a log.
        observer.thinking();
        assert!(activity.steps(7).is_empty());
    }

    #[test]
    fn a_superseded_loop_does_not_clear_its_replacement() {
        let activity = Activity::<i64>::default();
        let old = activity.begin(1);
        let new = activity.begin(1);
        activity.push(1, AiStep::Thinking { done: false });
        drop(old);
        assert_eq!(activity.steps(1).len(), 1, "the newer pass keeps its log");
        drop(new);
        assert!(activity.steps(1).is_empty());
    }

    #[test]
    fn a_long_loop_keeps_its_latest_steps() {
        let activity = Activity::<i64>::default();
        let _guard = activity.begin(1);
        for i in 0..(MAX_STEPS + 5) {
            activity.push(
                1,
                AiStep::Narration {
                    text: i.to_string(),
                },
            );
        }
        let steps = activity.steps(1);
        assert_eq!(steps.len(), MAX_STEPS);
        assert_eq!(steps[0], AiStep::Narration { text: "5".into() });
    }
}
