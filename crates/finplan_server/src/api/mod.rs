//! HTTP routing.

pub mod accounts;
pub mod assets;
pub mod events;
pub mod profiles;
pub mod runs;
pub mod scenarios;
pub mod specs;
pub mod taxes;

use std::collections::HashSet;

use axum::Router;
use axum::routing::get;
use serde::{Deserialize, Deserializer};
use ts_rs::TS;

use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .nest("/auth", crate::auth::routes::router())
        .merge(scenarios::router())
        .merge(assets::router())
        .merge(accounts::router())
        .merge(events::router())
        .merge(profiles::router())
        .merge(taxes::router())
        .merge(runs::router())
}

async fn health() -> &'static str {
    "ok"
}

/// Distinguish "field absent" from "field present and null". Serde collapses
/// the two into `None` for a plain `Option`; wrapping the deserialize in a
/// second layer keeps them apart.
///
/// Every PATCH body here reads an absent field as "unchanged", which on its own
/// makes clearing a nullable column unsayable — this is how `null` gets to mean
/// it.
pub(crate) fn double_option<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(de).map(Some)
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

/// The body every reorder route takes: the collection's row ids, in the order
/// the list should now be in.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ReorderRequest {
    pub ids: Vec<i64>,
}

/// The order `requested` asks for, reconciled against what the collection
/// actually holds.
///
/// Named rows come first, in the order named; anything the caller did not name
/// keeps its place behind them. An id that is not in the collection is dropped
/// rather than refused: a list a beat out of date — a row deleted in another
/// tab, one belonging to a scenario the caller does not own — should still
/// reorder under the user's hand rather than fail there.
fn reconcile(current: &[i64], requested: &[i64]) -> Vec<i64> {
    let known: HashSet<i64> = current.iter().copied().collect();
    let mut placed: HashSet<i64> = HashSet::new();
    let mut order: Vec<i64> = requested
        .iter()
        .copied()
        .filter(|id| known.contains(id) && placed.insert(*id))
        .collect();
    order.extend(current.iter().copied().filter(|id| !placed.contains(id)));
    order
}

/// Renumber `table`'s `sort_order` so the collection reads back as `requested`.
///
/// `current` is the collection's ids in their present order, already narrowed
/// to what the caller owns — this writes by primary key, so that query is the
/// only thing standing between a request and someone else's rows.
///
/// One transaction: a half-applied renumbering is a list with two rows claiming
/// the same place, which sorts by id and looks like the drag simply misfired.
pub async fn apply_order(
    db: &Db,
    table: &'static str,
    current: &[i64],
    requested: &[i64],
) -> ApiResult<()> {
    let order = reconcile(current, requested);
    if order == current {
        return Ok(());
    }

    // `table` is a literal from this crate's own call sites, never request data.
    let sql = format!("UPDATE {table} SET sort_order = ?1 WHERE id = ?2");
    let mut tx = db.begin().await?;
    for (rank, id) in order.iter().enumerate() {
        sqlx::query(&sql)
            .bind(rank as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
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
