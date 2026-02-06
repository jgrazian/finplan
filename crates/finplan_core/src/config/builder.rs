//! Simulation Builder
//!
//! The SimulationBuilder provides a fluent API for creating simulations
//! with automatic ID assignment, name-based lookups, and metadata tracking.
//!
//! # Example
//!
//! ```ignore
//! use finplan::config::{SimulationBuilder, AccountBuilder, AssetBuilder, EventBuilder};
//! use finplan::model::ReturnProfile;
//!
//! let (config, metadata) = SimulationBuilder::new()
//!     .start(2025, 1, 1)
//!     .duration_years(30)
//!     .birth_date(1980, 6, 15)
//!     
//!     // Define assets with return profiles
//!     .asset(AssetBuilder::us_total_market("VTSAX").price(100.0))
//!     .asset(AssetBuilder::total_bond("BND").price(50.0))
//!     
//!     // Define accounts
//!     .account(AccountBuilder::bank_account("Checking").cash(10_000.0))
//!     .account(AccountBuilder::taxable_brokerage("Brokerage").cash(50_000.0))
//!     .account(AccountBuilder::traditional_401k("Work 401k").cash(200_000.0))
//!     
//!     // Add positions to accounts
//!     .position("Brokerage", "VTSAX", 500.0, 45_000.0)
//!     .position("Work 401k", "VTSAX", 1500.0, 120_000.0)
//!     
//!     // Define events
//!     .event(EventBuilder::income("Salary")
//!         .to_account("Checking")
//!         .amount(8_000.0)
//!         .monthly()
//!         .until_age(65))
//!     .event(EventBuilder::expense("Living Expenses")
//!         .from_account("Checking")
//!         .amount(5_000.0)
//!         .monthly())
//!     
//!     .build();
//! ```

use std::collections::HashMap;

use super::SimulationConfig;
use super::account_builder::AccountBuilder;
use super::asset_builder::{AssetBuilder, AssetDefinition};
use super::event_builder::{
    AccountRef, AmountSpec, AssetRef, EventBuilder, EventDefinition, EventType, TriggerSpec,
    WithdrawalSourceSpec,
};
use super::metadata::SimulationMetadata;
use crate::model::{
    AccountId, AssetCoord, AssetId, AssetLot, Event, EventEffect, EventId, EventTrigger,
    IncomeType, InflationProfile, ReturnProfile, ReturnProfileId, TaxConfig, TransferAmount,
    WithdrawalSources,
};

/// Builder for creating simulations with automatic ID assignment and metadata tracking
pub struct SimulationBuilder {
    config: SimulationConfig,
    metadata: SimulationMetadata,
    next_account_id: u16,
    next_asset_id: u16,
    next_event_id: u16,
    next_return_profile_id: u16,

    // Pending builders (resolved during build)
    pending_accounts: Vec<AccountBuilder>,
    pending_assets: Vec<AssetDefinition>,
    pending_events: Vec<EventDefinition>,
    pending_positions: Vec<PendingPosition>,

    // Track asset -> return profile mappings
    asset_return_profiles: HashMap<String, ReturnProfileId>,
}

#[derive(Debug, Clone)]
struct PendingPosition {
    account_name: String,
    asset_name: String,
    units: f64,
    cost_basis: f64,
    purchase_date: Option<jiff::civil::Date>,
}

impl Default for SimulationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationBuilder {
    /// Create a new simulation builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: SimulationConfig::default(),
            metadata: SimulationMetadata::new(),
            next_account_id: 0,
            next_asset_id: 0,
            next_event_id: 0,
            next_return_profile_id: 0,
            pending_accounts: Vec::new(),
            pending_assets: Vec::new(),
            pending_events: Vec::new(),
            pending_positions: Vec::new(),
            asset_return_profiles: HashMap::new(),
        }
    }

    // =========================================================================
    // Basic Configuration
    // =========================================================================

    /// Set the simulation start date
    #[must_use]
    pub fn start_date(mut self, date: jiff::civil::Date) -> Self {
        self.config.start_date = Some(date);
        self
    }

    /// Set the simulation start date (convenience method)
    #[must_use]
    pub fn start(mut self, year: i16, month: i8, day: i8) -> Self {
        self.config.start_date = Some(jiff::civil::date(year, month, day));
        self
    }

    /// Set the simulation duration in years
    #[must_use]
    pub fn duration_years(mut self, years: usize) -> Self {
        self.config.duration_years = years;
        self
    }

    /// Alias for duration_years
    #[must_use]
    pub fn years(self, years: usize) -> Self {
        self.duration_years(years)
    }

    /// Set the birth date for age-based triggers
    #[must_use]
    pub fn birth_date(mut self, year: i16, month: i8, day: i8) -> Self {
        self.config.birth_date = Some(jiff::civil::date(year, month, day));
        self
    }

    /// Set the birth date using a Date object
    #[must_use]
    pub fn birth_date_obj(mut self, date: jiff::civil::Date) -> Self {
        self.config.birth_date = Some(date);
        self
    }

    /// Set the inflation profile
    #[must_use]
    pub fn inflation_profile(mut self, profile: InflationProfile) -> Self {
        self.config.inflation_profile = profile;
        self
    }

    /// Set a fixed inflation rate
    #[must_use]
    pub fn inflation(mut self, rate: f64) -> Self {
        self.config.inflation_profile = InflationProfile::Fixed(rate);
        self
    }

    /// Set the tax configuration
    #[must_use]
    pub fn tax_config(mut self, config: TaxConfig) -> Self {
        self.config.tax_config = config;
        self
    }

    // =========================================================================
    // Return Profiles
    // =========================================================================

    /// Add a named return profile
    #[must_use]
    pub fn return_profile(mut self, name: impl Into<String>, profile: ReturnProfile) -> Self {
        let name = name.into();
        let profile_id = ReturnProfileId(self.next_return_profile_id);
        self.next_return_profile_id += 1;

        self.config.return_profiles.insert(profile_id, profile);
        self.metadata
            .register_return_profile(profile_id, Some(name), None);

        self
    }

    // =========================================================================
    // Assets
    // =========================================================================

    /// Add an asset definition using the AssetBuilder
    #[must_use]
    pub fn asset(mut self, builder: AssetBuilder) -> Self {
        self.pending_assets.push(builder.build());
        self
    }

    /// Quick method to add an asset with a fixed return
    #[must_use]
    pub fn asset_fixed(mut self, name: impl Into<String>, price: f64, annual_return: f64) -> Self {
        self.pending_assets.push(
            AssetBuilder::new(name)
                .price(price)
                .fixed_return(annual_return)
                .build(),
        );
        self
    }

    // =========================================================================
    // Accounts
    // =========================================================================

    /// Add an account using the AccountBuilder
    #[must_use]
    pub fn account(mut self, builder: AccountBuilder) -> Self {
        self.pending_accounts.push(builder);
        self
    }

    /// Quick method to add a bank account with cash
    #[must_use]
    pub fn bank(self, name: impl Into<String>, cash: f64) -> Self {
        self.account(AccountBuilder::bank_account(name).cash(cash))
    }

    /// Quick method to add a taxable brokerage account
    #[must_use]
    pub fn brokerage(self, name: impl Into<String>, cash: f64) -> Self {
        self.account(AccountBuilder::taxable_brokerage(name).cash(cash))
    }

    /// Quick method to add a traditional 401k
    #[must_use]
    pub fn traditional_401k(self, name: impl Into<String>, cash: f64) -> Self {
        self.account(AccountBuilder::traditional_401k(name).cash(cash))
    }

    /// Quick method to add a Roth IRA
    #[must_use]
    pub fn roth_ira(self, name: impl Into<String>, cash: f64) -> Self {
        self.account(AccountBuilder::roth_ira(name).cash(cash))
    }

    // =========================================================================
    // Positions (assets held in accounts)
    // =========================================================================

    /// Add a position (asset holding) to an account by name
    ///
    /// # Arguments
    /// * `account_name` - Name of the account to add the position to
    /// * `asset_name` - Name of the asset (must be defined via `asset()`)
    /// * `units` - Number of shares/units
    /// * `cost_basis` - Total cost basis for this lot
    #[must_use]
    pub fn position(
        mut self,
        account_name: impl Into<String>,
        asset_name: impl Into<String>,
        units: f64,
        cost_basis: f64,
    ) -> Self {
        self.pending_positions.push(PendingPosition {
            account_name: account_name.into(),
            asset_name: asset_name.into(),
            units,
            cost_basis,
            purchase_date: None,
        });
        self
    }

    /// Add a position with a specific purchase date
    #[must_use]
    pub fn position_dated(
        mut self,
        account_name: impl Into<String>,
        asset_name: impl Into<String>,
        units: f64,
        cost_basis: f64,
        purchase_date: jiff::civil::Date,
    ) -> Self {
        self.pending_positions.push(PendingPosition {
            account_name: account_name.into(),
            asset_name: asset_name.into(),
            units,
            cost_basis,
            purchase_date: Some(purchase_date),
        });
        self
    }

    // =========================================================================
    // Events
    // =========================================================================

    /// Add an event using the EventBuilder
    #[must_use]
    pub fn event(mut self, builder: EventBuilder) -> Self {
        self.pending_events.push(builder.build());
        self
    }

    /// Quick method to add monthly income
    #[must_use]
    pub fn monthly_income(
        self,
        name: impl Into<String>,
        to_account: impl Into<String>,
        amount: f64,
    ) -> Self {
        self.event(
            EventBuilder::income(name)
                .to_account(to_account)
                .amount(amount)
                .gross()
                .monthly(),
        )
    }

    /// Quick method to add monthly expense
    #[must_use]
    pub fn monthly_expense(
        self,
        name: impl Into<String>,
        from_account: impl Into<String>,
        amount: f64,
    ) -> Self {
        self.event(
            EventBuilder::expense(name)
                .from_account(from_account)
                .amount(amount)
                .monthly(),
        )
    }

    // =========================================================================
    // Build
    // =========================================================================

    /// Build the simulation configuration and metadata
    ///
    /// This resolves all name references to IDs and creates the final configuration.
    #[must_use]
    pub fn build(mut self) -> (SimulationConfig, SimulationMetadata) {
        // 1. Register all assets and their return profiles
        let mut asset_ids: HashMap<String, AssetId> = HashMap::new();
        for asset_def in &self.pending_assets {
            let asset_id = AssetId(self.next_asset_id);
            self.next_asset_id += 1;

            // Get or create return profile
            let return_profile_id = if let Some(ref profile_name) = asset_def.return_profile_name {
                // Use named profile
                self.metadata
                    .return_profile_id(profile_name)
                    .unwrap_or_else(|| {
                        // Create new profile with this name
                        let id = ReturnProfileId(self.next_return_profile_id);
                        self.next_return_profile_id += 1;
                        self.config
                            .return_profiles
                            .insert(id, asset_def.return_profile.clone());
                        self.metadata
                            .register_return_profile(id, Some(profile_name.clone()), None);
                        id
                    })
            } else {
                // Create inline return profile for this asset
                let profile_id = ReturnProfileId(self.next_return_profile_id);
                self.next_return_profile_id += 1;
                self.config
                    .return_profiles
                    .insert(profile_id, asset_def.return_profile.clone());
                profile_id
            };

            // Register asset in config
            self.config
                .asset_returns
                .insert(asset_id, return_profile_id);
            self.config
                .asset_prices
                .insert(asset_id, asset_def.initial_price);

            // Register in metadata
            self.metadata.register_asset(
                asset_id,
                Some(asset_def.name.clone()),
                asset_def.description.clone(),
            );

            asset_ids.insert(asset_def.name.clone(), asset_id);
            self.asset_return_profiles
                .insert(asset_def.name.clone(), return_profile_id);
        }

        // 2. Register all accounts
        let mut account_ids: HashMap<String, AccountId> = HashMap::new();
        for account_builder in self.pending_accounts.drain(..) {
            let account_id = AccountId(self.next_account_id);
            self.next_account_id += 1;

            // Extract name and description before consuming the builder
            let name = account_builder.name.clone();
            let description = account_builder.description.clone();
            let account = account_builder.build_with_id(account_id);

            self.config.accounts.push(account);
            self.metadata
                .register_account(account_id, name.clone(), description);

            if let Some(n) = name {
                account_ids.insert(n, account_id);
            }
        }

        // 3. Add positions to accounts
        let start_date = self
            .config
            .start_date
            .unwrap_or(jiff::civil::date(2025, 1, 1));
        for pos in self.pending_positions.drain(..) {
            if let (Some(&account_id), Some(&asset_id)) = (
                account_ids.get(&pos.account_name),
                asset_ids.get(&pos.asset_name),
            ) {
                let lot = AssetLot {
                    asset_id,
                    purchase_date: pos.purchase_date.unwrap_or(start_date),
                    units: pos.units,
                    cost_basis: pos.cost_basis,
                };

                // Find the account and add the position
                if let Some(account) = self
                    .config
                    .accounts
                    .iter_mut()
                    .find(|a| a.account_id == account_id)
                    && let crate::model::AccountFlavor::Investment(ref mut inv) = account.flavor
                {
                    inv.positions.push(lot);
                }
            }
        }

        // 4. Register and resolve all events
        // Drain events first to avoid borrow issues
        let pending_events: Vec<_> = self.pending_events.drain(..).collect();
        for event_def in pending_events {
            let event_id = EventId(self.next_event_id);
            self.next_event_id += 1;

            let trigger = self.resolve_trigger(&event_def.trigger);
            let effects = self.resolve_effects(&event_def.event_type, &account_ids, &asset_ids);

            let event = Event {
                event_id,
                trigger,
                effects,
                once: event_def.once,
            };

            self.config.events.push(event);
            self.metadata
                .register_event(event_id, Some(event_def.name), event_def.description);
        }

        (self.config, self.metadata)
    }

    // =========================================================================
    // Resolution Helpers
    // =========================================================================

    fn resolve_trigger(&self, spec: &TriggerSpec) -> EventTrigger {
        match spec {
            TriggerSpec::Immediate => {
                // Trigger at simulation start
                if let Some(date) = self.config.start_date {
                    EventTrigger::Date(date)
                } else {
                    EventTrigger::Date(jiff::civil::date(2025, 1, 1))
                }
            }
            TriggerSpec::Date(d) => EventTrigger::Date(*d),
            TriggerSpec::Age { years, months } => EventTrigger::Age {
                years: *years,
                months: *months,
            },
            TriggerSpec::Repeating {
                interval,
                start,
                end,
                max_occurrences,
            } => EventTrigger::Repeating {
                interval: *interval,
                start_condition: start.as_ref().map(|s| Box::new(self.resolve_trigger(s))),
                end_condition: end.as_ref().map(|e| Box::new(self.resolve_trigger(e))),
                max_occurrences: *max_occurrences,
            },
        }
    }

    fn resolve_effects(
        &self,
        event_type: &EventType,
        account_ids: &HashMap<String, AccountId>,
        asset_ids: &HashMap<String, AssetId>,
    ) -> Vec<EventEffect> {
        match event_type {
            EventType::Income(spec) => {
                let account_id = self.resolve_account_ref(&spec.to_account, account_ids);
                vec![EventEffect::Income {
                    to: account_id,
                    amount: self.resolve_amount(&spec.amount),
                    amount_mode: spec.amount_mode,
                    income_type: spec.income_type.clone(),
                }]
            }
            EventType::Expense(spec) => {
                let account_id = self.resolve_account_ref(&spec.from_account, account_ids);
                vec![EventEffect::Expense {
                    from: account_id,
                    amount: self.resolve_amount(&spec.amount),
                }]
            }
            EventType::AssetPurchase(spec) => {
                let from_account = self.resolve_account_ref(&spec.from_account, account_ids);
                let to_asset = self.resolve_asset_ref(&spec.to_asset, account_ids, asset_ids);
                vec![EventEffect::AssetPurchase {
                    from: from_account,
                    to: to_asset,
                    amount: self.resolve_amount(&spec.amount),
                }]
            }
            EventType::AssetSale(spec) => {
                let to_account = self.resolve_account_ref(&spec.to_account, account_ids);
                let sources =
                    self.resolve_withdrawal_sources(&spec.sources, account_ids, asset_ids);
                // AssetSale in the builder API now translates to Sweep (liquidate + transfer)
                vec![EventEffect::Sweep {
                    sources,
                    to: to_account,
                    amount: self.resolve_amount(&spec.amount),
                    amount_mode: spec.amount_mode,
                    lot_method: spec.lot_method,
                    income_type: IncomeType::Taxable, // Default to taxable for asset sales
                }]
            }
            EventType::Custom(effects) => effects.clone(),
        }
    }

    fn resolve_account_ref(
        &self,
        account_ref: &AccountRef,
        account_ids: &HashMap<String, AccountId>,
    ) -> AccountId {
        match account_ref {
            AccountRef::Id(id) => *id,
            AccountRef::Name(name) => account_ids.get(name).copied().unwrap_or(AccountId(0)), // Fallback to ID 0 if not found
        }
    }

    fn resolve_asset_ref(
        &self,
        asset_ref: &AssetRef,
        account_ids: &HashMap<String, AccountId>,
        asset_ids: &HashMap<String, AssetId>,
    ) -> AssetCoord {
        match asset_ref {
            AssetRef::Coord(coord) => *coord,
            AssetRef::Named { account, asset } => {
                let account_id = account_ids.get(account).copied().unwrap_or(AccountId(0));
                let asset_id = asset_ids.get(asset).copied().unwrap_or(AssetId(0));
                AssetCoord {
                    account_id,
                    asset_id,
                }
            }
        }
    }

    fn resolve_amount(&self, spec: &AmountSpec) -> TransferAmount {
        match spec {
            AmountSpec::Fixed(v) => TransferAmount::Fixed(*v),
            AmountSpec::SourceBalance => TransferAmount::SourceBalance,
            AmountSpec::TransferAmount(t) => t.clone(),
        }
    }

    fn resolve_withdrawal_sources(
        &self,
        spec: &WithdrawalSourceSpec,
        account_ids: &HashMap<String, AccountId>,
        asset_ids: &HashMap<String, AssetId>,
    ) -> WithdrawalSources {
        match spec {
            WithdrawalSourceSpec::SingleAsset(asset_ref) => WithdrawalSources::SingleAsset(
                self.resolve_asset_ref(asset_ref, account_ids, asset_ids),
            ),
            WithdrawalSourceSpec::Strategy { order, exclude } => {
                let exclude_accounts = exclude
                    .iter()
                    .map(|r| self.resolve_account_ref(r, account_ids))
                    .collect();
                WithdrawalSources::Strategy {
                    order: *order,
                    exclude_accounts,
                }
            }
            WithdrawalSourceSpec::AccountOrder(accounts) => {
                // Convert account names to AssetCoords (using first asset in each account)
                // This is a simplification - in practice you might want to handle this differently
                let coords: Vec<AssetCoord> = accounts
                    .iter()
                    .map(|r| {
                        let account_id = self.resolve_account_ref(r, account_ids);
                        AssetCoord {
                            account_id,
                            asset_id: AssetId(0), // Will use cash/default asset
                        }
                    })
                    .collect();
                WithdrawalSources::Custom(coords)
            }
        }
    }
}
