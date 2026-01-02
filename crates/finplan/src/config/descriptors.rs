//! Descriptor structs for the Builder API
//!
//! Descriptors are used with SimulationBuilder to create entities without
//! manually assigning IDs. The builder assigns IDs automatically.

use crate::model::{AccountType, AssetClass, EventEffect, EventTrigger};

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
