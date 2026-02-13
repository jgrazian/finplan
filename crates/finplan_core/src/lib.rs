//! Financial planning simulation library
//!
//! This crate provides a Monte Carlo simulation engine for retirement and financial planning.
//! It supports:
//! - Multiple account types (Taxable, Tax-Deferred, Tax-Free, Illiquid)
//! - Complex event-driven modeling with triggers and effects
//! - Inflation and return profiles (fixed, historical, Monte Carlo)
//! - Tax calculations with progressive federal brackets
//! - Required Minimum Distribution (RMD) modeling
//! - Spending targets with various withdrawal strategies
//!
//! # Builder DSL
//!
//! Use the fluent builder API for ergonomic simulation setup:
//!
//! ```ignore
//! use finplan::config::{SimulationBuilder, AccountBuilder, AssetBuilder, EventBuilder};
//!
//! let (config, metadata) = SimulationBuilder::new()
//!     .start(2025, 1, 1)
//!     .years(30)
//!     .birth_date(1980, 6, 15)
//!     .asset(AssetBuilder::us_total_market("VTSAX").price(100.0))
//!     .account(AccountBuilder::taxable_brokerage("Brokerage").cash(50_000.0))
//!     .position("Brokerage", "VTSAX", 500.0, 45_000.0)
//!     .event(EventBuilder::income("Salary")
//!         .to_account("Brokerage")
//!         .amount(8_000.0)
//!         .monthly())
//!     .build();
//! ```

#![warn(clippy::all)]

// ============================================================================
// Core modules
// ============================================================================

pub mod analysis;
pub mod apply;
pub mod date_math;
pub mod error;
pub mod evaluate;
pub mod liquidation;
pub mod metrics;
pub mod optimization;
pub mod simulation;
pub mod simulation_state;
pub mod taxes;

// ============================================================================
// Type definition modules
// ============================================================================

pub mod config;
pub mod model;

// ============================================================================
// Test modules
// ============================================================================

#[cfg(test)]
mod tests;

// ============================================================================
// Public re-exports for convenience
// ============================================================================

pub use config::{
    AccountBuilder, AssetBuilder, EventBuilder, SimulationBuilder, SimulationMetadata,
};
