//! Integration tests for the finplan simulation engine
//!
//! Tests are organized by topic:
//! - `basic` - Core simulation mechanics
//! - `returns` - Investment returns and market appreciation
//! - `accounts` - Account structures and operations
//! - `simulation_result` - Result structure and methods
//! - `builder_dsl` - Builder DSL for fluent simulation setup
//!
//! Legacy tests (disabled - use old API):
//! - `event_effects` - Event system tests (Transfer, Sweep, triggers)
//! - `rmd` - Required Minimum Distribution tests
//! - `comprehensive` - Full lifecycle integration tests

mod basic;
mod returns;
mod accounts;
mod simulation_result;
mod builder_dsl;

// Legacy tests - disabled due to API changes
// mod comprehensive;
// mod event_effects;
// mod rmd;
