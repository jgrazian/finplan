//! Integration tests for the finplan simulation engine
//!
//! Tests are organized by topic:
//! - `basic` - Core simulation mechanics
//! - `returns` - Investment returns and market appreciation
//! - `accounts` - Account structures and operations
//! - `simulation_result` - Result structure and methods
//! - `builder_dsl` - Builder DSL for fluent simulation setup
//! - `contribution_limits` - Contribution limit enforcement tests
//!
//! Legacy tests (disabled - use old API):
//! - `event_effects` - Event system tests (Transfer, Sweep, triggers)
//! - `rmd` - Required Minimum Distribution tests
//! - `comprehensive` - Full lifecycle integration tests

mod accounts;
mod basic;
mod builder_dsl;
mod contribution_limits;
mod returns;
mod simulation_result;

// Legacy tests - disabled due to API changes
// mod comprehensive;
// mod event_effects;
// mod rmd;
