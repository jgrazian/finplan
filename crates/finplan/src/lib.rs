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
pub mod profiles;
pub mod simulation;
pub mod simulation_state;
pub mod taxes;

// ============================================================================
// Type definition modules
// ============================================================================

pub mod accounts;
pub mod builder;
pub mod cash_flows;
pub mod config;
pub mod descriptors;
pub mod events;
pub mod ids;
pub mod metadata;
pub mod records;
pub mod results;
pub mod rmd;
pub mod spending;
pub mod tax_config;

// ============================================================================
// Test modules
// ============================================================================

#[cfg(test)]
mod tests;

// ============================================================================
// Public re-exports for convenience
// ============================================================================

pub use accounts::{Account, AccountType, Asset, AssetClass};
pub use builder::SimulationBuilder;
pub use cash_flows::{
    CashFlow, CashFlowDirection, CashFlowLimits, CashFlowState, LimitPeriod, RepeatInterval,
    Timepoint,
};
pub use config::{SimulationConfig, SimulationParameters};
pub use events::{Event, EventEffect, EventTrigger, TriggerOffset};
pub use ids::{AccountId, AssetId, CashFlowId, EventId, SpendingTargetId};
pub use profiles::{InflationProfile, ReturnProfile};
pub use records::{CashFlowRecord, EventRecord, ReturnRecord, RmdRecord, TransferRecord, WithdrawalRecord};
pub use results::{AccountSnapshot, AssetSnapshot, MonteCarloResult, SimulationResult};
pub use rmd::{RmdTable, RmdTableEntry};
pub use simulation::{monte_carlo_simulate, simulate};
pub use spending::{SpendingTarget, SpendingTargetState, WithdrawalStrategy};
pub use tax_config::{TaxBracket, TaxConfig, TaxSummary};
