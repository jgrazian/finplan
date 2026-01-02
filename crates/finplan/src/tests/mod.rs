//! Integration tests for the finplan simulation engine
//!
//! Tests are organized by topic:
//! - `basic` - Core simulation mechanics
//! - `event_effects` - Event system tests (Transfer, Sweep, triggers)
//! - `rmd` - Required Minimum Distribution tests
//! - `comprehensive` - Full lifecycle integration tests

mod basic;
mod comprehensive;
mod event_effects;
mod rmd;
