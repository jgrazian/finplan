//! Descriptor structs for the Builder API
//!
//! Descriptors are used with SimulationBuilder to create entities without
//! manually assigning IDs. The builder assigns IDs automatically.

use crate::model::{
    AccountId, AccountType, AssetClass, AssetId, CashFlowDirection, CashFlowLimits, CashFlowState,
    EventEffect, EventTrigger, RepeatInterval, SpendingTargetState, WithdrawalStrategy,
};

/// Descriptor for creating an account (without ID)
#[derive(Debug, Clone)]
pub struct AccountDescriptor {
    pub account_type: AccountType,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl AccountDescriptor {
    pub fn new(account_type: AccountType) -> Self {
        Self {
            account_type,
            name: None,
            description: None,
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Descriptor for creating an asset (without ID)
#[derive(Debug, Clone)]
pub struct AssetDescriptor {
    pub asset_class: AssetClass,
    pub initial_value: f64,
    pub return_profile_index: usize,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl AssetDescriptor {
    pub fn new(asset_class: AssetClass, initial_value: f64, return_profile_index: usize) -> Self {
        Self {
            asset_class,
            initial_value,
            return_profile_index,
            name: None,
            description: None,
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Descriptor for creating a cash flow (without ID)
#[derive(Debug, Clone)]
pub struct CashFlowDescriptor {
    pub amount: f64,
    pub repeats: RepeatInterval,
    pub direction: CashFlowDirection,
    pub adjust_for_inflation: bool,
    pub state: CashFlowState,
    pub limits: Option<CashFlowLimits>,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl CashFlowDescriptor {
    pub fn new(amount: f64, repeats: RepeatInterval, direction: CashFlowDirection) -> Self {
        Self {
            amount,
            repeats,
            direction,
            adjust_for_inflation: false,
            state: CashFlowState::Pending,
            limits: None,
            name: None,
            description: None,
        }
    }

    /// Create an income CashFlow (External → Asset)
    pub fn income(
        amount: f64,
        repeats: RepeatInterval,
        target_account_id: AccountId,
        target_asset_id: AssetId,
    ) -> Self {
        Self::new(
            amount,
            repeats,
            CashFlowDirection::Income {
                target_account_id,
                target_asset_id,
            },
        )
    }

    /// Create an expense CashFlow (Asset → External)
    pub fn expense(
        amount: f64,
        repeats: RepeatInterval,
        source_account_id: AccountId,
        source_asset_id: AssetId,
    ) -> Self {
        Self::new(
            amount,
            repeats,
            CashFlowDirection::Expense {
                source_account_id,
                source_asset_id,
            },
        )
    }

    pub fn adjust_for_inflation(mut self, adjust: bool) -> Self {
        self.adjust_for_inflation = adjust;
        self
    }

    pub fn state(mut self, state: CashFlowState) -> Self {
        self.state = state;
        self
    }

    pub fn limits(mut self, limits: CashFlowLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Descriptor for creating an event (without ID)
#[derive(Debug, Clone)]
pub struct EventDescriptor {
    pub trigger: EventTrigger,
    pub effects: Vec<EventEffect>,
    pub once: bool,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl EventDescriptor {
    pub fn new(trigger: EventTrigger, effects: Vec<EventEffect>) -> Self {
        Self {
            trigger,
            effects,
            once: false,
            name: None,
            description: None,
        }
    }

    pub fn once(mut self) -> Self {
        self.once = true;
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Descriptor for creating a spending target (without ID)
#[derive(Debug, Clone)]
pub struct SpendingTargetDescriptor {
    pub amount: f64,
    pub repeats: RepeatInterval,
    pub withdrawal_strategy: WithdrawalStrategy,
    pub adjust_for_inflation: bool,
    pub net_amount_mode: bool,
    pub state: SpendingTargetState,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl SpendingTargetDescriptor {
    pub fn new(amount: f64, repeats: RepeatInterval) -> Self {
        Self {
            amount,
            repeats,
            withdrawal_strategy: WithdrawalStrategy::default(),
            adjust_for_inflation: false,
            net_amount_mode: false,
            state: SpendingTargetState::Pending,
            name: None,
            description: None,
        }
    }

    pub fn withdrawal_strategy(mut self, strategy: WithdrawalStrategy) -> Self {
        self.withdrawal_strategy = strategy;
        self
    }

    pub fn adjust_for_inflation(mut self, adjust: bool) -> Self {
        self.adjust_for_inflation = adjust;
        self
    }

    pub fn net_amount_mode(mut self, net: bool) -> Self {
        self.net_amount_mode = net;
        self
    }

    pub fn state(mut self, state: SpendingTargetState) -> Self {
        self.state = state;
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}
