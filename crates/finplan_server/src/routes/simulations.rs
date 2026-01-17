use axum::{
    Router,
    routing::{delete, get, post, put},
};

use crate::handlers::{self, DbConn};

pub fn simulation_routes() -> Router<DbConn> {
    Router::new()
        // Simulation CRUD
        .route("/api/simulations", get(handlers::list_simulations))
        .route("/api/simulations", post(handlers::create_simulation))
        // Builder-style simulation creation
        .route(
            "/api/simulations/build",
            post(handlers::create_simulation_from_request),
        )
        .route("/api/simulations/{id}", get(handlers::get_simulation))
        .route("/api/simulations/{id}", put(handlers::update_simulation))
        .route("/api/simulations/{id}", delete(handlers::delete_simulation))
        // Run simulation
        .route(
            "/api/simulations/{id}/run",
            post(handlers::run_saved_simulation),
        )
        .route("/api/simulate", post(handlers::run_simulation))
        // Simulation runs history
        .route(
            "/api/simulations/{id}/runs",
            get(handlers::list_simulation_runs),
        )
        .route("/api/runs/{id}", get(handlers::get_simulation_run))
}
