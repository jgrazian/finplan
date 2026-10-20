//! The AI spend ledger: one `ai_usage` row per model pass, for the admin
//! listener's token and cost totals (`admin`).
//!
//! A [`Ledger`] rides along on a pass's `PassMetrics`, adds up each answered
//! turn, and writes its row when it is dropped — so a pass that fails, or is
//! aborted because a newer one replaced it, is still charged for the turns it
//! had answered. The per-feature usage columns (`suggestion_reviews.ai_*`,
//! `draft_jobs`, the chat threads) describe only the latest pass and are
//! overwritten; this table is the history.

use std::sync::Mutex;

use crate::db::Db;
use crate::suggest::ai::TurnReport;

/// What the pass was for; `ai_usage.feature`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Review,
    NoteChat,
    PlanChat,
    Draft,
}

impl Feature {
    pub const ALL: [Self; 4] = [Self::Review, Self::NoteChat, Self::PlanChat, Self::Draft];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::NoteChat => "note_chat",
            Self::PlanChat => "plan_chat",
            Self::Draft => "draft",
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Tally {
    turns: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    cost_usd: f64,
    unpriced_turns: i64,
}

/// One pass's spend, written to `ai_usage` when dropped.
pub struct Ledger {
    db: Db,
    feature: Feature,
    user_id: String,
    scenario_id: i64,
    model: String,
    request_id: Option<String>,
    tally: Mutex<Tally>,
}

impl Ledger {
    pub fn new(
        db: &Db,
        feature: Feature,
        user_id: &str,
        scenario_id: i64,
        model: &str,
        request_id: Option<String>,
    ) -> Self {
        Self {
            db: db.clone(),
            feature,
            user_id: user_id.to_owned(),
            scenario_id,
            model: model.to_owned(),
            request_id,
            tally: Mutex::default(),
        }
    }

    /// Add one answered model request.
    pub fn turn(&self, turn: &TurnReport) {
        let mut tally = self.tally.lock().unwrap_or_else(|e| e.into_inner());
        tally.turns += 1;
        tally.input_tokens += turn.input_tokens as i64;
        tally.output_tokens += turn.output_tokens as i64;
        tally.cache_read_tokens += turn.cache_read_input_tokens as i64;
        tally.cache_write_tokens += turn.cache_creation_input_tokens as i64;
        match turn.cost {
            Some((usd, _)) => tally.cost_usd += usd,
            None => tally.unpriced_turns += 1,
        }
    }
}

impl Drop for Ledger {
    /// A pass that never got an answer spent nothing and writes nothing.
    /// Dropping is synchronous, so the row is written on a task of its own.
    fn drop(&mut self) {
        let tally = std::mem::take(self.tally.get_mut().unwrap_or_else(|e| e.into_inner()));
        if tally.turns == 0 {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                event = "ai_usage.write_failed",
                feature = self.feature.as_str(),
                reason = "no_runtime"
            );
            return;
        };
        let db = self.db.clone();
        let feature = self.feature;
        let user_id = std::mem::take(&mut self.user_id);
        let scenario_id = self.scenario_id;
        let model = std::mem::take(&mut self.model);
        let request_id = self.request_id.take();
        runtime.spawn(async move {
            let written = sqlx::query(
                "INSERT INTO ai_usage (user_id, feature, scenario_id, model, request_id, turns,
                                       input_tokens, output_tokens, cache_read_tokens,
                                       cache_write_tokens, cost_usd, unpriced_turns)
                 VALUES ((SELECT id FROM users WHERE id = ?1), ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                         ?10, ?11, ?12)",
            )
            .bind(&user_id)
            .bind(feature.as_str())
            .bind(scenario_id)
            .bind(&model)
            .bind(&request_id)
            .bind(tally.turns)
            .bind(tally.input_tokens)
            .bind(tally.output_tokens)
            .bind(tally.cache_read_tokens)
            .bind(tally.cache_write_tokens)
            .bind(tally.cost_usd)
            .bind(tally.unpriced_turns)
            .execute(&db)
            .await;
            if written.is_err() {
                tracing::warn!(
                    event = "ai_usage.write_failed",
                    feature = feature.as_str(),
                    cost_usd = tally.cost_usd,
                    reason = "database"
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(cost: Option<f64>) -> TurnReport {
        TurnReport {
            turn: 1,
            seconds: 0.5,
            input_tokens: 1000,
            output_tokens: 200,
            cache_read_input_tokens: 50,
            cache_creation_input_tokens: 10,
            cost: cost.map(|usd| (usd, crate::observability::AiCostSource::Reported)),
        }
    }

    async fn rows(db: &Db) -> Vec<(Option<String>, String, i64, i64, f64, i64)> {
        for _ in 0..100 {
            let found: Vec<(Option<String>, String, i64, i64, f64, i64)> = sqlx::query_as(
                "SELECT user_id, feature, turns, input_tokens, cost_usd, unpriced_turns
                   FROM ai_usage ORDER BY id",
            )
            .fetch_all(db)
            .await
            .unwrap();
            if !found.is_empty() {
                return found;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        Vec::new()
    }

    #[tokio::test]
    async fn a_dropped_pass_writes_its_turns_once() {
        let db = crate::db::connect("sqlite::memory:", 1).await.unwrap();
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES ('u','u@example.com','!')")
            .execute(&db)
            .await
            .unwrap();
        let ledger = Ledger::new(&db, Feature::PlanChat, "u", 7, "test/model", None);
        ledger.turn(&turn(Some(0.01)));
        ledger.turn(&turn(None));
        drop(ledger);
        let found = rows(&db).await;
        assert_eq!(
            found,
            vec![(Some("u".into()), "plan_chat".into(), 2, 2000, 0.01, 1)]
        );
    }

    #[tokio::test]
    async fn a_pass_without_turns_writes_nothing_and_a_deleted_user_keeps_the_spend() {
        let db = crate::db::connect("sqlite::memory:", 1).await.unwrap();
        drop(Ledger::new(&db, Feature::Review, "u", 1, "m", None));
        // The user is already gone: the row is kept, unowned.
        let ledger = Ledger::new(&db, Feature::Draft, "gone", 1, "m", None);
        ledger.turn(&turn(Some(0.5)));
        drop(ledger);
        let found = rows(&db).await;
        assert_eq!(found, vec![(None, "draft".into(), 1, 1000, 0.5, 0)]);
    }
}
