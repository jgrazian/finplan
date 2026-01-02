//! Simulation Builder
//!
//! The SimulationBuilder provides a fluent API for creating simulations
//! with automatic ID assignment and metadata tracking.

use super::SimulationConfig;
use super::descriptors::{
    AccountDescriptor, AssetDescriptor, CashFlowDescriptor, EventDescriptor,
    SpendingTargetDescriptor,
};
use crate::model::{
    Account, AccountId, Asset, AssetId, CashFlow, CashFlowId, EntityMetadata, Event, EventId,
    InflationProfile, ReturnProfile, SimulationMetadata, SpendingTarget, SpendingTargetId,
    TaxConfig,
};

/// Builder for creating simulations with automatic ID assignment and metadata tracking
///
/// # Example
///
/// ```ignore
/// use finplan::builder::SimulationBuilder;
/// use finplan::accounts::AccountType;
/// use finplan::descriptors::{AccountDescriptor, AssetDescriptor};
///
/// let (builder, brokerage_id) = SimulationBuilder::new()
///     .start_date(jiff::civil::date(2025, 1, 1))
///     .duration_years(30)
///     .add_account(
///         AccountDescriptor::new(AccountType::Taxable)
///             .name("Brokerage")
///     );
///
/// let (builder, stock_id) = builder.add_asset(
///     brokerage_id,
///     AssetDescriptor::new(AssetClass::Investable, 100_000.0, 0)
///         .name("S&P 500 Index")
/// );
///
/// let (config, metadata) = builder.build();
/// ```
pub struct SimulationBuilder {
    config: SimulationConfig,
    metadata: SimulationMetadata,
    next_account_id: u16,
    next_asset_id: u16,
    next_cash_flow_id: u16,
    next_event_id: u16,
    next_spending_target_id: u16,
}

impl Default for SimulationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationBuilder {
    pub fn new() -> Self {
        Self {
            config: SimulationConfig::default(),
            metadata: SimulationMetadata::new(),
            next_account_id: 0,
            next_asset_id: 0,
            next_cash_flow_id: 0,
            next_event_id: 0,
            next_spending_target_id: 0,
        }
    }

    /// Set the simulation start date
    pub fn start_date(mut self, date: jiff::civil::Date) -> Self {
        self.config.start_date = Some(date);
        self
    }

    /// Set the simulation duration in years
    pub fn duration_years(mut self, years: usize) -> Self {
        self.config.duration_years = years;
        self
    }

    /// Set the birth date for age-based triggers
    pub fn birth_date(mut self, date: jiff::civil::Date) -> Self {
        self.config.birth_date = Some(date);
        self
    }

    /// Set the inflation profile
    pub fn inflation_profile(mut self, profile: InflationProfile) -> Self {
        self.config.inflation_profile = profile;
        self
    }

    /// Add a return profile
    pub fn add_return_profile(mut self, profile: ReturnProfile) -> Self {
        self.config.return_profiles.push(profile);
        self
    }

    /// Set the tax configuration
    pub fn tax_config(mut self, config: TaxConfig) -> Self {
        self.config.tax_config = config;
        self
    }

    /// Add an account using a descriptor
    pub fn add_account(mut self, descriptor: AccountDescriptor) -> (Self, AccountId) {
        let account_id = AccountId(self.next_account_id);
        self.next_account_id += 1;

        let account = Account {
            account_id,
            account_type: descriptor.account_type,
            assets: Vec::new(),
        };

        self.config.accounts.push(account);
        self.metadata.accounts.insert(
            account_id,
            EntityMetadata {
                name: descriptor.name,
                description: descriptor.description,
            },
        );

        (self, account_id)
    }

    /// Add an asset to an existing account using a descriptor
    pub fn add_asset(
        mut self,
        account_id: AccountId,
        descriptor: AssetDescriptor,
    ) -> (Self, AssetId) {
        let asset_id = AssetId(self.next_asset_id);
        self.next_asset_id += 1;

        let asset = Asset {
            asset_id,
            asset_class: descriptor.asset_class,
            initial_value: descriptor.initial_value,
            return_profile_index: descriptor.return_profile_index,
        };

        // Find the account and add the asset
        if let Some(account) = self
            .config
            .accounts
            .iter_mut()
            .find(|a| a.account_id == account_id)
        {
            account.assets.push(asset);
        }

        self.metadata.assets.insert(
            asset_id,
            EntityMetadata {
                name: descriptor.name,
                description: descriptor.description,
            },
        );

        (self, asset_id)
    }

    /// Add a cash flow using a descriptor
    pub fn add_cash_flow(mut self, descriptor: CashFlowDescriptor) -> (Self, CashFlowId) {
        let cash_flow_id = CashFlowId(self.next_cash_flow_id);
        self.next_cash_flow_id += 1;

        let cash_flow = CashFlow {
            cash_flow_id,
            amount: descriptor.amount,
            repeats: descriptor.repeats,
            cash_flow_limits: descriptor.limits,
            adjust_for_inflation: descriptor.adjust_for_inflation,
            direction: descriptor.direction,
            state: descriptor.state,
        };

        self.config.cash_flows.push(cash_flow);
        self.metadata.cash_flows.insert(
            cash_flow_id,
            EntityMetadata {
                name: descriptor.name,
                description: descriptor.description,
            },
        );

        (self, cash_flow_id)
    }

    /// Add an event using a descriptor
    pub fn add_event(mut self, descriptor: EventDescriptor) -> (Self, EventId) {
        let event_id = EventId(self.next_event_id);
        self.next_event_id += 1;

        let event = Event {
            event_id,
            trigger: descriptor.trigger,
            effects: descriptor.effects,
            once: descriptor.once,
        };

        self.config.events.push(event);
        self.metadata.events.insert(
            event_id,
            EntityMetadata {
                name: descriptor.name,
                description: descriptor.description,
            },
        );

        (self, event_id)
    }

    /// Add a spending target using a descriptor
    pub fn add_spending_target(
        mut self,
        descriptor: SpendingTargetDescriptor,
    ) -> (Self, SpendingTargetId) {
        let spending_target_id = SpendingTargetId(self.next_spending_target_id);
        self.next_spending_target_id += 1;

        let spending_target = SpendingTarget {
            spending_target_id,
            amount: descriptor.amount,
            repeats: descriptor.repeats,
            net_amount_mode: descriptor.net_amount_mode,
            adjust_for_inflation: descriptor.adjust_for_inflation,
            withdrawal_strategy: descriptor.withdrawal_strategy,
            exclude_accounts: Vec::new(),
            state: descriptor.state,
        };

        self.config.spending_targets.push(spending_target);
        self.metadata.spending_targets.insert(
            spending_target_id,
            EntityMetadata {
                name: descriptor.name,
                description: descriptor.description,
            },
        );

        (self, spending_target_id)
    }

    /// Build and return the simulation configuration and metadata
    pub fn build(self) -> (SimulationConfig, SimulationMetadata) {
        (self.config, self.metadata)
    }
}
