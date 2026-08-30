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
