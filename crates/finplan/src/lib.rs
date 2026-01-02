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

// ============================================================================
// Core modules
// ============================================================================

pub mod event_engine;
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

pub use config::SimulationBuilder;
pub use config::SimulationConfig;
pub use simulation::{monte_carlo_simulate, simulate};
pub use taxes::{LiquidationTaxResult, WithdrawalTaxResult};
