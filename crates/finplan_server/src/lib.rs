//! FinPlan HTTP server.
//!
//! Layering, outermost to innermost:
//!
//! ```text
//!   api/       axum handlers; JSON in, JSON out
//!   domain/    cross-table operations (scenario cloning)
//!   compile/   stored rows  ->  finplan_core::SimulationConfig
//!   runner/    background Monte Carlo execution and result persistence
//!   analysis/  sweeps, sensitivity and goal seeks, held in memory
//! ```
//!
//! The database schema is normalized around the *domain*, not around the
//! engine's in-memory types: stable ids, class-table inheritance for account
//! flavors, and self-referential tables for the recursive trigger, amount and
//! effect trees. `compile` is the only module that bridges the two worlds.

pub mod analysis;
pub mod api;
pub mod auth;
pub mod compile;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod runner;
pub mod seed;
pub mod state;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderValue, Method, header};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use config::ServerConfig;
use state::AppState;

/// Build the application state and router, run migrations, and recover any run
/// left in flight by a previous process.
pub async fn build(config: ServerConfig) -> Result<(Router, AppState), Box<dyn std::error::Error>> {
    let db = db::connect(&config.database_url, config.db_pool_size).await?;

    match auth::session::purge_expired(&db).await {
        Ok(n) if n > 0 => tracing::info!(count = n, "purged expired sessions"),
        Ok(_) => {}
        Err(err) => tracing::warn!(error = %err, "failed to purge expired sessions"),
    }

    let runs = runner::spawn(db.clone(), config.sim_workers);
    runner::requeue_orphans(&db, &runs).await?;

    let analyses = analysis::AnalysisJobs::new(config.sim_workers);

    let state = AppState {
        db: db.clone(),
        config: Arc::new(config),
        runs,
        analyses,
    };

    // Sessions accumulate; sweep them hourly rather than only at boot.
    tokio::spawn({
        let db = db.clone();
        async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(3600));
            ticker.tick().await; // the first tick fires immediately
            loop {
                ticker.tick().await;
                if let Err(err) = auth::session::purge_expired(&db).await {
                    tracing::warn!(error = %err, "session sweep failed");
                }
            }
        }
    });

    let cors = build_cors(&state.config.cors_origins);

    let router = Router::new()
        .nest("/api", api::router())
        .layer(cors)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    Ok((router, state))
}

/// Credentials travel in a cookie, so the allowed origins must be explicit —
/// `Access-Control-Allow-Origin: *` is rejected by browsers alongside
/// `Allow-Credentials`.
fn build_cors(origins: &[String]) -> CorsLayer {
    let parsed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|origin| match origin.trim().parse::<HeaderValue>() {
            Ok(value) => Some(value),
            Err(_) => {
                tracing::warn!(origin, "ignoring unparseable CORS origin");
                None
            }
        })
        .collect();

    CorsLayer::new()
        .allow_origin(parsed)
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
}
