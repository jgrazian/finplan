//! HTTP routing.

pub mod accounts;
pub mod ai_activity;
pub mod analysis;
pub mod archives;
pub mod assets;
pub mod contact;
pub mod documents;
pub mod draft_agent;
pub mod drafts;
pub mod events;
pub(crate) mod expression_refs;
pub mod expressions;
pub mod onboarding;
pub mod parameters;
pub mod plan_chat;
pub mod preview;
pub mod profiles;
pub mod reports;
pub mod review_ai;
pub mod runs;
pub mod scenarios;
pub mod suggestion_chat;
pub(crate) mod suggestion_paths;
pub mod suggestions;
pub mod taxes;
pub mod what_if;

use axum::Router;
use axum::routing::get;
use finplan_plan::edit::reconcile;
use serde::Deserialize;
use ts_rs::TS;

use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router(config: &crate::config::ServerConfig) -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .nest("/auth", crate::auth::routes::router())
        .merge(scenarios::router())
        .merge(assets::router())
        .merge(accounts::router())
        .merge(events::router())
        .merge(expressions::router())
        .merge(parameters::router())
        .merge(profiles::router())
        .merge(taxes::router())
        .merge(runs::router())
        .merge(reports::router())
        .merge(analysis::router())
        .merge(what_if::router())
        .merge(preview::router())
        .merge(suggestions::router())
        .merge(suggestion_chat::router())
        .merge(plan_chat::router())
        .merge(archives::router())
        .merge(onboarding::router())
        .merge(contact::router())
        .merge(drafts::router())
        .merge(documents::router(config))
        .nest("/billing", crate::billing::router())
}

async fn health() -> &'static str {
    "ok"
}

/// Confirm the scenario exists and belongs to the caller.
///
/// Every nested resource route goes through this, so a caller can never reach a
/// row under someone else's scenario. A scenario owned by another user is
/// reported as `not found` rather than `forbidden`, so ids are not probeable.
pub async fn owned_scenario(db: &Db, scenario_id: i64, user_id: &str) -> ApiResult<()> {
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM scenarios WHERE id = ?1 AND user_id = ?2")
            .bind(scenario_id)
            .bind(user_id)
            .fetch_optional(db)
            .await?;

    exists.map(|_| ()).ok_or(ApiError::NotFound("scenario"))
}

/// Whether the scenario is an AI-guided draft (see `drafts`): a plan still
/// being written, which has no run to write suggestions or previews against.
pub(crate) async fn is_draft(db: &Db, scenario_id: i64) -> ApiResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM scenarios WHERE id = ?1 AND status = 'draft')",
    )
    .bind(scenario_id)
    .fetch_one(db)
    .await?)
}

/// The body every reorder route takes: the collection's row ids, in the order
/// the list should now be in.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ReorderRequest {
    pub ids: Vec<i64>,
}

/// Renumber `table`'s `sort_order` so the collection reads back as `requested`.
///
/// `current` is the collection's ids in their present order, already narrowed
/// to what the caller owns — this writes by primary key, so that query is the
/// only thing standing between a request and someone else's rows.
///
/// Returns the number of existing rows updated, or zero when no reorder commits.
///
/// One transaction: a half-applied renumbering is a list with two rows claiming
/// the same place, which sorts by id and looks like the drag simply misfired.
pub async fn apply_order(
    db: &Db,
    table: &'static str,
    current: &[i64],
    requested: &[i64],
) -> ApiResult<u64> {
    let order = reconcile(current, requested);
    if order == current {
        return Ok(0);
    }

    // `table` is a literal from this crate's own call sites, never request data.
    let sql = format!("UPDATE {table} SET sort_order = ?1 WHERE id = ?2");
    let mut tx = db.begin().await?;
    let mut affected = 0;
    for (rank, id) in order.iter().enumerate() {
        affected += sqlx::query(&sql)
            .bind(rank as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    }
    tx.commit().await?;
    Ok(affected)
}

/// Bump a scenario's `updated_at`, so clients can tell when cached results have
/// gone stale relative to the configuration that produced them.
pub async fn touch_scenario(db: &Db, scenario_id: i64) -> ApiResult<()> {
    sqlx::query("UPDATE scenarios SET updated_at = datetime('now') WHERE id = ?1")
        .bind(scenario_id)
        .execute(db)
        .await?;
    Ok(())
}
