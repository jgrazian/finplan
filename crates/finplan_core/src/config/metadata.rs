//! Simulation metadata for human-readable names and descriptions
//!
//! `EntityMetadata` provides optional names and descriptions for accounts,
//! assets, cash flows, events, and spending targets.
//!
//! `SimulationMetadata` provides bidirectional mappings between string names
//! and IDs, enabling the builder DSL to use human-readable names.

use crate::model::{AccountId, AssetId, EventId, ReturnProfileId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metadata entry for any simulation entity
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EntityMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Holds human-readable names and descriptions for simulation entities,
/// along with bidirectional mappings for name-based lookups.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimulationMetadata {
    /// Account ID to metadata mapping
    pub accounts: HashMap<AccountId, EntityMetadata>,
    /// Asset ID to metadata mapping  
    pub assets: HashMap<AssetId, EntityMetadata>,
    /// Event ID to metadata mapping
    pub events: HashMap<EventId, EntityMetadata>,
    /// Return profile ID to metadata mapping
    pub return_profiles: HashMap<ReturnProfileId, EntityMetadata>,

    /// Name to Account ID reverse lookup
    #[serde(default)]
    pub account_names: HashMap<String, AccountId>,
    /// Name to Asset ID reverse lookup
    #[serde(default)]
    pub asset_names: HashMap<String, AssetId>,
    /// Name to Event ID reverse lookup
    #[serde(default)]
    pub event_names: HashMap<String, EventId>,
    /// Name to Return Profile ID reverse lookup
    #[serde(default)]
    pub return_profile_names: HashMap<String, ReturnProfileId>,
}

impl SimulationMetadata {
    /// Create a new empty metadata instance
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an account with optional name and description
    pub fn register_account(
        &mut self,
        id: AccountId,
        name: Option<String>,
        description: Option<String>,
    ) {
        if let Some(ref n) = name {
            self.account_names.insert(n.clone(), id);
        }
        self.accounts
            .insert(id, EntityMetadata { name, description });
    }

    /// Register an asset with optional name and description
    pub fn register_asset(
        &mut self,
        id: AssetId,
        name: Option<String>,
        description: Option<String>,
    ) {
        if let Some(ref n) = name {
            self.asset_names.insert(n.clone(), id);
        }
        self.assets.insert(id, EntityMetadata { name, description });
    }

    /// Register an event with optional name and description
    pub fn register_event(
        &mut self,
        id: EventId,
        name: Option<String>,
        description: Option<String>,
    ) {
        if let Some(ref n) = name {
            self.event_names.insert(n.clone(), id);
        }
        self.events.insert(id, EntityMetadata { name, description });
    }

    /// Register a return profile with optional name and description
    pub fn register_return_profile(
        &mut self,
        id: ReturnProfileId,
        name: Option<String>,
        description: Option<String>,
    ) {
        if let Some(ref n) = name {
            self.return_profile_names.insert(n.clone(), id);
        }
        self.return_profiles
            .insert(id, EntityMetadata { name, description });
    }

    /// Look up an account ID by name
    #[must_use]
    pub fn account_id(&self, name: &str) -> Option<AccountId> {
        self.account_names.get(name).copied()
    }

    /// Look up an asset ID by name
    #[must_use]
    pub fn asset_id(&self, name: &str) -> Option<AssetId> {
        self.asset_names.get(name).copied()
    }

    /// Look up an event ID by name
    #[must_use]
    pub fn event_id(&self, name: &str) -> Option<EventId> {
        self.event_names.get(name).copied()
    }

    /// Look up a return profile ID by name
    #[must_use]
    pub fn return_profile_id(&self, name: &str) -> Option<ReturnProfileId> {
        self.return_profile_names.get(name).copied()
    }

    /// Get the name of an account by ID
    #[must_use]
    pub fn account_name(&self, id: AccountId) -> Option<&str> {
        self.accounts.get(&id).and_then(|m| m.name.as_deref())
    }

    /// Get the name of an asset by ID
    #[must_use]
    pub fn asset_name(&self, id: AssetId) -> Option<&str> {
        self.assets.get(&id).and_then(|m| m.name.as_deref())
    }

    /// Get the name of an event by ID
    #[must_use]
    pub fn event_name(&self, id: EventId) -> Option<&str> {
        self.events.get(&id).and_then(|m| m.name.as_deref())
    }

    /// Get the name of a return profile by ID
    #[must_use]
    pub fn return_profile_name(&self, id: ReturnProfileId) -> Option<&str> {
        self.return_profiles
            .get(&id)
            .and_then(|m| m.name.as_deref())
    }
}
