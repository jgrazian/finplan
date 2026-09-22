//! The most recent sweep of a scenario, kept across restarts and reloads.
//!
//! The one exception to analyses living in memory (see the module docs). A
//! sweep's grid is minutes of CPU and the whole Analysis screen is drawn from
//! it, so losing it to a page reload costs more than the row it takes to keep.
//! Only the newest grid per scenario is held: a sweep answers the question the
//! variables above it currently ask, and the previous answer belonged to a
//! question nobody is looking at any more.
//!
//! The grid is stored as the JSON the results endpoint would have returned,
//! because nothing queries into it — it is written whole and read back whole.
//! The layout drawn over it is kept beside it, in its own row: a re-run
//! replaces the grid, and the graphs someone arranged over it have to survive
//! that, since carrying a layout onto the next sweep is the whole point of
//! reconciling it against the new axes.

use crate::db::Db;
use crate::error::ApiResult;
use crate::observability::{Component, ErrorClass, Telemetry};

use super::results::{CachedSweep, SweepResults};

/// Replace the scenario's cached sweep with this one.
pub async fn save(
    db: &Db,
    scenario_id: i64,
    user_id: &str,
    results: &SweepResults,
) -> Result<(), sqlx::Error> {
    let json = serde_json::to_string(results).map_err(|err| sqlx::Error::Encode(Box::new(err)))?;
    sqlx::query(
        "INSERT INTO sweep_cache (scenario_id, user_id, results, created_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(scenario_id) DO UPDATE
            SET user_id = excluded.user_id,
                results = excluded.results,
                created_at = excluded.created_at",
    )
    .bind(scenario_id)
    .bind(user_id)
    .bind(json)
    .execute(db)
    .await?;
    Ok(())
}

/// The scenario's cached sweep, or `None` where it has never been swept.
///
/// A grid stored by a previous version of the server may no longer parse into
/// the current [`SweepResults`]. That is a cache miss and not an error: the
/// screen offers to run a sweep, which is what it would have done anyway.
pub async fn load(
    db: &Db,
    scenario_id: i64,
    user_id: &str,
    telemetry: &Telemetry,
) -> ApiResult<Option<CachedSweep>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT results, created_at FROM sweep_cache WHERE scenario_id = ?1 AND user_id = ?2",
    )
    .bind(scenario_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    let Some((json, created_at)) = row else {
        return Ok(None);
    };
    match serde_json::from_str::<SweepResults>(&json) {
        Ok(results) => Ok(Some(CachedSweep {
            scenario_id,
            created_at,
            results,
            layout: load_layout(db, scenario_id, user_id, telemetry).await?,
        })),
        Err(_) => {
            telemetry.recoverable_error_event(
                Component::Analysis,
                ErrorClass::Preparation,
                "analysis.cache_failed",
            );
            Ok(None)
        }
    }
}

/// The graph layout drawn over the scenario's sweep, as the client stored it.
///
/// Unreadable JSON is treated as no layout at all: the screen falls back to the
/// graphs a fresh sweep opens on, which is the same place it starts from when
/// nobody has arranged anything yet.
pub async fn load_layout(
    db: &Db,
    scenario_id: i64,
    user_id: &str,
    telemetry: &Telemetry,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    let row: Option<String> = sqlx::query_scalar(
        "SELECT graphs FROM sweep_layout WHERE scenario_id = ?1 AND user_id = ?2",
    )
    .bind(scenario_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    Ok(row.and_then(|json| match serde_json::from_str(&json) {
        Ok(value) => Some(value),
        Err(_) => {
            telemetry.recoverable_error_event(
                Component::Analysis,
                ErrorClass::Preparation,
                "analysis.cache_failed",
            );
            None
        }
    }))
}

/// Replace the scenario's stored layout.
pub async fn save_layout(
    db: &Db,
    scenario_id: i64,
    user_id: &str,
    graphs: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    let json = serde_json::to_string(graphs).map_err(|err| sqlx::Error::Encode(Box::new(err)))?;
    sqlx::query(
        "INSERT INTO sweep_layout (scenario_id, user_id, graphs, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(scenario_id) DO UPDATE
            SET user_id = excluded.user_id,
                graphs = excluded.graphs,
                updated_at = excluded.updated_at",
    )
    .bind(scenario_id)
    .bind(user_id)
    .bind(json)
    .execute(db)
    .await?;
    Ok(())
}
