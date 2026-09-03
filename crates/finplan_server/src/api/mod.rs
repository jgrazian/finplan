//! HTTP routing.

pub mod accounts;
pub mod assets;
pub mod events;
pub mod profiles;
pub mod runs;
pub mod scenarios;
pub mod specs;
pub mod taxes;

use axum::Router;
use axum::routing::get;
use serde::{Deserialize, Deserializer};

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

/// Bump a scenario's `updated_at`, so clients can tell when cached results have
/// gone stale relative to the configuration that produced them.
pub async fn touch_scenario(db: &Db, scenario_id: i64) -> ApiResult<()> {
    sqlx::query("UPDATE scenarios SET updated_at = datetime('now') WHERE id = ?1")
        .bind(scenario_id)
        .execute(db)
        .await?;
    Ok(())
}
