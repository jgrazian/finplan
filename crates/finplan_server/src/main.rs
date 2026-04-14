mod auth;
mod db;
mod error;
mod handlers;
mod models;

use axum::Router;
use axum::http::Method;
use axum::routing::{delete, get, post, put};
use sqlx::SqlitePool;
use tower_http::cors::{Any, CorsLayer};

use crate::auth::JwtSecret;

fn api_routes() -> Router<SqlitePool> {
    Router::new()
        // Auth (public)
        .route("/auth/register", post(handlers::auth::register))
        .route("/auth/login", post(handlers::auth::login))
        .route("/auth/logout", post(handlers::auth::logout))
        .route("/auth/me", get(handlers::auth::me))
        // Accounts (authenticated via AuthUser extractor)
        .route("/accounts", get(handlers::accounts::list_accounts))
        .route("/accounts", post(handlers::accounts::create_account))
        .route("/accounts/{id}", get(handlers::accounts::get_account))
        .route("/accounts/{id}", put(handlers::accounts::update_account))
        .route("/accounts/{id}", delete(handlers::accounts::delete_account))
        // Holdings
        .route(
            "/accounts/{account_id}/holdings",
            get(handlers::holdings::list_holdings),
        )
        .route(
            "/accounts/{account_id}/holdings",
            post(handlers::holdings::create_holding),
        )
        .route("/holdings/{id}", put(handlers::holdings::update_holding))
        .route("/holdings/{id}", delete(handlers::holdings::delete_holding))
        // Return profiles
        .route("/profiles", get(handlers::profiles::list_profiles))
        .route("/profiles", post(handlers::profiles::create_profile))
        .route("/profiles/{id}", put(handlers::profiles::update_profile))
        .route("/profiles/{id}", delete(handlers::profiles::delete_profile))
        // Asset mappings
        .route("/mappings", get(handlers::mappings::list_mappings))
        .route(
            "/mappings/{asset_name}",
            put(handlers::mappings::upsert_mapping),
        )
        .route(
            "/mappings/{asset_name}",
            delete(handlers::mappings::delete_mapping),
        )
        // Allocation
        .route("/allocation", get(handlers::allocation::get_allocation))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "finplan_server=info,tower_http=info".parse().unwrap()),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:finplan.db?mode=rwc".into());

    let jwt_secret =
        std::env::var("JWT_SECRET").unwrap_or_else(|_| "finplan-dev-secret-change-in-prod".into());

    let pool = db::init(&database_url).await;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    let app = Router::new()
        .nest("/api", api_routes())
        .layer(axum::Extension(JwtSecret(jwt_secret)))
        .layer(cors)
        .with_state(pool);

    let addr = "0.0.0.0:3001";
    tracing::info!("Starting finplan server on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
