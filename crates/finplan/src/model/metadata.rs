//! Simulation metadata for human-readable names and descriptions
//!
//! EntityMetadata provides optional names and descriptions for accounts,
//! assets, cash flows, events, and spending targets.

use super::ids::{AccountId, AssetId, CashFlowId, EventId, SpendingTargetId};
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
    pub cash_flows: HashMap<CashFlowId, EntityMetadata>,
    pub events: HashMap<EventId, EntityMetadata>,
    pub spending_targets: HashMap<SpendingTargetId, EntityMetadata>,
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

    pub fn get_cash_flow_name(&self, id: CashFlowId) -> Option<&str> {
        self.cash_flows.get(&id)?.name.as_deref()
    }

    pub fn get_event_name(&self, id: EventId) -> Option<&str> {
        self.events.get(&id)?.name.as_deref()
    }

    pub fn get_spending_target_name(&self, id: SpendingTargetId) -> Option<&str> {
        self.spending_targets.get(&id)?.name.as_deref()
    }
}
