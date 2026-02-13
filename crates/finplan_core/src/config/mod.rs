//! Simulation configuration
//!
//! The main configuration type is `SimulationConfig`, which contains everything
//! needed to run a simulation. Helper methods support optimization use cases.
//!
//! # Builder DSL
//!
//! For a more ergonomic way to create simulations, use the builder DSL:
//!
//! ```ignore
//! use finplan::config::{SimulationBuilder, AccountBuilder, AssetBuilder, EventBuilder};
//!
//! let (config, metadata) = SimulationBuilder::new()
//!     .start(2025, 1, 1)
//!     .years(30)
//!     .birth_date(1980, 6, 15)
//!     
//!     // Define assets
//!     .asset(AssetBuilder::us_total_market("VTSAX").price(100.0))
//!     .asset(AssetBuilder::total_bond("BND").price(50.0))
//!     
//!     // Define accounts with preset types
//!     .account(AccountBuilder::bank_account("Checking").cash(10_000.0))
//!     .account(AccountBuilder::taxable_brokerage("Brokerage").cash(5_000.0))
//!     .account(AccountBuilder::traditional_401k("Work 401k").cash(200_000.0))
//!     
//!     // Add positions to accounts
//!     .position("Brokerage", "VTSAX", 500.0, 45_000.0)
//!     
//!     // Define events with fluent API
//!     .event(EventBuilder::income("Salary")
//!         .to_account("Checking")
//!         .amount(8_000.0)
//!         .monthly()
//!         .until_age(65))
//!     
//!     .build();
//! ```

use std::collections::HashMap;

use crate::model::{
    Account, AssetId, Event, EventId, EventTrigger, InflationProfile, ReturnProfile,
    ReturnProfileId, TaxConfig,
};
use serde::{Deserialize, Serialize};

// Builder DSL modules
pub mod account_builder;
pub mod asset_builder;
pub mod builder;
pub mod event_builder;
pub mod metadata;

// Re-export builder types for convenient access
pub use account_builder::AccountBuilder;
pub use asset_builder::{AssetBuilder, AssetDefinition};
pub use builder::SimulationBuilder;
pub use event_builder::{EventBuilder, EventDefinition};
pub use metadata::{EntityMetadata, SimulationMetadata};

fn default_duration_years() -> usize {
    30
}

fn default_true() -> bool {
    true
}

/// Complete simulation configuration
///
/// This is the main configuration type passed to the simulation engine.
/// Use the builder pattern methods for optimization scenarios.
///
/// # Conceptual Organization
///
/// **World assumptions** (scenarios you might compare):
/// - `return_profiles` - market assumptions
/// - `inflation_profile` - inflation model
/// - `tax_config` - tax law assumptions
///
/// **Your situation** (fixed facts):
/// - `birth_date` - for age-based calculations
/// - `start_date` - when simulation begins
/// - `accounts` - current balances
///
/// **Your plan** (structure with tunable values):
/// - `events` - life events (retirement, home purchase, etc.)
/// - `cash_flows` - income and contributions
/// - `spending_targets` - withdrawal needs
///
/// # Optimization Use Cases
///
/// The config provides helper methods for common optimization scenarios:
///
/// ```ignore
/// // Find optimal retirement age
/// for age in 50..70 {
///     let config = base_config.with_retirement_age(retirement_event_id, age);
///     let result = simulate(&config, seed);
///     // evaluate result...
/// }
///
/// // Find max sustainable spending
/// for spending in (50_000..200_000).step_by(5_000) {
///     let config = base_config.with_spending_amount(spending_target_id, spending as f64);
///     let result = simulate(&config, seed);
///     // evaluate result...
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    // === World Assumptions ===
    /// Return profiles for different asset classes.
    /// Assets reference these by index (`return_profile_index`).
    #[serde(default)]
    pub return_profiles: HashMap<ReturnProfileId, ReturnProfile>,

    /// Inflation model
    #[serde(default)]
    pub inflation_profile: InflationProfile,

    /// Map of `AssetId` to `ReturnProfileId` (which return profile applies to each asset)
    #[serde(default)]
    pub asset_returns: HashMap<AssetId, ReturnProfileId>,

    /// Initial asset prices (price per unit at simulation start)
    /// If not specified, assets default to $1.00 per unit
    #[serde(default)]
    pub asset_prices: HashMap<AssetId, f64>,

    /// Tax configuration (brackets, rates, etc.)
    #[serde(default)]
    pub tax_config: TaxConfig,

    // === Your Situation ===
    /// Start date for the simulation
    pub start_date: Option<jiff::civil::Date>,

    /// Birth date for age-based triggers and RMD calculations
    pub birth_date: Option<jiff::civil::Date>,

    /// Accounts with current balances
    #[serde(default)]
    pub accounts: Vec<Account>,

    // === Your Plan ===
    /// How many years to simulate
    #[serde(default = "default_duration_years")]
    pub duration_years: usize,

    /// Events that trigger state changes (retirement, home purchase, etc.)
    #[serde(default)]
    pub events: Vec<Event>,

    /// Per-asset tracking error (annualized standard deviation of idiosyncratic noise).
    /// Assets with tracking error get their base profile returns perturbed by N(0, tracking_error)
    /// each year, modeling deviation from the benchmark.
    #[serde(default)]
    pub asset_tracking_errors: HashMap<AssetId, f64>,

    /// Whether to collect ledger entries during simulation (default: true)
    ///
    /// Disable for batch MC iterations and parameter sweeps to save CPU/memory.
    /// When disabled, `SimulationResult.ledger` and `yearly_cash_flows` will be empty.
    #[serde(default = "default_true")]
    pub collect_ledger: bool,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            return_profiles: HashMap::new(),
            inflation_profile: InflationProfile::default(),
            asset_returns: HashMap::new(),
            asset_prices: HashMap::new(),
            asset_tracking_errors: HashMap::new(),
            tax_config: TaxConfig::default(),
            start_date: None,
            birth_date: None,
            accounts: Vec::new(),
            duration_years: default_duration_years(),
            events: Vec::new(),
            collect_ledger: true,
        }
    }
}

impl SimulationConfig {
    /// Create a new empty configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // === Optimization Helpers ===

    /// Create a variant with a different retirement age
    ///
    /// Finds the event with the given ID and updates its Age trigger.
    /// Returns None if the event doesn't exist or isn't age-triggered.
    #[must_use]
    pub fn with_retirement_age(&self, event_id: EventId, age: u8) -> Option<Self> {
        let mut config = self.clone();

        let event = config.events.iter_mut().find(|e| e.event_id == event_id)?;

        match &mut event.trigger {
            EventTrigger::Age { years, .. } => {
                *years = age;
                Some(config)
            }
            // Handle compound triggers that contain an Age trigger
            EventTrigger::And(triggers) | EventTrigger::Or(triggers) => {
                for trigger in triggers {
                    if let EventTrigger::Age { years, .. } = trigger {
                        *years = age;
                        return Some(config);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Create a variant with a different simulation duration
    #[must_use]
    pub fn with_duration_years(&self, years: usize) -> Self {
        let mut config = self.clone();
        config.duration_years = years;
        config
    }

    /// Set duration to simulate until a specific age
    #[must_use]
    pub fn with_end_age(&self, end_age: u8) -> Option<Self> {
        let birth = self.birth_date?;
        let start = self.start_date?;

        let current_age = {
            let years = start.year() - birth.year();
            if start.month() < birth.month()
                || (start.month() == birth.month() && start.day() < birth.day())
            {
                years - 1
            } else {
                years
            }
        };

        let duration = (i32::from(end_age) - i32::from(current_age)).max(1) as usize;
        Some(self.with_duration_years(duration))
    }

    // === Convenience Getters ===

    /// Calculate current age at start date
    #[must_use]
    pub fn initial_age(&self) -> Option<u8> {
        let birth = self.birth_date?;
        let start = self.start_date?;
        let years = start.year() - birth.year();

        if start.month() < birth.month()
            || (start.month() == birth.month() && start.day() < birth.day())
        {
            Some((years - 1) as u8)
        } else {
            Some(years as u8)
        }
    }

    /// Find an event by ID
    #[must_use]
    pub fn event(&self, id: EventId) -> Option<&Event> {
        self.events.iter().find(|e| e.event_id == id)
    }
}

// ============================================================================
// Optimization Support
// ============================================================================

/// Common optimization targets for retirement planning
///
/// Use this struct to define what you're searching for, then iterate
/// over possible values to find the optimal configuration.
///
/// # Example
///
/// ```ignore
/// let goal = OptimizationGoal::new()
///     .target_end_net_worth(0.0)  // Die broke
///     .evaluate_at_age(95);       // Plan to age 95
///
/// // Binary search for max sustainable spending
/// let optimal_spending = binary_search(50_000.0, 200_000.0, |spending| {
///     let config = base_config.with_spending_amount(spending_id, spending)?;
///     let result = simulate(&config, seed);
///     let end_worth = result.final_net_worth();
///     end_worth >= goal.target_end_net_worth.unwrap_or(0.0)
/// });
/// ```
#[derive(Debug, Clone, Default)]
pub struct OptimizationGoal {
    /// Target net worth at evaluation point (0.0 = "die broke")
    pub target_end_net_worth: Option<f64>,

    /// Age at which to evaluate success
    pub evaluate_at_age: Option<u8>,

    /// Minimum acceptable success rate in Monte Carlo (e.g., 0.95 for 95%)
    pub min_success_rate: Option<f64>,
}

impl OptimizationGoal {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn target_end_net_worth(mut self, net_worth: f64) -> Self {
        self.target_end_net_worth = Some(net_worth);
        self
    }

    #[must_use]
    pub fn evaluate_at_age(mut self, age: u8) -> Self {
        self.evaluate_at_age = Some(age);
        self
    }

    #[must_use]
    pub fn min_success_rate(mut self, rate: f64) -> Self {
        self.min_success_rate = Some(rate);
        self
    }
}
