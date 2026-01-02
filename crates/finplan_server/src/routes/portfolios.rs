use axum::{
    Router,
    routing::{delete, get, post, put},
};

use crate::handlers::{self, DbConn};

pub fn portfolio_routes() -> Router<DbConn> {
    Router::new()
        .route("/api/portfolios", get(handlers::list_portfolios))
        .route("/api/portfolios", post(handlers::create_portfolio))
        .route("/api/portfolios/{id}", get(handlers::get_portfolio))
        .route("/api/portfolios/{id}", put(handlers::update_portfolio))
        .route("/api/portfolios/{id}", delete(handlers::delete_portfolio))
        .route(
            "/api/portfolios/{id}/networth",
            get(handlers::get_portfolio_networth),
        )
}
