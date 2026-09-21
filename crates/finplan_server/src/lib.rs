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
pub mod billing;
pub mod compile;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod observability;
pub mod runner;
pub mod seed;
pub mod state;

use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, Method, header};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;

use config::ServerConfig;
use state::AppState;

/// Build the application state and router, run migrations, and recover any run
/// left in flight by a previous process.
///
/// This takes an initial queue snapshot but opens no network listeners. The
/// executable owns an `ObservabilityRuntime` for sampling and session maintenance.
pub async fn build(config: ServerConfig) -> Result<(Router, AppState), Box<dyn std::error::Error>> {
    config.validate().inspect_err(|reason| {
        // validate() returns only application-owned static explanations.
        tracing::error!(event = "server.configuration_invalid", reason);
    })?;
    let telemetry = observability::Telemetry::new(config.sim_workers);
    let db = db::connect(&config.database_url, config.db_pool_size)
        .await
        .inspect_err(|_| {
            tracing::error!(
                event = "server.initialization_failed",
                phase = "database",
                class = "database"
            );
        })?;
    observability::purge_sessions(&db, &telemetry).await;

    let runs = runner::spawn_with_telemetry(db.clone(), config.sim_workers, telemetry.clone());
    runner::requeue_orphans(&db, &runs).await?;

    let analyses = analysis::AnalysisJobs::new_with_telemetry(
        db.clone(),
        config.sim_workers,
        telemetry.clone(),
    );

    let state = AppState {
        telemetry,
        db: db.clone(),
        config: Arc::new(config),
        runs,
        analyses,
    };

    observability::sample(&state).await;

    let cors = build_cors(&state.config.cors_origins);

    let router = Router::new()
        .nest("/api", api::router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            billing::mutation_entitlements,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::protection::auth_throttle,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::protection::mutation_origin,
        ))
        .layer(cors)
        .layer(CompressionLayer::new())
        .layer(axum::middleware::from_fn_with_state(
            state.telemetry.clone(),
            observability::request_telemetry,
        ))
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
                tracing::warn!(
                    event = "server.configuration_warning",
                    reason = "invalid_cors_origin"
                );
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
        .expose_headers([axum::http::HeaderName::from_static("x-request-id")])
}
