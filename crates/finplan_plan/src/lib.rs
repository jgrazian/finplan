//! The financial plan as a value, with no database and no server.
//!
//! What a plan *is* (its accounts, assets, events and the rows that link them)
//! used to live inside the HTTP server, mixed with SQL. This crate is the one
//! home for that model, so anything that needs a plan can reach it without
//! SQLite: the server's tests, an in-memory edit that previews a change, and,
//! because it depends on nothing that does I/O, a build for the browser. The
//! `sqlx` feature adds `FromRow` to the row structs for the server; everything
//! else is plain data and logic.

pub mod compile;
pub mod error;
pub mod graph;
pub mod snapshot;
pub mod specs;

pub use error::{PlanError, PlanResult};
