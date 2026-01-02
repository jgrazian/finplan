//! Simulation metadata for human-readable names and descriptions
//!
//! EntityMetadata provides optional names and descriptions for accounts,
//! assets, cash flows, events, and spending targets.

use super::ids::{AccountId, AssetId, EventId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metadata entry for any simulation entity
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EntityMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Holds human-readable names and descriptions for simulation entities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimulationMetadata {
    pub accounts: HashMap<AccountId, EntityMetadata>,
    pub assets: HashMap<AssetId, EntityMetadata>,
    pub events: HashMap<EventId, EntityMetadata>,
}

impl SimulationMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_account_name(&self, id: AccountId) -> Option<&str> {
        self.accounts.get(&id)?.name.as_deref()
    }

    pub fn get_asset_name(&self, id: AssetId) -> Option<&str> {
        self.assets.get(&id)?.name.as_deref()
    }

    pub fn get_event_name(&self, id: EventId) -> Option<&str> {
        self.events.get(&id)?.name.as_deref()
    }
}
