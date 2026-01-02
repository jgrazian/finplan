mod db;
mod error;
mod handlers;
mod models;
mod routes;
mod validation;

use axum::{Router, routing::get};
use handlers::DbConn;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
    let conn = Connection::open("finplan.db").expect("Failed to open database");
    db::init_db(&conn).expect("Failed to initialize database");
    let db_conn: DbConn = Arc::new(Mutex::new(conn));

    let app = Router::new()
        .route("/", get(|| async { "FinPlan API Server" }))
        .merge(routes::portfolio_routes())
        .merge(routes::simulation_routes())
        .with_state(db_conn)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001")
        .await
        .expect("Failed to bind to port 3001");

    println!(
        "FinPlan API Server listening on {}",
        listener.local_addr().unwrap()
    );

    axum::serve(listener, app)
        .await
        .expect("Failed to start server");
}
